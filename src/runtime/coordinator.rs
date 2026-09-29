use crate::config::{AppConfig, BrokerConfig};
use crate::core::ports::{AppendResult, ClockPort, LogSinkPort, RawIngressSink, SnapshotExchangePort, SubmitResult};
use crate::core::types::*;
use crate::metrics::diagnostics::{DiagnosticStage, DiagnosticsHandle, DiagnosticsRuntime};

use crate::state::snapshot::{SnapshotBuilder, SnapshotExchange};
use crate::storage::logger::AsyncLogger;
use crate::tick::engine::TickEngine;
use crate::transport::router::TransportRouter;
use crate::transport::tcp::TransportReceiver;
use crossbeam_channel::{bounded, Receiver, Sender};
use parking_lot::{Condvar, Mutex};
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

    fn submit_timeout(
        &self,
        item: IngressItem,
        timeout: std::time::Duration,
    ) -> SubmitResult<IngressItem> {
        match self.sender.send_timeout(item, timeout) {
            Ok(_) => SubmitResult::Accepted,
            Err(crossbeam_channel::SendTimeoutError::Timeout(it)) => SubmitResult::Full(it),
            Err(crossbeam_channel::SendTimeoutError::Disconnected(it)) => SubmitResult::Closed(it),
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
    pub deployment_brokers: Vec<BrokerConfig>,
    receivers: Vec<Arc<TransportReceiver>>,
    router: Option<Arc<TransportRouter>>,
    engine: Arc<Mutex<TickEngine>>,
    logger: Option<Arc<AsyncLogger>>,
    diagnostics: Option<DiagnosticsRuntime>,
    threads: Vec<JoinHandle<()>>,
    tick_wake: Arc<(Mutex<bool>, Condvar)>,
}

impl RuntimeCoordinator {
    pub fn new(config: AppConfig) -> Result<Self, String> {
        Self::new_with_diagnostics(config, false)
    }

    pub fn new_with_diagnostics(
        config: AppConfig,
        diagnostics_enabled: bool,
    ) -> Result<Self, String> {
        config.validate()?;

        let run_id = RunId::new_random();
        log::info!("Initializing RuntimeCoordinator (run_id: {:?})", run_id);
        let clock = Arc::new(SystemClock::new(run_id));
        let running = Arc::new(AtomicBool::new(true));
        let diagnostics = if diagnostics_enabled {
            Some(DiagnosticsRuntime::start(
                "data/diagnostics",
                run_id,
                config.logger.enabled,
                &config.protocol.ack_mode,
            )?)
        } else {
            None
        };
        let diagnostics_handle = diagnostics.as_ref().map(DiagnosticsRuntime::handle);
        if let Some(diagnostics) = &diagnostics {
            log::info!(
                "Performance diagnostics enabled (file: {})",
                diagnostics.output_path().display()
            );
        }

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

        let log_sink_port: Option<Arc<dyn LogSinkPort>> = logger
            .as_ref()
            .map(|l| l.clone() as Arc<dyn LogSinkPort>);

        let mut tick_engine = TickEngine::new(config.clone());
        if let Some(diagnostics) = &diagnostics_handle {
            tick_engine = tick_engine.with_diagnostics(diagnostics.clone());
        }
        let engine = Arc::new(Mutex::new(tick_engine));
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
            let receiver = TransportReceiver::new_with_limits(
                b_cfg.clone(),
                config.protocol.ack_mode.clone(),
                config.protocol.max_payload_length as usize,
                config.protocol.debug_resync_limit as usize,
                config.ingress.progress_interval_ms,
                log_sink_port.clone(),
                clock.clone(),
                ingress_sink.clone(),
            );
            let receiver = if let Some(diagnostics) = &diagnostics_handle {
                receiver.with_diagnostics(diagnostics.clone())
            } else {
                receiver
            };
            let receiver = Arc::new(receiver);
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
        let tick_wake = Arc::new((Mutex::new(false), Condvar::new()));
        let tick_wake_worker = tick_wake.clone();
        let tick_wake_pub = tick_wake.clone();

        let eng_clone = engine.clone();
        let run_clone = running.clone();
        let clk_engine = clock.clone();
        let diagnostics_enabled = diagnostics_handle.is_some();
        let engine_handle = thread::spawn(move || {
            Self::engine_worker_loop(
                ingress_rx,
                eng_clone,
                run_clone,
                clk_engine,
                diagnostics_enabled,
                tick_wake_worker,
            );
        });
        threads.push(engine_handle);

        // 3. Spawn Snapshot Publisher Worker
        let eng_pub = engine.clone();
        let ex_pub = exchange.clone();
        let run_pub = running.clone();
        let clk_pub = clock.clone();
        let diagnostics_pub = diagnostics_handle.clone();
        let repaint_hz = config.display.repaint_hz.max(1);
        let timeframe_ms = config.display.timeframe_ms;

        let pub_handle = thread::spawn(move || {
            let builder = SnapshotBuilder::new(run_id);
            let interval = Duration::from_micros(1_000_000 / repaint_hz as u64);
            let (lock, cvar) = &*tick_wake_pub;

            while run_pub.load(Ordering::SeqCst) {
                let frame_start = Instant::now();
                let clk_sample = clk_pub.sample();
                let now_utc = UtcMs(clk_sample.unix_ns.unwrap_or(0) / 1_000_000);

                let projection_start = diagnostics_pub.as_ref().map(|_| Instant::now());
                let proj = {
                    let eng = eng_pub.lock();
                    eng.make_projection_at(now_utc, clk_sample.mono_ns)
                };
                if let (Some(diagnostics), Some(start)) =
                    (&diagnostics_pub, projection_start)
                {
                    diagnostics.record_duration(
                        DiagnosticStage::ProjectionBuild,
                        start.elapsed(),
                    );
                }

                let snapshot_start = diagnostics_pub.as_ref().map(|_| Instant::now());
                let snap = builder.build_owned(proj, now_utc, clk_sample.mono_ns, timeframe_ms);
                if let (Some(diagnostics), Some(start)) = (&diagnostics_pub, snapshot_start) {
                    diagnostics.record_duration(
                        DiagnosticStage::SnapshotBuild,
                        start.elapsed(),
                    );
                }
                ex_pub.publish(snap);

                // Rate-limit to repaint_hz: sleep for remaining frame budget if needed
                let elapsed = frame_start.elapsed();
                if elapsed < interval {
                    thread::sleep(interval - elapsed);
                }

                // If no new tick arrived during the frame budget, wait for next tick or 200ms idle timeout.
                // Clear pending under the same lock so the next iteration starts fresh without double-locking.
                let mut pending = lock.lock();
                if !*pending && run_pub.load(Ordering::SeqCst) {
                    cvar.wait_for(&mut pending, Duration::from_millis(200));
                }
                *pending = false;
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
            diagnostics,
            threads,
            tick_wake,
        })
    }

    fn engine_worker_loop(
        rx: Receiver<IngressItem>,
        engine: Arc<Mutex<TickEngine>>,
        running: Arc<AtomicBool>,
        clock: Arc<dyn ClockPort>,
        diagnostics_enabled: bool,
        tick_wake: Arc<(Mutex<bool>, Condvar)>,
    ) {
        // Once shutdown begins, receivers are stopped first and every frame
        // already accepted into ingress is processed before this worker exits.
        while running.load(Ordering::SeqCst) || !rx.is_empty() {
            match rx.recv_timeout(Duration::from_millis(20)) {
                Ok(item) => {
                    let mut eng = engine.lock();
                    if diagnostics_enabled {
                        eng.on_ingress_item_at(item, clock.sample().mono_ns);
                    } else {
                        eng.on_ingress_item(item);
                    }
                    // Batch drain up to 64 additional immediately available items under the same lock
                    for _ in 0..64 {
                        match rx.try_recv() {
                            Ok(next_item) => {
                                if diagnostics_enabled {
                                    eng.on_ingress_item_at(next_item, clock.sample().mono_ns);
                                } else {
                                    eng.on_ingress_item(next_item);
                                }
                            }
                            Err(_) => break,
                        }
                    }
                    drop(eng);

                    // Signal publisher immediately
                    {
                        let (lock, cvar) = &*tick_wake;
                        let mut pending = lock.lock();
                        if !*pending {
                            *pending = true;
                            cvar.notify_one();
                        }
                    }
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

    pub fn diagnostics_handle(&self) -> Option<DiagnosticsHandle> {
        self.diagnostics
            .as_ref()
            .map(DiagnosticsRuntime::handle)
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
        {
            let (lock, cvar) = &*self.tick_wake;
            let mut pending = lock.lock();
            *pending = true;
            cvar.notify_all();
        }
    }

    pub fn wait_for_shutdown(self) {
        for handle in self.threads {
            let _ = handle.join();
        }
        if let Some(logger) = &self.logger {
            logger.finish();
        }
        if let Some(diagnostics) = self.diagnostics {
            diagnostics.finish();
        }
        log::info!("RuntimeCoordinator shutdown complete.");
    }
}
