//! TCP listener and receiver per broker.

use crate::config::BrokerConfig;
use crate::core::ports::{ClockPort, LogSinkPort, RawIngressSink, SubmitResult};
use crate::core::types::*;
use crate::metrics::diagnostics::{DiagnosticStage, DiagnosticsHandle};
use crate::protocol::codec::{encode_frame, StreamingDecoder};
use crate::protocol::*;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

pub struct TransportReceiver {
    broker_config: BrokerConfig,
    ack_mode: String,
    clock: Arc<dyn ClockPort>,
    ingress_sink: Arc<dyn RawIngressSink>,
    log_sink: Option<Arc<dyn LogSinkPort>>,
    diagnostics: Option<DiagnosticsHandle>,
    max_payload_length: usize,
    debug_resync_limit: usize,
    progress_interval: Duration,
    activity_timeout: Duration,
    running: Arc<AtomicBool>,
}

impl TransportReceiver {
    pub fn new(
        broker_config: BrokerConfig,
        ack_mode: String,
        clock: Arc<dyn ClockPort>,
        ingress_sink: Arc<dyn RawIngressSink>,
    ) -> Self {
        Self::new_with_limits(
            broker_config,
            ack_mode,
            1_048_576,
            65_536,
            1,
            None,
            clock,
            ingress_sink,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new_with_limits(
        broker_config: BrokerConfig,
        ack_mode: String,
        max_payload_length: usize,
        debug_resync_limit: usize,
        progress_interval_ms: u64,
        log_sink: Option<Arc<dyn LogSinkPort>>,
        clock: Arc<dyn ClockPort>,
        ingress_sink: Arc<dyn RawIngressSink>,
    ) -> Self {
        Self {
            broker_config,
            ack_mode,
            clock,
            ingress_sink,
            log_sink,
            diagnostics: None,
            max_payload_length,
            debug_resync_limit,
            progress_interval: Duration::from_millis(progress_interval_ms.max(1)),
            activity_timeout: Duration::from_secs(3),
            running: Arc::new(AtomicBool::new(true)),
        }
    }

    pub const fn with_activity_timeout(mut self, timeout: Duration) -> Self {
        self.activity_timeout = timeout;
        self
    }

    pub fn with_diagnostics(mut self, diagnostics: DiagnosticsHandle) -> Self {
        self.diagnostics = Some(diagnostics);
        self
    }

    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
    }

    pub fn run(&self) {
        let addr = format!("{}:{}", self.broker_config.host, self.broker_config.port);
        // Keep the receiver alive while the configured legacy port is
        // temporarily unavailable. This covers app restarts and startup races.
        let listener = loop {
            if !self.running.load(Ordering::SeqCst) {
                return;
            }
            match TcpListener::bind(&addr) {
                Ok(listener) => break listener,
                Err(error) => {
                    log::warn!(
                        "Failed to bind TCP listener on {} for broker {}: {}. Retrying...",
                        addr, self.broker_config.id, error
                    );
                    thread::sleep(Duration::from_millis(250));
                }
            }
        };
        listener.set_nonblocking(true).ok();

        let mut generation: u64 = 0;
        let mut last_progress_time = std::time::Instant::now();

        while self.running.load(Ordering::SeqCst) {
            // Accept one connection at a time per broker
            match listener.accept() {
                Ok((stream, _peer_addr)) => {
                    generation += 1;
                    stream.set_nodelay(true).ok();
                    stream.set_nonblocking(true).ok();
                    self.process_accepted_connection(stream, generation);
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    // Send idle progress watermark
                    if last_progress_time.elapsed() >= self.progress_interval {
                        let sample = self.clock.sample();
                        self.submit_ingress_item(IngressItem::Progress {
                            broker_id: self.broker_config.id,
                            watermark_ns: sample.mono_ns,
                        });
                        last_progress_time = std::time::Instant::now();
                    }
                    thread::sleep(Duration::from_millis(1));
                }
                Err(e) => {
                    log::error!("Listener accept error for broker {}: {}", self.broker_config.id, e);
                    thread::sleep(Duration::from_millis(50));
                }
            }
        }
    }

    pub(crate) fn handle_routed_connection(&self, stream: TcpStream, generation: u64) {
        stream.set_nodelay(true).ok();
        stream.set_nonblocking(true).ok();
        self.process_accepted_connection(stream, generation);
    }

    pub(crate) fn report_idle_progress(&self) {
        let sample = self.clock.sample();
        self.submit_ingress_item(IngressItem::Progress {
            broker_id: self.broker_config.id,
            watermark_ns: sample.mono_ns,
        });
    }

    fn process_accepted_connection(&self, stream: TcpStream, generation: u64) {
        log::info!(
            "Broker {} connection established (generation: {})",
            self.broker_config.id,
            generation
        );
        let connected_mono = self.clock.sample().mono_ns;
        self.submit_ingress_item(IngressItem::Connected {
            broker_id: self.broker_config.id,
            generation,
            connected_at_mono: connected_mono,
        });

        let end_reason = self.handle_connection(stream, generation);
        log::info!(
            "Broker {} connection closed (generation: {}, reason: {})",
            self.broker_config.id,
            generation,
            end_reason
        );

        self.submit_ingress_item(IngressItem::End {
            broker_id: self.broker_config.id,
            generation,
            reason: end_reason,
        });
    }

    fn handle_connection(&self, mut stream: TcpStream, generation: u64) -> String {
        // Wait in the socket so arriving data wakes us immediately, rather than
        // waiting for a polling sleep (especially costly on Windows).
        if let Err(error) = stream.set_nonblocking(false)
            .and_then(|()| stream.set_read_timeout(Some(self.progress_interval)))
            .and_then(|()| stream.set_write_timeout(Some(Duration::from_secs(1))))
        {
            return format!("Socket configuration failed: {error}");
        }
        stream.set_nodelay(true).ok();
        let mut decoder = StreamingDecoder::new_with_raw_capture(
            self.max_payload_length,
            self.debug_resync_limit,
            self.log_sink.is_some(),
        );
        let mut read_buf = [0u8; 8192];
        let mut frame_index: u64 = 0;
        let mut last_progress = std::time::Instant::now();
        let mut last_activity = std::time::Instant::now();

        while self.running.load(Ordering::SeqCst) {
            match stream.read(&mut read_buf) {
                Ok(0) => {
                    // Clean EOF from client
                    return "Client closed connection (clean EOF)".to_string();
                }
                Ok(n) => {
                    last_activity = std::time::Instant::now();
                    // All frames completed by this read share its observation
                    // time; decoding and durable writes must not skew it.
                    let clk = self.clock.sample();
                    if let Err(e) = decoder.push(&read_buf[..n]) {
                        log::error!(
                            "Decoder push error for broker {}: {}, terminating connection",
                            self.broker_config.id, e
                        );
                        return format!("Decoder push error: {e}");
                    }

                    // Decode all complete frames
                    let mut read_error = None;
                    loop {
                        match decoder.next_frame() {
                            Ok(Some(decoded)) => {
                                frame_index += 1;
                                let ack = if self.ack_mode != "off" && decoded.frame.header.message_type == MSG_TYPE_TICK_BATCH {
                                    Some((decoded.frame.header.session_id,
                                        decoded.frame.header.sequence_start + u64::from(decoded.frame.header.tick_count) - 1))
                                } else {
                                    None
                                };
                                let mut frame = decoded.frame;
                                frame.header.broker_id = self.broker_config.id;

                                let rx_frame = ReceivedFrame {
                                    frame,
                                    raw_wire_bytes: decoded.raw_wire_bytes,
                                    run_id: clk.run_id,
                                    rx_mono_ns: clk.mono_ns,
                                    rx_unix_ns: clk.unix_ns,
                                    connection_generation: generation,
                                    frame_index,
                                };

                                if self.log_sink.is_some() {
                                    let persist_start =
                                        self.diagnostics.as_ref().map(|_| Instant::now());
                                    if let Err(error) = self.persist_raw_frame(&rx_frame) {
                                        return format!("Raw log durability failure: {error}");
                                    }
                                    if let (Some(diagnostics), Some(start)) =
                                        (&self.diagnostics, persist_start)
                                    {
                                        diagnostics.record_duration(
                                            DiagnosticStage::RawPersistence,
                                            start.elapsed(),
                                        );
                                    }
                                }

                                if !self.submit_ingress_item(IngressItem::Frame(rx_frame)) {
                                    return "Ingress closed before frame acceptance".to_string();
                                }
                                if let Some(diagnostics) = &self.diagnostics {
                                    let accepted_mono = self.clock.sample().mono_ns;
                                    diagnostics.record_ns(
                                        DiagnosticStage::TcpReceiveToIngress,
                                        accepted_mono.saturating_sub(clk.mono_ns).0,
                                    );
                                }

                                // In raw-capture mode, stable storage completes before the
                                // bounded ingress accepts the frame. Otherwise ACK marks only
                                // volatile ingress acceptance.
                                if let Some((session_id, seq_end)) = ack {
                                    let ack_write_start =
                                        self.diagnostics.as_ref().map(|_| Instant::now());
                                    if let Err(error) = self.send_batch_ack(
                                        &mut stream,
                                        session_id,
                                        seq_end,
                                    ) {
                                        return format!("Batch ACK write failed: {error}");
                                    }
                                    if let (Some(diagnostics), Some(start)) =
                                        (&self.diagnostics, ack_write_start)
                                    {
                                        diagnostics.record_ns(
                                            DiagnosticStage::TcpReceiveToAck,
                                            self.clock
                                                .sample()
                                                .mono_ns
                                                .saturating_sub(clk.mono_ns)
                                                .0,
                                        );
                                        diagnostics.record_duration(
                                            DiagnosticStage::BatchAckWrite,
                                            start.elapsed(),
                                        );
                                    }
                                }
                            }
                            Ok(None) => {
                                break;
                            }
                            Err(e) => {
                                log::warn!(
                                    "Malformed frame from broker {}: {}, closing connection",
                                    self.broker_config.id, e
                                );
                                read_error = Some(format!("Malformed frame: {e}"));
                                break;
                            }
                        }
                    }

                    if let Some(err_msg) = read_error {
                        return err_msg;
                    }
                }
                Err(ref e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {
                    if !self.activity_timeout.is_zero() && last_activity.elapsed() >= self.activity_timeout {
                        return format!(
                            "Connection activity timeout: no data or heartbeat received for {:.1}s",
                            last_activity.elapsed().as_secs_f32()
                        );
                    }
                    if last_progress.elapsed() >= self.progress_interval {
                        let sample = self.clock.sample();
                        self.submit_ingress_item(IngressItem::Progress {
                            broker_id: self.broker_config.id,
                            watermark_ns: sample.mono_ns,
                        });
                        last_progress = std::time::Instant::now();
                    }
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(e) => {
                    // Socket error or disconnected
                    return format!("Socket read error: {e}");
                }
            }
        }

        "Server stopped".to_string()
    }

    fn submit_ingress_item(&self, mut item: IngressItem) -> bool {
        while self.running.load(Ordering::SeqCst) {
            match self.ingress_sink.submit_timeout(item, Duration::from_millis(20)) {
                SubmitResult::Accepted => return true,
                SubmitResult::Full(returned_item) => {
                    item = returned_item;
                    std::thread::yield_now();
                }
                SubmitResult::Closed(_) => return false,
            }
        }
        false
    }

    fn persist_raw_frame(&self, frame: &ReceivedFrame) -> Result<(), String> {
        let Some(sink) = &self.log_sink else {
            return Ok(());
        };
        sink.append_durable(Arc::new(LogRecord::RawFrame(LogRawFrame {
            broker_id: frame.frame.header.broker_id,
            connection_generation: frame.connection_generation,
            frame_index: frame.frame_index,
            rx_mono_ns: frame.rx_mono_ns,
            rx_unix_ns: frame.rx_unix_ns,
            config_epoch: 1,
            analysis_segment: 1,
            raw_wire_bytes: frame.raw_wire_bytes.clone(),
            // Replay recomputes dispositions from source session/sequence.
            dispositions: Vec::new(),
        })))
    }

    fn send_batch_ack(&self, stream: &mut TcpStream, session_id: SessionId, seq_end: u64) -> std::io::Result<()> {
        let ack_frame = Frame {
            header: Header {
                magic: MAGIC_TICK,
                protocol_version: PROTOCOL_VERSION,
                message_type: MSG_TYPE_BATCH_ACK,
                header_length: HEADER_LENGTH,
                header_flags: 0,
                broker_id: self.broker_config.id,
                session_id,
                sequence_start: 0,
                tick_count: 0,
                payload_length: BATCH_ACK_PAYLOAD_LENGTH as u32,
            },
            payload: FramePayload::BatchAck(BatchAckPayload {
                sequence_end: seq_end,
            }),
        };

        let bytes = encode_frame(&ack_frame)
            .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error.to_string()))?;
        stream.write_all(&bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::ports::AppendResult;
    use parking_lot::Mutex;
    use std::sync::atomic::AtomicU64;

    struct TestClock(AtomicU64);
    impl ClockPort for TestClock {
        fn sample(&self) -> ClockReading {
            ClockReading {
                run_id: RunId([1; 16]),
                mono_ns: MonoNs(self.0.load(Ordering::SeqCst)),
                unix_ns: None,
            }
        }
    }

    #[derive(Default)]
    struct Ingress(Mutex<Vec<IngressItem>>);
    impl RawIngressSink for Ingress {
        fn try_submit(&self, item: IngressItem) -> SubmitResult<IngressItem> {
            self.0.lock().push(item);
            SubmitResult::Accepted
        }
    }

    struct SlowLog(Arc<TestClock>);
    impl LogSinkPort for SlowLog {
        fn try_append(&self, _: Arc<LogRecord>) -> AppendResult<Arc<LogRecord>> {
            // Model time spent persisting a frame without wall-clock sleeps.
            self.0.0.fetch_add(1_000_000, Ordering::SeqCst);
            AppendResult::Accepted
        }
        fn flush(&self) -> Result<(), String> {
            Ok(())
        }
    }

    #[test]
    fn coalesced_frames_keep_read_timestamp_despite_storage_delay() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let mut client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (server, _) = listener.accept().unwrap();
        let frame = Frame {
            header: Header {
                magic: MAGIC_TICK,
                protocol_version: PROTOCOL_VERSION,
                message_type: MSG_TYPE_BATCH_ACK,
                header_length: HEADER_LENGTH,
                header_flags: 0,
                broker_id: 1,
                session_id: 1,
                sequence_start: 0,
                tick_count: 0,
                payload_length: BATCH_ACK_PAYLOAD_LENGTH as u32,
            },
            payload: FramePayload::BatchAck(BatchAckPayload { sequence_end: 1 }),
        };
        let bytes = encode_frame(&frame).unwrap().repeat(2);
        client.write_all(&bytes).unwrap();
        client.shutdown(std::net::Shutdown::Write).unwrap();

        // Ensure both frames are available before the receiver starts reading.
        let mut peek = [0; 96];
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        server.set_nonblocking(true).unwrap();
        while server.peek(&mut peek).unwrap_or(0) < bytes.len() {
            assert!(std::time::Instant::now() < deadline);
            thread::yield_now();
        }
        let clock = Arc::new(TestClock(AtomicU64::new(100)));
        let ingress = Arc::new(Ingress::default());
        let config = crate::config::AppConfig::default().brokers[0].clone();
        let receiver = TransportReceiver::new_with_limits(
            config,
            "off".into(),
            1_048_576,
            65_536,
            1,
            Some(Arc::new(SlowLog(clock.clone()))),
            clock,
            ingress.clone(),
        );
        assert!(receiver.handle_connection(server, 1).contains("clean EOF"));
        let items = ingress.0.lock();
        let times: Vec<_> = items
            .iter()
            .filter_map(|item| match item {
                IngressItem::Frame(frame) => Some(frame.rx_mono_ns),
                _ => None,
            })
            .collect();
        assert_eq!(times, vec![MonoNs(100), MonoNs(100)]);
    }
}
