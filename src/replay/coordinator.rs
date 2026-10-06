//! Replay Coordinator: Orchestrates `VirtualClock`, Parquet Hive sources,
//! `ReplayDriver`, and Snapshot Publisher into a unified runtime.

use super::clock::VirtualClock;
use super::driver::ReplayDriver;
use super::merge_stream::MergeStream;
use super::parquet_source::BrokerParquetSource;
use crate::config::AppConfig;
use crate::core::ports::{ClockPort, SnapshotExchangePort};
use crate::core::types::{BrokerId, RunId, UtcMs};
use crate::state::snapshot::{SnapshotBuilder, SnapshotExchange};
use crate::tick::engine::TickEngine;
use parking_lot::{Condvar, Mutex, RwLock};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

pub struct ReplayCoordinator {
    pub run_id: RunId,
    pub config: AppConfig,
    pub clock: VirtualClock,
    pub exchange: Arc<SnapshotExchange>,
    pub running: Arc<AtomicBool>,
    pub engine: Arc<Mutex<TickEngine>>,
    pub driver: Option<ReplayDriver>,
    pub trade_store: Arc<RwLock<crate::core::models::ReplayTradeStore>>,
    threads: Vec<JoinHandle<()>>,
}

impl ReplayCoordinator {
    /// # Errors
    ///
    /// Returns a message from [`AppConfig::validate`] when the configuration is invalid, and a
    /// per-broker message when that broker has no Parquet source under `tick_dir`.
    ///
    /// # Panics
    ///
    /// The spawned clock thread unwraps `interval.checked_sub(elapsed)`; the loop interval is a fixed
    /// positive constant, so this cannot fire.
    pub fn new(
        config: AppConfig,
        tick_dir: &Path,
        ws_url: &str,
    ) -> Result<Self, String> {
        config.validate()?;

        let run_id = RunId::new_random();
        log::info!(
            "[ReplayCoordinator] Initializing ReplayCoordinator (run_id: {:?}, tick_dir: {})",
            run_id,
            tick_dir.display()
        );

        // Discover and build Parquet sources for all configured brokers
        let mut sources = Vec::new();
        for b in &config.brokers {
            match BrokerParquetSource::new(b.id, &b.name, &b.symbol, tick_dir) {
                Ok(mut src) => {
                    src = src.with_receive_delay_profile();
                    if let Some(delay) = b.receive_delay_ms {
                        src.receive_delay_ms = delay;
                    }
                    sources.push(src);
                }
                Err(e) => {
                    log::warn!(
                        "[ReplayCoordinator] Could not initialize source for broker {} ({}): {}",
                        b.id,
                        b.name,
                        e
                    );
                }
            }
        }

        if sources.is_empty() {
            log::warn!(
                "[ReplayCoordinator] No parquet partitions found in '{}'. Replay will wait for sync.",
                tick_dir.display()
            );
        }

        let merge_stream = Arc::new(RwLock::new(MergeStream::new(sources)));
        let clock = VirtualClock::new(run_id, 0);
        let running = Arc::new(AtomicBool::new(true));

        let tick_engine = TickEngine::new(config.clone());
        let engine = Arc::new(Mutex::new(tick_engine));
        let exchange = Arc::new(SnapshotExchange::new_empty(run_id));

        let tick_wake = Arc::new((Mutex::new(false), Condvar::new()));
        let trade_store = Arc::new(RwLock::new(crate::core::models::ReplayTradeStore::default()));

        // Start ReplayDriver
        let mut driver = ReplayDriver::new(
            run_id,
            clock.clone(),
            engine.clone(),
            merge_stream,
            tick_wake.clone(),
            trade_store.clone(),
        );
        driver.start(ws_url.to_string());

        // Spawn Snapshot Publisher Thread
        let eng_pub = engine.clone();
        let ex_pub = exchange.clone();
        let run_pub = running.clone();
        let clk_pub = clock.clone();
        let tick_wake_pub = tick_wake.clone();
        let repaint_hz = config.display.repaint_hz.max(1);
        let timeframe_ms = config.display.timeframe_ms;

        let pub_handle = thread::spawn(move || {
            let builder = SnapshotBuilder::new(run_id);
            let interval = Duration::from_micros(1_000_000 / u64::from(repaint_hz));
            let (lock, cvar) = &*tick_wake_pub;

            while run_pub.load(Ordering::SeqCst) {
                let frame_start = Instant::now();
                let clk_sample = clk_pub.sample();
                let now_utc = UtcMs(clk_sample.unix_ns.unwrap_or(0) / 1_000_000);

                let proj = {
                    let eng = eng_pub.lock();
                    eng.make_projection_at(now_utc, clk_sample.mono_ns)
                };

                let snap = builder.build_owned(proj, now_utc, clk_sample.mono_ns, timeframe_ms);
                ex_pub.publish(snap);

                let elapsed = frame_start.elapsed();
                if elapsed < interval {
                    thread::sleep(interval.checked_sub(elapsed).unwrap());
                }

                let mut pending = lock.lock();
                if !*pending && run_pub.load(Ordering::SeqCst) {
                    cvar.wait_for(&mut pending, Duration::from_millis(100));
                }
                *pending = false;
            }
        });

        Ok(Self {
            run_id,
            config,
            clock,
            exchange,
            running,
            engine,
            driver: Some(driver),
            trade_store,
            threads: vec![pub_handle],
        })
    }

    pub fn clock_port(&self) -> Arc<dyn ClockPort> {
        Arc::new(self.clock.clone())
    }

    pub fn set_active_pair(&self, pair: (BrokerId, BrokerId)) {
        self.engine.lock().set_active_pair(pair);
    }

    pub fn pair_selection_handler(&self) -> Arc<dyn Fn((BrokerId, BrokerId)) + Send + Sync> {
        let engine = self.engine.clone();
        Arc::new(move |pair| engine.lock().set_active_pair(pair))
    }

    pub fn stop(&mut self) {
        log::info!("[ReplayCoordinator] Stopping ReplayCoordinator...");
        self.running.store(false, Ordering::SeqCst);
        if let Some(mut d) = self.driver.take() {
            d.stop();
            d.wait_for_shutdown();
        }
    }

    pub fn wait_for_shutdown(mut self) {
        for h in self.threads.drain(..) {
            let _ = h.join();
        }
        log::info!("[ReplayCoordinator] Shutdown complete.");
    }
}
