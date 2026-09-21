//! UiSnapshot builder and atomic ArcSwap exchange.
//! Reference: docs/blueprint/semantics-snapshot.md

use crate::contracts::models::*;
use crate::contracts::ports::SnapshotExchangePort;
use crate::contracts::types::*;
use arc_swap::ArcSwap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

pub struct SnapshotBuilder {
    run_id: RunId,
    snapshot_revision: AtomicU64,
}

impl SnapshotBuilder {
    pub fn new(run_id: RunId) -> Self {
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
        let snap_rev = self.snapshot_revision.fetch_add(1, Ordering::SeqCst) + 1;
        let active_candles = projection.candle_views.get(&timeframe_ms).cloned();

        Arc::new(UiSnapshot {
            schema_revision: SCHEMA_REVISION,
            snapshot_revision: snap_rev,
            projection_revision: projection.revision,
            run_id: self.run_id,
            built_mono_ns: now_mono,
            processed_watermark_ns: projection.watermark_ns,
            display_now_utc,
            active_pair: projection.active_pair,
            broker_overviews: projection.broker_overviews.clone(),
            active_pair_comparison: projection.active_pair_comparison.clone(),
            active_candles,
            diagnostics: projection.global_diagnostics.clone(),
        })
    }
}

pub struct SnapshotExchange {
    current: ArcSwap<UiSnapshot>,
}

impl SnapshotExchange {
    pub fn new(initial: Arc<UiSnapshot>) -> Self {
        Self {
            current: ArcSwap::from(initial),
        }
    }

    pub fn new_empty(run_id: RunId) -> Self {
        let initial = Arc::new(UiSnapshot {
            schema_revision: SCHEMA_REVISION,
            snapshot_revision: 0,
            projection_revision: 0,
            run_id,
            built_mono_ns: MonoNs::ZERO,
            processed_watermark_ns: MonoNs::ZERO,
            display_now_utc: UtcMs::ZERO,
            active_pair: (1, 2),
            broker_overviews: Vec::new(),
            active_pair_comparison: None,
            active_candles: None,
            diagnostics: Vec::new(),
        });
        Self::new(initial)
    }
}

impl SnapshotExchangePort for SnapshotExchange {
    fn publish(&self, snapshot: Arc<UiSnapshot>) {
        self.current.store(snapshot);
    }

    fn load_latest(&self) -> Arc<UiSnapshot> {
        self.current.load_full()
    }
}
