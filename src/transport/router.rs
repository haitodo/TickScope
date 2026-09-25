//! Shared loopback listener that routes EA connections to broker receivers.

use crate::contracts::types::BrokerId;
use crate::transport::tcp::TransportReceiver;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::io::Read;
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

const ROUTE_HELLO_MAGIC: [u8; 4] = *b"TSCP";
const ROUTE_HELLO_LENGTH: usize = 8;

struct ActiveConnectionGuard(Arc<AtomicBool>);

impl Drop for ActiveConnectionGuard {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

pub struct TransportRouter {
    listener: TcpListener,
    port: u16,
    receivers: HashMap<BrokerId, Arc<TransportReceiver>>,
    active_connections: HashMap<BrokerId, Arc<AtomicBool>>,
    generations: HashMap<BrokerId, AtomicU64>,
    connection_threads: Mutex<Vec<thread::JoinHandle<()>>>,
    progress_interval: Duration,
    running: AtomicBool,
}

impl TransportRouter {
    pub fn bind_loopback(
        receivers: HashMap<BrokerId, Arc<TransportReceiver>>,
        progress_interval_ms: u64,
    ) -> Result<Self, String> {
        let listener = TcpListener::bind(("127.0.0.1", 0))
            .map_err(|error| format!("Failed to bind the shared MT5 loopback port: {error}"))?;
        let port = listener
            .local_addr()
            .map_err(|error| format!("Failed to inspect the shared MT5 loopback port: {error}"))?
            .port();
        listener
            .set_nonblocking(true)
            .map_err(|error| format!("Failed to configure the shared MT5 listener: {error}"))?;

        let active_connections = receivers
            .keys()
            .map(|&broker_id| (broker_id, Arc::new(AtomicBool::new(false))))
            .collect();
        let generations = receivers
            .keys()
            .map(|&broker_id| (broker_id, AtomicU64::new(0)))
            .collect();

        Ok(Self {
            listener,
            port,
            receivers,
            active_connections,
            generations,
            connection_threads: Mutex::new(Vec::new()),
            progress_interval: Duration::from_millis(progress_interval_ms.max(1)),
            running: AtomicBool::new(true),
        })
    }

    pub fn local_port(&self) -> u16 {
        self.port
    }

    pub fn stop(&self) {
        self.running.store(false, Ordering::Release);
    }

    pub fn run(&self) {
        let mut last_progress = Instant::now();
        while self.running.load(Ordering::Acquire) {
            match self.listener.accept() {
                Ok((stream, _peer_addr)) => self.route(stream),
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    if last_progress.elapsed() >= self.progress_interval {
                        for receiver in self.receivers.values() {
                            receiver.report_idle_progress();
                        }
                        last_progress = Instant::now();
                    }
                    thread::sleep(Duration::from_millis(1));
                }
                Err(error) => {
                    eprintln!("Shared MT5 listener accept failed: {error}");
                    thread::sleep(Duration::from_millis(50));
                }
            }
        }

        let handles = std::mem::take(&mut *self.connection_threads.lock());
        for handle in handles {
            let _ = handle.join();
        }
    }

    fn route(&self, mut stream: TcpStream) {
        stream.set_nodelay(true).ok();
        if let Err(error) = stream.set_read_timeout(Some(Duration::from_secs(2))) {
            eprintln!("Failed to configure route handshake timeout: {error}");
            return;
        }

        let mut hello = [0u8; ROUTE_HELLO_LENGTH];
        if let Err(error) = stream.read_exact(&mut hello) {
            eprintln!("Rejected MT5 connection without a complete route handshake: {error}");
            return;
        }
        if hello[..4] != ROUTE_HELLO_MAGIC[..] {
            eprintln!("Rejected MT5 connection with an invalid route handshake.");
            return;
        }

        let broker_id = u32::from_le_bytes(hello[4..8].try_into().unwrap());
        let Some(receiver) = self.receivers.get(&broker_id).cloned() else {
            eprintln!("Rejected MT5 connection for unknown broker id {broker_id}.");
            return;
        };
        if let Err(error) = stream.set_nonblocking(true) {
            eprintln!("Failed to configure broker {broker_id} connection: {error}");
            return;
        }
        let Some(active) = self.active_connections.get(&broker_id).cloned() else {
            return;
        };
        if active
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            eprintln!("Rejected a duplicate active MT5 connection for broker {broker_id}.");
            return;
        }

        let generation = self
            .generations
            .get(&broker_id)
            .map(|counter| counter.fetch_add(1, Ordering::AcqRel) + 1)
            .unwrap_or(1);
        stream.set_read_timeout(None).ok();

        let active_for_thread = active.clone();
        let spawn_result = thread::Builder::new()
            .name(format!("mt5-broker-{broker_id}"))
            .spawn(move || {
                let _active_guard = ActiveConnectionGuard(active_for_thread);
                receiver.handle_routed_connection(stream, generation);
            });
        match spawn_result {
            Ok(handle) => {
                let mut handles = self.connection_threads.lock();
                let mut index = 0;
                while index < handles.len() {
                    if handles[index].is_finished() {
                        let finished = handles.swap_remove(index);
                        let _ = finished.join();
                    } else {
                        index += 1;
                    }
                }
                handles.push(handle);
            }
            Err(error) => {
                active.store(false, Ordering::Release);
                eprintln!("Failed to start MT5 receiver for broker {broker_id}: {error}");
            }
        }
    }
}
