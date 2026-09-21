use crate::contracts::config::AppConfig;
use crate::contracts::ports::{ClockPort, RawIngressSink, SnapshotExchangePort, SubmitResult};
use crate::contracts::types::*;

use crate::state::snapshot::{SnapshotBuilder, SnapshotExchange};
use crate::storage::logger::AsyncLogger;
use crate::tick::engine::TickEngine;
use crate::transport::tcp::TransportReceiver;
use crossbeam_channel::{bounded, Receiver, Sender};
use parking_lot::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub struct SystemClock {
    run_id: RunId,
    start_instant: Instant,
}

impl SystemClock {
    pub fn new(run_id: RunId) -> Self {
        Self {
            run_id,
            start_instant: Instant::now(),
        }
    }
}

impl ClockPort for SystemClock {
    fn sample(&self) -> ClockReading {
        let elapsed = self.start_instant.elapsed();
        let mono_ns = elapsed.as_nanos() as u64;

        let unix_ns = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .ok()
            .map(|d| d.as_nanos() as i64);

        ClockReading {
            run_id: self.run_id,
            mono_ns: MonoNs(mono_ns),
            unix_ns,
        }
    }
}

struct ChannelIngressSink {
    sender: Sender<IngressItem>,
}

impl RawIngressSink for ChannelIngressSink {
    fn try_submit(&self, item: IngressItem) -> SubmitResult<IngressItem> {
        match self.sender.try_send(item) {
            Ok(_) => SubmitResult::Accepted,
            Err(crossbeam_channel::TrySendError::Full(it)) => SubmitResult::Full(it),
            Err(crossbeam_channel::TrySendError::Disconnected(it)) => SubmitResult::Closed(it),
        }
    }
}

pub struct RuntimeCoordinator {
    pub run_id: RunId,
    pub config: AppConfig,
    pub clock: Arc<dyn ClockPort>,
    pub exchange: Arc<SnapshotExchange>,
    pub running: Arc<AtomicBool>,
    receivers: Vec<Arc<TransportReceiver>>,
    engine: Arc<Mutex<TickEngine>>,
    logger: Option<Arc<AsyncLogger>>,
    threads: Vec<JoinHandle<()>>,
}

impl RuntimeCoordinator {
    pub fn new(config: AppConfig) -> Result<Self, String> {
        config.validate()?;

        let run_id = RunId::new_random();
        let clock = Arc::new(SystemClock::new(run_id));
        let running = Arc::new(AtomicBool::new(true));

        // Logger setup
        let logger = if config.logger.enabled {
            let l = AsyncLogger::new(
                &config.logger.log_dir,
                run_id,
                config.logger.max_queue_records,
                config.logger.flush_interval_ms,
            )?;
            Some(Arc::new(l))
        } else {
            None
        };

        let log_sink_port: Option<Arc<dyn crate::contracts::ports::LogSinkPort>> = logger
            .as_ref()
            .map(|l| l.clone() as Arc<dyn crate::contracts::ports::LogSinkPort>);

        let engine = Arc::new(Mutex::new(TickEngine::new(config.clone(), log_sink_port)));
        let exchange = Arc::new(SnapshotExchange::new_empty(run_id));

        // Ingress channel
        let (ingress_tx, ingress_rx) = bounded::<IngressItem>(config.ingress.max_frames_per_broker * config.brokers.len());
        let ingress_sink = Arc::new(ChannelIngressSink { sender: ingress_tx });

        let mut receivers = Vec::new();
        let mut threads = Vec::new();

        // 1. Spawn Transport Receivers for each broker
        for b_cfg in &config.brokers {
            let receiver = Arc::new(TransportReceiver::new(
                b_cfg.clone(),
                config.protocol.ack_mode.clone(),
                clock.clone(),
                ingress_sink.clone(),
            ));
            receivers.push(receiver.clone());

            let rec_clone = receiver.clone();
            let handle = thread::spawn(move || {
                rec_clone.run();
            });
            threads.push(handle);
        }

        // 2. Spawn Engine Worker
        let eng_clone = engine.clone();
        let run_clone = running.clone();
        let engine_handle = thread::spawn(move || {
            Self::engine_worker_loop(ingress_rx, eng_clone, run_clone);
        });
        threads.push(engine_handle);

        // 3. Spawn Snapshot Publisher Worker
        let eng_pub = engine.clone();
        let ex_pub = exchange.clone();
        let run_pub = running.clone();
        let clk_pub = clock.clone();
        let repaint_hz = config.display.repaint_hz.max(1);
        let timeframe_ms = config.display.timeframe_ms;

        let pub_handle = thread::spawn(move || {
            let builder = SnapshotBuilder::new(run_id);
            let interval = Duration::from_micros(1_000_000 / repaint_hz as u64);

            while run_pub.load(Ordering::SeqCst) {
                let clk_sample = clk_pub.sample();
                let now_utc = UtcMs(clk_sample.unix_ns.unwrap_or(0) / 1_000_000);

                let proj = {
                    let eng = eng_pub.lock();
                    eng.make_projection(now_utc)
                };

                let snap = builder.build(&proj, now_utc, clk_sample.mono_ns, timeframe_ms);
                ex_pub.publish(snap);

                thread::sleep(interval);
            }
        });
        threads.push(pub_handle);

        Ok(Self {
            run_id,
            config,
            clock,
            exchange,
            running,
            receivers,
            engine,
            logger,
            threads,
        })
    }

    fn engine_worker_loop(
        rx: Receiver<IngressItem>,
        engine: Arc<Mutex<TickEngine>>,
        running: Arc<AtomicBool>,
    ) {
        while running.load(Ordering::SeqCst) {
            match rx.recv_timeout(Duration::from_millis(1)) {
                Ok(item) => {
                    let mut eng = engine.lock();
                    eng.on_ingress_item(item);
                }
                Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
                Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
            }
        }
    }

    pub fn set_active_pair(&self, pair: (BrokerId, BrokerId)) {
        self.engine.lock().set_active_pair(pair);
    }

    pub fn stop(&mut self) {
        self.running.store(false, Ordering::SeqCst);
        for r in &self.receivers {
            r.stop();
        }
        if let Some(l) = &self.logger {
            l.stop();
        }
    }

    pub fn wait_for_shutdown(self) {
        for handle in self.threads {
            let _ = handle.join();
        }
    }
}
