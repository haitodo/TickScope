//! `UiSnapshot` builder and atomic `ArcSwap` exchange.

use crate::core::models::{EngineProjection, UiSnapshot, SCHEMA_REVISION};
use crate::core::ports::SnapshotExchangePort;
use crate::core::types::{RunId, UtcMs, MonoNs};
use arc_swap::ArcSwap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

pub struct SnapshotBuilder {
    run_id: RunId,
    snapshot_revision: AtomicU64,
}

impl SnapshotBuilder {
    pub const fn new(run_id: RunId) -> Self {
        Self {
            run_id,
            snapshot_revision: AtomicU64::new(0),
        }
    }

    pub fn build(
        &self,
        projection: &EngineProjection,
        display_now_utc: UtcMs,
        now_mono: MonoNs,
        timeframe_ms: i64,
    ) -> Arc<UiSnapshot> {
        self.build_owned(projection.clone(), display_now_utc, now_mono, timeframe_ms)
    }

    /// Move an owned projection directly into the snapshot without cloning its collections.
    pub fn build_owned(
        &self,
        projection: EngineProjection,
        display_now_utc: UtcMs,
        now_mono: MonoNs,
        _timeframe_ms: i64,
    ) -> Arc<UiSnapshot> {
        let snap_rev = self.snapshot_revision.fetch_add(1, Ordering::SeqCst) + 1;

        Arc::new(UiSnapshot {
            schema_revision: SCHEMA_REVISION,
            snapshot_revision: snap_rev,
            projection_revision: projection.revision,
            run_id: self.run_id,
            built_mono_ns: now_mono,
            processed_watermark_ns: projection.watermark_ns,
            display_now_utc,
            active_pair: projection.active_pair,
            broker_overviews: projection.broker_overviews,
            active_pair_comparison: projection.active_pair_comparison,
            active_candles: None,
            candle_views: projection.candle_views,
            mid_candle_views: projection.mid_candle_views,
            diagnostics: projection.global_diagnostics,
            consensus: projection.consensus,
            active_clusters: projection.active_clusters,
            current_breadth: projection.current_breadth,
            fingerprints: projection.fingerprints,
            hypotheses: projection.hypotheses,
            latency_summary: projection.latency_summary,
            realtime_quote_points: projection.realtime_quote_points,
        })
    }
}

pub struct SnapshotExchange {
    current: ArcSwap<UiSnapshot>,
    repaint_signal: parking_lot::RwLock<Option<Arc<dyn Fn() + Send + Sync>>>,
}

impl SnapshotExchange {
    pub fn new(initial: Arc<UiSnapshot>) -> Self {
        Self {
            current: ArcSwap::from(initial),
            repaint_signal: parking_lot::RwLock::new(None),
        }
    }

    pub fn new_empty(run_id: RunId) -> Self {
        let initial = UiSnapshot {
            run_id,
            ..Default::default()
        };
        Self::new(Arc::new(initial))
    }
}

impl SnapshotExchangePort for SnapshotExchange {
    fn publish(&self, snapshot: Arc<UiSnapshot>) {
        self.current.store(snapshot);
        let guard = self.repaint_signal.read();
        if let Some(signal) = guard.as_ref() {
            signal();
        }
    }

    fn load_latest(&self) -> Arc<UiSnapshot> {
        self.current.load_full()
    }

    fn register_repaint_signal(&self, signal: Arc<dyn Fn() + Send + Sync>) {
        *self.repaint_signal.write() = Some(signal);
    }
}
