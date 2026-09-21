//! TCP listener and receiver per broker.
//! Reference: docs/blueprint/interfaces.md and docs/blueprint/invariants.md

use crate::contracts::config::BrokerConfig;
use crate::contracts::ports::{ClockPort, RawIngressSink, SubmitResult};
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
    clock: Arc<dyn ClockPort>,
    ingress_sink: Arc<dyn RawIngressSink>,
    running: Arc<AtomicBool>,
}

impl TransportReceiver {
    pub fn new(
        broker_config: BrokerConfig,
        clock: Arc<dyn ClockPort>,
        ingress_sink: Arc<dyn RawIngressSink>,
    ) -> Self {
        Self {
            broker_config,
            clock,
            ingress_sink,
            running: Arc::new(AtomicBool::new(true)),
        }
    }

    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
    }

    pub fn run(&self) {
        let addr = format!("{}:{}", self.broker_config.host, self.broker_config.port);
        let listener = match TcpListener::bind(&addr) {
            Ok(l) => {
                l.set_nonblocking(true).ok();
                l
            }
            Err(e) => {
                eprintln!(
                    "Failed to bind TCP listener on {} for broker {}: {}",
                    addr, self.broker_config.id, e
                );
                return;
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

                    self.handle_connection(stream, generation);

                    self.submit_ingress_item(IngressItem::End {
                        broker_id: self.broker_config.id,
                        generation,
                        reason: "Connection closed".to_string(),
                    });
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    // Send idle progress watermark
                    if last_progress_time.elapsed() >= Duration::from_millis(1) {
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

    fn handle_connection(&self, mut stream: TcpStream, generation: u64) {
        let mut decoder = StreamingDecoder::new(1_048_576, 65_536);
        let mut read_buf = [0u8; 8192];
        let mut frame_index: u64 = 0;
        let mut last_progress = std::time::Instant::now();

        while self.running.load(Ordering::SeqCst) {
            match stream.read(&mut read_buf) {
                Ok(0) => {
                    // Clean EOF
                    break;
                }
                Ok(n) => {
                    if let Err(e) = decoder.push(&read_buf[..n]) {
                        eprintln!(
                            "Decoder push error for broker {}: {}, terminating connection",
                            self.broker_config.id, e
                        );
                        break;
                    }

                    // Decode all complete frames
                    let mut read_error = false;
                    loop {
                        match decoder.next_frame() {
                            Ok(Some(decoded)) => {
                                frame_index += 1;
                                let clk = self.clock.sample();

                                let rx_frame = ReceivedFrame {
                                    frame: decoded.frame.clone(),
                                    raw_wire_bytes: decoded.raw_wire_bytes,
                                    run_id: clk.run_id,
                                    rx_mono_ns: clk.mono_ns,
                                    rx_unix_ns: clk.unix_ns,
                                    connection_generation: generation,
                                    frame_index,
                                };

                                self.submit_ingress_item(IngressItem::Frame(rx_frame));

                                // If batch ACK is enabled or requested
                                if decoded.frame.header.message_type == MSG_TYPE_TICK_BATCH {
                                    let seq_end = decoded.frame.header.sequence_start
                                        + decoded.frame.header.tick_count as u64
                                        - 1;
                                    self.send_batch_ack(&mut stream, generation, seq_end);
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
                                read_error = true;
                                break;
                            }
                        }
                    }

                    if read_error {
                        break;
                    }
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    if last_progress.elapsed() >= Duration::from_millis(1) {
                        let sample = self.clock.sample();
                        self.submit_ingress_item(IngressItem::Progress {
                            broker_id: self.broker_config.id,
                            watermark_ns: sample.mono_ns,
                        });
                        last_progress = std::time::Instant::now();
                    }
                    thread::sleep(Duration::from_micros(200));
                }
                Err(_e) => {
                    // Socket error or disconnected
                    break;
                }
            }
        }
    }

    fn submit_ingress_item(&self, mut item: IngressItem) {
        while self.running.load(Ordering::SeqCst) {
            match self.ingress_sink.try_submit(item) {
                SubmitResult::Accepted => return,
                SubmitResult::Full(returned_item) => {
                    item = returned_item;
                    thread::sleep(Duration::from_micros(200));
                }
                SubmitResult::Closed(_) => return,
            }
        }
    }

    fn send_batch_ack(&self, stream: &mut TcpStream, _generation: u64, seq_end: u64) {
        let ack_frame = Frame {
            header: Header {
                magic: MAGIC_TICK,
                protocol_version: PROTOCOL_VERSION,
                message_type: MSG_TYPE_BATCH_ACK,
                header_length: HEADER_LENGTH,
                header_flags: 0,
                broker_id: self.broker_config.id,
                session_id: 0,
                sequence_start: 0,
                tick_count: 0,
                payload_length: BATCH_ACK_PAYLOAD_LENGTH as u32,
            },
            payload: FramePayload::BatchAck(BatchAckPayload {
                sequence_end: seq_end,
            }),
        };

        if let Ok(bytes) = encode_frame(&ack_frame) {
            let _ = stream.write_all(&bytes);
        }
    }
}
