//! Shared loopback listener that routes EA connections to broker receivers.

use crate::core::types::BrokerId;
use crate::protocol::bytes::le_u32;
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

struct ActiveConnectionGuard {
    broker_id: BrokerId,
    generation: u64,
    active_generations: Arc<Mutex<HashMap<BrokerId, u64>>>,
    active_sockets: Arc<Mutex<HashMap<BrokerId, TcpStream>>>,
}

impl Drop for ActiveConnectionGuard {
    fn drop(&mut self) {
        let mut gens = self.active_generations.lock();
        if let Some(&current_gen) = gens.get(&self.broker_id) {
            if current_gen == self.generation {
                gens.remove(&self.broker_id);
                self.active_sockets.lock().remove(&self.broker_id);
            }
        }
    }
}

pub struct TransportRouter {
    listener: TcpListener,
    port: u16,
    receivers: HashMap<BrokerId, Arc<TransportReceiver>>,
    active_generations: Arc<Mutex<HashMap<BrokerId, u64>>>,
    active_sockets: Arc<Mutex<HashMap<BrokerId, TcpStream>>>,
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

        let active_generations = Arc::new(Mutex::new(HashMap::new()));
        let active_sockets = Arc::new(Mutex::new(HashMap::new()));
        let generations = receivers
            .keys()
            .map(|&broker_id| (broker_id, AtomicU64::new(0)))
            .collect();

        Ok(Self {
            listener,
            port,
            receivers,
            active_generations,
            active_sockets,
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
        for (_, socket) in self.active_sockets.lock().drain() {
            let _ = socket.shutdown(std::net::Shutdown::Both);
        }
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
                    log::error!("Shared MT5 listener accept failed: {error}");
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
            log::warn!("Failed to configure route handshake timeout: {error}");
            return;
        }

        let mut hello = [0u8; ROUTE_HELLO_LENGTH];
        if let Err(error) = stream.read_exact(&mut hello) {
            log::warn!("Rejected MT5 connection without a complete route handshake: {error}");
            return;
        }
        if hello[..4] != ROUTE_HELLO_MAGIC[..] {
            log::warn!("Rejected MT5 connection with an invalid route handshake.");
            return;
        }

        let broker_id = le_u32(&hello, 4);
        let Some(receiver) = self.receivers.get(&broker_id).cloned() else {
            log::warn!("Rejected MT5 connection for unknown broker id {broker_id}.");
            return;
        };
        if let Err(error) = stream.set_nonblocking(true) {
            log::warn!("Failed to configure broker {broker_id} connection: {error}");
            return;
        }

        let generation = self
            .generations
            .get(&broker_id)
            .map_or(1, |counter| counter.fetch_add(1, Ordering::AcqRel) + 1);
        stream.set_read_timeout(None).ok();

        // Connection Takeover: if a previous connection is still active for this broker,
        // shut down its socket so its receiver loop unblocks immediately and terminates.
        {
            let mut gens = self.active_generations.lock();
            gens.insert(broker_id, generation);

            let mut sockets = self.active_sockets.lock();
            if let Some(old_socket) = sockets.remove(&broker_id) {
                log::info!(
                    "MT5 broker {broker_id} reconnected: superseding previous connection with generation {generation}"
                );
                let _ = old_socket.shutdown(std::net::Shutdown::Both);
            }
            if let Ok(cloned) = stream.try_clone() {
                sockets.insert(broker_id, cloned);
            }
        }

        log::info!("Accepted MT5 connection for broker {broker_id} (generation: {generation})");

        let active_guard = ActiveConnectionGuard {
            broker_id,
            generation,
            active_generations: self.active_generations.clone(),
            active_sockets: self.active_sockets.clone(),
        };

        let spawn_result = thread::Builder::new()
            .name(format!("mt5-broker-{broker_id}"))
            .spawn(move || {
                let _guard = active_guard;
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
                let mut gens = self.active_generations.lock();
                if gens.get(&broker_id) == Some(&generation) {
                    gens.remove(&broker_id);
                    self.active_sockets.lock().remove(&broker_id);
                }
                log::error!("Failed to start MT5 receiver for broker {broker_id}: {error}");
            }
        }
    }
}
