use crate::contracts::config::AppConfig;
use crate::contracts::ports::{AppendResult, ClockPort, LogSinkPort, RawIngressSink, SnapshotExchangePort, SubmitResult};
use crate::contracts::types::*;

use crate::state::snapshot::{SnapshotBuilder, SnapshotExchange};
use crate::storage::logger::AsyncLogger;
use crate::tick::engine::TickEngine;
use crate::transport::router::TransportRouter;
use crate::transport::tcp::TransportReceiver;
use crossbeam_channel::{bounded, Receiver, Sender};
use parking_lot::Mutex;
use std::collections::HashMap;
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
    /// Broker routes with the actual listener ports used by this process.
    /// These are written to each MT5 terminal's connection map at startup.
    pub deployment_brokers: Vec<crate::contracts::config::BrokerConfig>,
    receivers: Vec<Arc<TransportReceiver>>,
    router: Option<Arc<TransportRouter>>,
    engine: Arc<Mutex<TickEngine>>,
    logger: Option<Arc<AsyncLogger>>,
    threads: Vec<JoinHandle<()>>,
}

impl RuntimeCoordinator {
    pub fn new(config: AppConfig) -> Result<Self, String> {
        config.validate()?;

        let run_id = RunId::new_random();
        log::info!("Initializing RuntimeCoordinator (run_id: {:?})", run_id);
        let clock = Arc::new(SystemClock::new(run_id));
        let running = Arc::new(AtomicBool::new(true));

        // Logger setup
        let logger = if config.logger.enabled {
            let l = AsyncLogger::new(
                &config.logger.log_dir,
                run_id,
                config.logger.max_queue_records,
                config.logger.max_queue_bytes,
                config.logger.flush_interval_ms,
            )?;
            log::info!("Binary tick logger enabled (dir: {})", config.logger.log_dir);
            Some(Arc::new(l))
        } else {
            log::info!("Binary tick logger disabled in configuration");
            None
        };

        if let Some(logger) = &logger {
            let config_text = toml::to_string(&config)
                .map_err(|error| format!("Failed to serialize startup configuration for logging: {error}"))?;
            let metadata = Arc::new(LogRecord::Metadata(LogMetadata {
                config_epoch: 1,
                observed_mono_ns: clock.sample().mono_ns,
                toml_text: config_text,
            }));
            match logger.try_append(metadata) {
                AppendResult::Accepted => {}
                AppendResult::Full(_) => {
                    return Err("Logger queue is full while recording startup configuration".to_string());
                }
                AppendResult::Fault(_, reason) => {
                    return Err(format!("Logger rejected startup configuration: {reason}"));
                }
            }
        }

        let log_sink_port: Option<Arc<dyn crate::contracts::ports::LogSinkPort>> = logger
            .as_ref()
            .map(|l| l.clone() as Arc<dyn crate::contracts::ports::LogSinkPort>);

        let engine = Arc::new(Mutex::new(TickEngine::new(config.clone())));
        let exchange = Arc::new(SnapshotExchange::new_empty(run_id));

        // Ingress channel
        let (ingress_tx, ingress_rx) = bounded::<IngressItem>(config.ingress.max_frames_per_broker * config.brokers.len());
        let ingress_sink = Arc::new(ChannelIngressSink { sender: ingress_tx });

        let mut receivers = Vec::new();
        let mut threads = Vec::new();
        let mut deployment_brokers = config.brokers.clone();
        let mut routed_receivers = HashMap::new();

        // Build one broker receiver per configured source. Auto-deploy mode
        // routes all EA connections through one shared listener below.
        for b_cfg in &config.brokers {
            let receiver = Arc::new(TransportReceiver::new_with_limits(
                b_cfg.clone(),
                config.protocol.ack_mode.clone(),
                config.protocol.max_payload_length as usize,
                config.protocol.debug_resync_limit as usize,
                config.ingress.progress_interval_ms,
                log_sink_port.clone(),
                clock.clone(),
                ingress_sink.clone(),
            ));
            receivers.push(receiver.clone());
            routed_receivers.insert(b_cfg.id, receiver.clone());

            if !config.mt5.auto_deploy {
                let rec_clone = receiver.clone();
                let handle = thread::spawn(move || {
                    rec_clone.run();
                });
                threads.push(handle);
            }
        }

        let router = if config.mt5.auto_deploy {
            let router = Arc::new(TransportRouter::bind_loopback(
                routed_receivers,
                config.ingress.progress_interval_ms,
            )?);
            log::info!("MT5 shared router listening on 127.0.0.1:{}", router.local_port());
            for broker in &mut deployment_brokers {
                broker.host = "127.0.0.1".to_string();
                broker.port = router.local_port();
            }
            let router_thread = router.clone();
            threads.push(thread::spawn(move || router_thread.run()));
            Some(router)
        } else {
            None
        };

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
                    eng.make_projection_at(now_utc, clk_sample.mono_ns)
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
            deployment_brokers,
            receivers,
            router,
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
        // Once shutdown begins, receivers are stopped first and every frame
        // already accepted into ingress is processed before this worker exits.
        while running.load(Ordering::SeqCst) || !rx.is_empty() {
            match rx.recv_timeout(Duration::from_millis(1)) {
                Ok(item) => {
                    let mut eng = engine.lock();
                    eng.on_ingress_item(item);
                }
                Err(crossbeam_channel::RecvTimeoutError::Timeout) => {
                    if !running.load(Ordering::SeqCst) && rx.is_empty() {
                        break;
                    }
                }
                Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
            }
        }
    }

    pub fn set_active_pair(&self, pair: (BrokerId, BrokerId)) {
        self.engine.lock().set_active_pair(pair);
    }

    pub fn pair_selection_handler(&self) -> Arc<dyn Fn((BrokerId, BrokerId)) + Send + Sync> {
        let engine = self.engine.clone();
        Arc::new(move |pair| engine.lock().set_active_pair(pair))
    }

    pub fn stop(&mut self) {
        log::info!("Stopping RuntimeCoordinator...");
        if let Some(router) = &self.router {
            router.stop();
        }
        for r in &self.receivers {
            r.stop();
        }
        self.running.store(false, Ordering::SeqCst);
    }

    pub fn wait_for_shutdown(self) {
        for handle in self.threads {
            let _ = handle.join();
        }
        if let Some(logger) = &self.logger {
            logger.finish();
        }
        log::info!("RuntimeCoordinator shutdown complete.");
    }
}
