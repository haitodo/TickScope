//! TCP listener and receiver per broker.
//! Reference: docs/blueprint/interfaces.md and docs/blueprint/invariants.md

use crate::contracts::config::BrokerConfig;
use crate::contracts::ports::{ClockPort, LogSinkPort, RawIngressSink, SubmitResult};
use crate::contracts::types::*;
use crate::protocol::codec::{encode_frame, StreamingDecoder};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

pub struct TransportReceiver {
    broker_config: BrokerConfig,
    ack_mode: String,
    clock: Arc<dyn ClockPort>,
    ingress_sink: Arc<dyn RawIngressSink>,
    log_sink: Option<Arc<dyn LogSinkPort>>,
    max_payload_length: usize,
    debug_resync_limit: usize,
    progress_interval: Duration,
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
            max_payload_length,
            debug_resync_limit,
            progress_interval: Duration::from_millis(progress_interval_ms.max(1)),
            running: Arc::new(AtomicBool::new(true)),
        }
    }

    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
    }

    pub fn run(&self) {
        let addr = format!("{}:{}", self.broker_config.host, self.broker_config.port);
        // Keep the receiver alive while the port is temporarily unavailable.
        // This covers app restarts and startup races with another receiver.
        let listener = loop {
            if !self.running.load(Ordering::SeqCst) {
                return;
            }
            match TcpListener::bind(&addr) {
                Ok(l) => {
                    l.set_nonblocking(true).ok();
                    break l;
                }
                Err(e) => {
                    eprintln!(
                        "Failed to bind TCP listener on {} for broker {}: {}. Retrying...",
                        addr, self.broker_config.id, e
                    );
                    thread::sleep(Duration::from_millis(250));
                }
            }
        };

        let mut generation: u64 = 0;
        let mut last_progress_time = std::time::Instant::now();

        while self.running.load(Ordering::SeqCst) {
            // Accept one connection at a time per broker
            match listener.accept() {
                Ok((stream, _peer_addr)) => {
                    generation += 1;
                    stream.set_nodelay(true).ok();
                    stream.set_nonblocking(true).ok();

                    let connected_mono = self.clock.sample().mono_ns;
                    self.submit_ingress_item(IngressItem::Connected {
                        broker_id: self.broker_config.id,
                        generation,
                        connected_at_mono: connected_mono,
                    });

                    let end_reason = self.handle_connection(stream, generation);

                    self.submit_ingress_item(IngressItem::End {
                        broker_id: self.broker_config.id,
                        generation,
                        reason: end_reason,
                    });
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
                    eprintln!("Listener accept error for broker {}: {}", self.broker_config.id, e);
                    thread::sleep(Duration::from_millis(50));
                }
            }
        }
    }

    fn handle_connection(&self, mut stream: TcpStream, generation: u64) -> String {
        let mut decoder = StreamingDecoder::new(self.max_payload_length, self.debug_resync_limit);
        let mut read_buf = [0u8; 8192];
        let mut frame_index: u64 = 0;
        let mut last_progress = std::time::Instant::now();

        while self.running.load(Ordering::SeqCst) {
            match stream.read(&mut read_buf) {
                Ok(0) => {
                    // Clean EOF from client
                    return "Client closed connection (clean EOF)".to_string();
                }
                Ok(n) => {
                    if let Err(e) = decoder.push(&read_buf[..n]) {
                        eprintln!(
                            "Decoder push error for broker {}: {}, terminating connection",
                            self.broker_config.id, e
                        );
                        return format!("Decoder push error: {}", e);
                    }

                    // Decode all complete frames
                    let mut read_error = None;
                    loop {
                        match decoder.next_frame() {
                            Ok(Some(decoded)) => {
                                frame_index += 1;
                                let clk = self.clock.sample();

                                let mut frame = decoded.frame.clone();
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

                                if let Err(error) = self.persist_raw_frame(&rx_frame) {
                                    return format!("Raw log durability failure: {error}");
                                }

                                if !self.submit_ingress_item(IngressItem::Frame(rx_frame)) {
                                    return "Ingress closed before frame acceptance".to_string();
                                }

                                // A batch is acknowledged only after its raw wire frame has
                                // reached stable storage and the bounded ingress accepted it.
                                if self.ack_mode != "off" && decoded.frame.header.message_type == MSG_TYPE_TICK_BATCH {
                                    let seq_end = decoded.frame.header.sequence_start
                                        + decoded.frame.header.tick_count as u64
                                        - 1;
                                    if let Err(error) = self.send_batch_ack(
                                        &mut stream,
                                        decoded.frame.header.session_id,
                                        seq_end,
                                    ) {
                                        return format!("Batch ACK write failed: {error}");
                                    }
                                }
                            }
                            Ok(None) => {
                                break;
                            }
                            Err(e) => {
                                eprintln!(
                                    "Malformed frame from broker {}: {}, closing connection",
                                    self.broker_config.id, e
                                );
                                read_error = Some(format!("Malformed frame: {}", e));
                                break;
                            }
                        }
                    }

                    if let Some(err_msg) = read_error {
                        return err_msg;
                    }
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    if last_progress.elapsed() >= self.progress_interval {
                        let sample = self.clock.sample();
                        self.submit_ingress_item(IngressItem::Progress {
                            broker_id: self.broker_config.id,
                            watermark_ns: sample.mono_ns,
                        });
                        last_progress = std::time::Instant::now();
                    }
                    thread::sleep(Duration::from_micros(200));
                }
                Err(e) => {
                    // Socket error or disconnected
                    return format!("Socket read error: {}", e);
                }
            }
        }

        "Server stopped".to_string()
    }

    fn submit_ingress_item(&self, mut item: IngressItem) -> bool {
        while self.running.load(Ordering::SeqCst) {
            match self.ingress_sink.try_submit(item) {
                SubmitResult::Accepted => return true,
                SubmitResult::Full(returned_item) => {
                    item = returned_item;
                    thread::sleep(Duration::from_micros(200));
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
