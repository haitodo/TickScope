//! Latency instrumentation and percentile tracking across pipeline stages.
//!
//! Tracks monotonic latency across:
//! `rx_mono_ns` -> `engine_mono_ns` -> `projection_mono_ns` -> `snapshot_mono_ns` -> `render_mono_ns`
//!
//! Adheres strictly to RFC Beta 0.3 Sections 19, 20, 21, 62, 65, 66:
//! - Zero dynamic allocations on recording (hot path)
//! - Pure monotonic clock timestamps (`MonoNs`)
//! - Online percentile statistics (p50, p95, p99, max)
//! - Queue depth and dropped snapshot tracking

use crate::contracts::MonoNs;
use serde::{Deserialize, Serialize};
use std::fmt;

/// Capacity of the circular buffer used for rolling percentile tracking.
/// 2048 samples provides high statistical resolution while remaining power-of-two
/// for fast bitwise modulo wrapping.
pub const LATENCY_RING_CAPACITY: usize = 2048;

/// Enumeration of pipeline stages for stage-based queries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PipelineStage {
    TickToEngine,
    EngineToProjection,
    ProjectionToSnapshot,
    SnapshotToUi,
    TotalPipeline,
}

/// Monotonic timestamps captured as a tick travels through the entire pipeline.
///
/// Stage progression:
/// 1. `rx_mono_ns`: Arrival timestamp at network/ingestion layer.
/// 2. `engine_mono_ns`: Tick processing time in normalization/matcher engine.
/// 3. `projection_mono_ns`: State projection update timestamp.
/// 4. `snapshot_mono_ns`: UI snapshot creation/exchange timestamp.
/// 5. `render_mono_ns`: UI painter presentation/render timestamp.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct PipelineTimestamps {
    pub rx_mono_ns: MonoNs,
    pub engine_mono_ns: MonoNs,
    pub projection_mono_ns: MonoNs,
    pub snapshot_mono_ns: MonoNs,
    pub render_mono_ns: MonoNs,
}

impl PipelineTimestamps {
    pub const ZERO: Self = Self {
        rx_mono_ns: MonoNs::ZERO,
        engine_mono_ns: MonoNs::ZERO,
        projection_mono_ns: MonoNs::ZERO,
        snapshot_mono_ns: MonoNs::ZERO,
        render_mono_ns: MonoNs::ZERO,
    };

    pub fn new(rx_mono_ns: MonoNs) -> Self {
        Self {
            rx_mono_ns,
            ..Self::ZERO
        }
    }

    pub fn with_engine(mut self, engine_mono_ns: MonoNs) -> Self {
        self.engine_mono_ns = engine_mono_ns;
        self
    }

    pub fn with_projection(mut self, projection_mono_ns: MonoNs) -> Self {
        self.projection_mono_ns = projection_mono_ns;
        self
    }

    pub fn with_snapshot(mut self, snapshot_mono_ns: MonoNs) -> Self {
        self.snapshot_mono_ns = snapshot_mono_ns;
        self
    }

    pub fn with_render(mut self, render_mono_ns: MonoNs) -> Self {
        self.render_mono_ns = render_mono_ns;
        self
    }

    pub fn mark_engine(&mut self, now: MonoNs) {
        self.engine_mono_ns = now;
    }

    pub fn mark_projection(&mut self, now: MonoNs) {
        self.projection_mono_ns = now;
    }

    pub fn mark_snapshot(&mut self, now: MonoNs) {
        self.snapshot_mono_ns = now;
    }

    pub fn mark_render(&mut self, now: MonoNs) {
        self.render_mono_ns = now;
    }

    pub fn tick_to_engine_ns(&self) -> Option<u64> {
        if self.rx_mono_ns.0 > 0 && self.engine_mono_ns.0 >= self.rx_mono_ns.0 {
            Some(self.engine_mono_ns.0 - self.rx_mono_ns.0)
        } else {
            None
        }
    }

    pub fn engine_to_projection_ns(&self) -> Option<u64> {
        if self.engine_mono_ns.0 > 0 && self.projection_mono_ns.0 >= self.engine_mono_ns.0 {
            Some(self.projection_mono_ns.0 - self.engine_mono_ns.0)
        } else {
            None
        }
    }

    pub fn projection_to_snapshot_ns(&self) -> Option<u64> {
        if self.projection_mono_ns.0 > 0 && self.snapshot_mono_ns.0 >= self.projection_mono_ns.0 {
            Some(self.snapshot_mono_ns.0 - self.projection_mono_ns.0)
        } else {
            None
        }
    }

    pub fn snapshot_to_ui_ns(&self) -> Option<u64> {
        if self.snapshot_mono_ns.0 > 0 && self.render_mono_ns.0 >= self.snapshot_mono_ns.0 {
            Some(self.render_mono_ns.0 - self.snapshot_mono_ns.0)
        } else {
            None
        }
    }

    pub fn total_latency_ns(&self) -> Option<u64> {
        if self.rx_mono_ns.0 > 0 && self.render_mono_ns.0 >= self.rx_mono_ns.0 {
            Some(self.render_mono_ns.0 - self.rx_mono_ns.0)
        } else {
            None
        }
    }
}

/// Summary percentile statistics for a stage or overall pipeline (in microseconds).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
pub struct PercentileStats {
    pub sample_count: u64,
    pub p50_us: f64,
    pub p95_us: f64,
    pub p99_us: f64,
    pub max_us: f64,
}

impl PercentileStats {
    pub const fn empty() -> Self {
        Self {
            sample_count: 0,
            p50_us: 0.0,
            p95_us: 0.0,
            p99_us: 0.0,
            max_us: 0.0,
        }
    }
}

impl fmt::Display for PercentileStats {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "n={}, p50={:.2}µs, p95={:.2}µs, p99={:.2}µs, max={:.2}µs",
            self.sample_count, self.p50_us, self.p95_us, self.p99_us, self.max_us
        )
    }
}

/// Latency statistics across all four stages plus end-to-end total pipeline,
/// along with queue depth and dropped snapshot count.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
pub struct StageLatencySummary {
    pub tick_to_engine: PercentileStats,
    pub engine_to_projection: PercentileStats,
    pub projection_to_snapshot: PercentileStats,
    pub snapshot_to_ui: PercentileStats,
    pub total_pipeline: PercentileStats,
    pub queue_depth: usize,
    pub dropped_snapshots: u64,
}

impl StageLatencySummary {
    pub fn queue_depth(&self) -> usize {
        self.queue_depth
    }

    pub fn dropped_snapshots(&self) -> u64 {
        self.dropped_snapshots
    }
}

impl fmt::Display for StageLatencySummary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "Stage Latency Summary:")?;
        writeln!(f, "  Tick -> Engine:          {}", self.tick_to_engine)?;
        writeln!(f, "  Engine -> Projection:    {}", self.engine_to_projection)?;
        writeln!(f, "  Projection -> Snapshot:  {}", self.projection_to_snapshot)?;
        writeln!(f, "  Snapshot -> UI:          {}", self.snapshot_to_ui)?;
        writeln!(f, "  Total Pipeline:          {}", self.total_pipeline)?;
        writeln!(f, "  Queue Depth:             {}", self.queue_depth)?;
        write!(f,   "  Dropped Snapshots:       {}", self.dropped_snapshots)
    }
}

/// Fixed-capacity ring buffer tracking recent latency samples (in nanoseconds)
/// with zero dynamic allocations on recording.
#[derive(Debug, Clone)]
pub struct LatencyRingBuffer {
    buffer: Box<[u64; LATENCY_RING_CAPACITY]>,
    write_idx: usize,
    valid_count: usize,
    total_samples: u64,
}

impl Default for LatencyRingBuffer {
    fn default() -> Self {
        Self::new()
    }
}

impl LatencyRingBuffer {
    pub fn new() -> Self {
        let buffer: Box<[u64; LATENCY_RING_CAPACITY]> =
            vec![0u64; LATENCY_RING_CAPACITY].into_boxed_slice().try_into().unwrap();
        Self {
            buffer,
            write_idx: 0,
            valid_count: 0,
            total_samples: 0,
        }
    }

    /// Record a single latency measurement in nanoseconds.
    ///
    /// O(1) performance with zero heap allocation and bitwise power-of-two wrapping.
    #[inline]
    pub fn record(&mut self, latency_ns: u64) {
        self.buffer[self.write_idx] = latency_ns;
        self.write_idx = (self.write_idx + 1) & (LATENCY_RING_CAPACITY - 1);
        self.total_samples = self.total_samples.saturating_add(1);
        if self.valid_count < LATENCY_RING_CAPACITY {
            self.valid_count += 1;
        }
    }

    #[inline]
    pub fn total_samples(&self) -> u64 {
        self.total_samples
    }

    #[inline]
    pub fn valid_count(&self) -> usize {
        self.valid_count
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.valid_count == 0
    }

    pub fn clear(&mut self) {
        self.write_idx = 0;
        self.valid_count = 0;
        self.total_samples = 0;
    }

    /// Compute percentile statistics (in microseconds) for the rolling window.
    ///
    /// Uses stack-allocated scratch space to perform fast sorting without dynamic heap allocation.
    pub fn compute_percentiles(&self) -> PercentileStats {
        if self.valid_count == 0 {
            return PercentileStats {
                sample_count: self.total_samples,
                p50_us: 0.0,
                p95_us: 0.0,
                p99_us: 0.0,
                max_us: 0.0,
            };
        }

        let n = self.valid_count;
        let mut scratch = [0u64; LATENCY_RING_CAPACITY];
        scratch[..n].copy_from_slice(&self.buffer[..n]);
        let sorted = &mut scratch[..n];
        sorted.sort_unstable();

        PercentileStats {
            sample_count: self.total_samples,
            p50_us: percentile_at(sorted, 0.50),
            p95_us: percentile_at(sorted, 0.95),
            p99_us: percentile_at(sorted, 0.99),
            max_us: sorted[n - 1] as f64 / 1_000.0,
        }
    }
}

/// Helper function to compute nearest-rank percentile value in microseconds from a sorted slice.
#[inline]
fn percentile_at(sorted: &[u64], pct: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let n = sorted.len();
    if n == 1 {
        return sorted[0] as f64 / 1_000.0;
    }
    let rank = (n as f64 * pct).ceil() as usize;
    let idx = rank.clamp(1, n) - 1;
    sorted[idx] as f64 / 1_000.0
}

/// Metric tracker monitoring stage-by-stage latencies, queue depth, and dropped snapshots.
///
/// Guarantees:
/// - Zero dynamic allocations during `record_*` calls.
/// - Pure monotonic clock differences (`saturating_sub`).
/// - Rolling window percentile analysis for dashboard/telemetry.
#[derive(Debug, Clone)]
pub struct LatencyMetrics {
    tick_to_engine: LatencyRingBuffer,
    engine_to_projection: LatencyRingBuffer,
    projection_to_snapshot: LatencyRingBuffer,
    snapshot_to_ui: LatencyRingBuffer,
    total_pipeline: LatencyRingBuffer,
    queue_depth: usize,
    dropped_snapshots: u64,
}

impl Default for LatencyMetrics {
    fn default() -> Self {
        Self {
            tick_to_engine: LatencyRingBuffer::new(),
            engine_to_projection: LatencyRingBuffer::new(),
            projection_to_snapshot: LatencyRingBuffer::new(),
            snapshot_to_ui: LatencyRingBuffer::new(),
            total_pipeline: LatencyRingBuffer::new(),
            queue_depth: 0,
            dropped_snapshots: 0,
        }
    }
}

impl LatencyMetrics {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record latency from tick arrival to engine processing.
    #[inline]
    pub fn record_tick_to_engine(&mut self, rx_ns: MonoNs, engine_ns: MonoNs) {
        let delta = engine_ns.saturating_sub(rx_ns).0;
        self.tick_to_engine.record(delta);
    }

    /// Record latency from engine processing to projection update.
    #[inline]
    pub fn record_engine_to_projection(&mut self, engine_ns: MonoNs, proj_ns: MonoNs) {
        let delta = proj_ns.saturating_sub(engine_ns).0;
        self.engine_to_projection.record(delta);
    }

    /// Record latency from projection update to snapshot publication.
    #[inline]
    pub fn record_projection_to_snapshot(&mut self, proj_ns: MonoNs, snap_ns: MonoNs) {
        let delta = snap_ns.saturating_sub(proj_ns).0;
        self.projection_to_snapshot.record(delta);
    }

    /// Record latency from snapshot publication to UI rendering presentation.
    #[inline]
    pub fn record_snapshot_to_ui(&mut self, snap_ns: MonoNs, render_ns: MonoNs) {
        let delta = render_ns.saturating_sub(snap_ns).0;
        self.snapshot_to_ui.record(delta);
    }

    /// Record end-to-end total pipeline latency from tick reception to UI presentation.
    #[inline]
    pub fn record_total_pipeline(&mut self, rx_ns: MonoNs, render_ns: MonoNs) {
        let delta = render_ns.saturating_sub(rx_ns).0;
        self.total_pipeline.record(delta);
    }

    /// Record all stages available in a complete `PipelineTimestamps` set.
    pub fn record_pipeline_timestamps(&mut self, ts: &PipelineTimestamps) {
        if ts.rx_mono_ns.0 > 0 && ts.engine_mono_ns.0 >= ts.rx_mono_ns.0 {
            self.record_tick_to_engine(ts.rx_mono_ns, ts.engine_mono_ns);
        }
        if ts.engine_mono_ns.0 > 0 && ts.projection_mono_ns.0 >= ts.engine_mono_ns.0 {
            self.record_engine_to_projection(ts.engine_mono_ns, ts.projection_mono_ns);
        }
        if ts.projection_mono_ns.0 > 0 && ts.snapshot_mono_ns.0 >= ts.projection_mono_ns.0 {
            self.record_projection_to_snapshot(ts.projection_mono_ns, ts.snapshot_mono_ns);
        }
        if ts.snapshot_mono_ns.0 > 0 && ts.render_mono_ns.0 >= ts.snapshot_mono_ns.0 {
            self.record_snapshot_to_ui(ts.snapshot_mono_ns, ts.render_mono_ns);
        }
        if ts.rx_mono_ns.0 > 0 && ts.render_mono_ns.0 >= ts.rx_mono_ns.0 {
            self.record_total_pipeline(ts.rx_mono_ns, ts.render_mono_ns);
        }
    }

    /// Record current ingestion/processing queue depth.
    #[inline]
    pub fn record_queue_depth(&mut self, depth: usize) {
        self.queue_depth = depth;
    }

    /// Increment count of dropped snapshots (due to backpressure or UI frame skipping).
    #[inline]
    pub fn increment_dropped_snapshots(&mut self) {
        self.dropped_snapshots = self.dropped_snapshots.saturating_add(1);
    }

    #[inline]
    pub fn queue_depth(&self) -> usize {
        self.queue_depth
    }

    #[inline]
    pub fn dropped_snapshots(&self) -> u64 {
        self.dropped_snapshots
    }

    pub fn tick_to_engine(&self) -> &LatencyRingBuffer {
        &self.tick_to_engine
    }

    pub fn engine_to_projection(&self) -> &LatencyRingBuffer {
        &self.engine_to_projection
    }

    pub fn projection_to_snapshot(&self) -> &LatencyRingBuffer {
        &self.projection_to_snapshot
    }

    pub fn snapshot_to_ui(&self) -> &LatencyRingBuffer {
        &self.snapshot_to_ui
    }

    pub fn total_pipeline(&self) -> &LatencyRingBuffer {
        &self.total_pipeline
    }

    pub fn clear(&mut self) {
        self.tick_to_engine.clear();
        self.engine_to_projection.clear();
        self.projection_to_snapshot.clear();
        self.snapshot_to_ui.clear();
        self.total_pipeline.clear();
        self.queue_depth = 0;
        self.dropped_snapshots = 0;
    }

    /// Compute latency summary across all pipeline stages.
    pub fn compute_summary(&self) -> StageLatencySummary {
        let tick_to_engine = self.tick_to_engine.compute_percentiles();
        let engine_to_projection = self.engine_to_projection.compute_percentiles();
        let projection_to_snapshot = self.projection_to_snapshot.compute_percentiles();
        let snapshot_to_ui = self.snapshot_to_ui.compute_percentiles();

        let total_pipeline = if self.total_pipeline.total_samples() > 0 {
            self.total_pipeline.compute_percentiles()
        } else if tick_to_engine.sample_count > 0
            && engine_to_projection.sample_count > 0
            && projection_to_snapshot.sample_count > 0
            && snapshot_to_ui.sample_count > 0
        {
            // Synthesize total pipeline statistics by summing stage percentiles
            PercentileStats {
                sample_count: tick_to_engine
                    .sample_count
                    .min(engine_to_projection.sample_count)
                    .min(projection_to_snapshot.sample_count)
                    .min(snapshot_to_ui.sample_count),
                p50_us: tick_to_engine.p50_us
                    + engine_to_projection.p50_us
                    + projection_to_snapshot.p50_us
                    + snapshot_to_ui.p50_us,
                p95_us: tick_to_engine.p95_us
                    + engine_to_projection.p95_us
                    + projection_to_snapshot.p95_us
                    + snapshot_to_ui.p95_us,
                p99_us: tick_to_engine.p99_us
                    + engine_to_projection.p99_us
                    + projection_to_snapshot.p99_us
                    + snapshot_to_ui.p99_us,
                max_us: tick_to_engine.max_us
                    + engine_to_projection.max_us
                    + projection_to_snapshot.max_us
                    + snapshot_to_ui.max_us,
            }
        } else {
            PercentileStats::default()
        };

        StageLatencySummary {
            tick_to_engine,
            engine_to_projection,
            projection_to_snapshot,
            snapshot_to_ui,
            total_pipeline,
            queue_depth: self.queue_depth,
            dropped_snapshots: self.dropped_snapshots,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_state() {
        let metrics = LatencyMetrics::default();
        assert_eq!(metrics.queue_depth(), 0);
        assert_eq!(metrics.dropped_snapshots(), 0);

        let summary = metrics.compute_summary();
        assert_eq!(summary.tick_to_engine.sample_count, 0);
        assert_eq!(summary.tick_to_engine.p50_us, 0.0);
        assert_eq!(summary.engine_to_projection.sample_count, 0);
        assert_eq!(summary.projection_to_snapshot.sample_count, 0);
        assert_eq!(summary.snapshot_to_ui.sample_count, 0);
        assert_eq!(summary.total_pipeline.sample_count, 0);
        assert_eq!(summary.queue_depth, 0);
        assert_eq!(summary.dropped_snapshots, 0);
    }

    #[test]
    fn test_record_single_stage_metrics() {
        let mut metrics = LatencyMetrics::new();
        // 1,500 ns = 1.5 µs
        metrics.record_tick_to_engine(MonoNs(10_000), MonoNs(11_500));

        let summary = metrics.compute_summary();
        assert_eq!(summary.tick_to_engine.sample_count, 1);
        assert!((summary.tick_to_engine.p50_us - 1.5).abs() < f64::EPSILON);
        assert!((summary.tick_to_engine.p95_us - 1.5).abs() < f64::EPSILON);
        assert!((summary.tick_to_engine.p99_us - 1.5).abs() < f64::EPSILON);
        assert!((summary.tick_to_engine.max_us - 1.5).abs() < f64::EPSILON);
    }

    #[test]
    fn test_percentiles_calculation() {
        let mut ring = LatencyRingBuffer::new();
        // Insert 100 values: 1,000 ns, 2,000 ns, ..., 100,000 ns (1.0 µs to 100.0 µs)
        for i in 1..=100 {
            ring.record(i * 1_000);
        }

        let stats = ring.compute_percentiles();
        assert_eq!(stats.sample_count, 100);
        assert!((stats.p50_us - 50.0).abs() < 1e-6, "expected 50.0, got {}", stats.p50_us);
        assert!((stats.p95_us - 95.0).abs() < 1e-6, "expected 95.0, got {}", stats.p95_us);
        assert!((stats.p99_us - 99.0).abs() < 1e-6, "expected 99.0, got {}", stats.p99_us);
        assert!((stats.max_us - 100.0).abs() < 1e-6, "expected 100.0, got {}", stats.max_us);
    }

    #[test]
    fn test_ring_buffer_wraparound_no_alloc() {
        let mut ring = LatencyRingBuffer::new();
        // Capacity is 2048. Record 3,000 samples.
        for i in 1..=3000 {
            ring.record(i * 100);
        }

        assert_eq!(ring.total_samples(), 3000);
        assert_eq!(ring.valid_count(), LATENCY_RING_CAPACITY);

        let stats = ring.compute_percentiles();
        assert_eq!(stats.sample_count, 3000);
        // Max should be the last recorded value: 3,000 * 100 ns = 300,000 ns = 300.0 µs
        assert!((stats.max_us - 300.0).abs() < 1e-6);
        // Oldest sample in the window is (3000 - 2048 + 1) * 100 ns = 953 * 100 = 95,300 ns = 95.3 µs
        // Median in window of 2048 samples: index 1023 in range [953..=3000]
        // value = (953 + 1023) * 100 = 197,600 ns = 197.6 µs
        assert!((stats.p50_us - 197.6).abs() < 0.2);
    }

    #[test]
    fn test_all_pipeline_stages_and_summary() {
        let mut metrics = LatencyMetrics::new();

        metrics.record_tick_to_engine(MonoNs(1_000), MonoNs(2_000));      // 1.0 µs
        metrics.record_engine_to_projection(MonoNs(2_000), MonoNs(4_000)); // 2.0 µs
        metrics.record_projection_to_snapshot(MonoNs(4_000), MonoNs(7_000)); // 3.0 µs
        metrics.record_snapshot_to_ui(MonoNs(7_000), MonoNs(11_000));     // 4.0 µs
        metrics.record_queue_depth(42);
        metrics.increment_dropped_snapshots();
        metrics.increment_dropped_snapshots();

        let summary = metrics.compute_summary();
        assert_eq!(summary.queue_depth, 42);
        assert_eq!(summary.dropped_snapshots, 2);

        assert!((summary.tick_to_engine.p50_us - 1.0).abs() < 1e-6);
        assert!((summary.engine_to_projection.p50_us - 2.0).abs() < 1e-6);
        assert!((summary.projection_to_snapshot.p50_us - 3.0).abs() < 1e-6);
        assert!((summary.snapshot_to_ui.p50_us - 4.0).abs() < 1e-6);

        // Synthesized total pipeline: 1.0 + 2.0 + 3.0 + 4.0 = 10.0 µs
        assert!((summary.total_pipeline.p50_us - 10.0).abs() < 1e-6);
        assert!((summary.total_pipeline.max_us - 10.0).abs() < 1e-6);
        assert_eq!(summary.total_pipeline.sample_count, 1);
    }

    #[test]
    fn test_pipeline_timestamps_struct_and_recording() {
        let mut ts = PipelineTimestamps::new(MonoNs(100_000));
        ts.mark_engine(MonoNs(101_000));
        ts.mark_projection(MonoNs(102_500));
        ts.mark_snapshot(MonoNs(104_000));
        ts.mark_render(MonoNs(106_000));

        assert_eq!(ts.tick_to_engine_ns(), Some(1_000));
        assert_eq!(ts.engine_to_projection_ns(), Some(1_500));
        assert_eq!(ts.projection_to_snapshot_ns(), Some(1_500));
        assert_eq!(ts.snapshot_to_ui_ns(), Some(2_000));
        assert_eq!(ts.total_latency_ns(), Some(6_000));

        let mut metrics = LatencyMetrics::new();
        metrics.record_pipeline_timestamps(&ts);

        let summary = metrics.compute_summary();
        assert!((summary.tick_to_engine.p50_us - 1.0).abs() < 1e-6);
        assert!((summary.engine_to_projection.p50_us - 1.5).abs() < 1e-6);
        assert!((summary.projection_to_snapshot.p50_us - 1.5).abs() < 1e-6);
        assert!((summary.snapshot_to_ui.p50_us - 2.0).abs() < 1e-6);
        // Explicit total pipeline recorded: 6,000 ns = 6.0 µs
        assert!((summary.total_pipeline.p50_us - 6.0).abs() < 1e-6);
        assert_eq!(summary.total_pipeline.sample_count, 1);
    }

    #[test]
    fn test_monotonic_safety_and_underflow() {
        let mut metrics = LatencyMetrics::new();
        // Inverted timestamps (clock jitter or invalid order)
        metrics.record_tick_to_engine(MonoNs(50_000), MonoNs(40_000));

        let summary = metrics.compute_summary();
        // saturating_sub should prevent underflow and record 0.0 µs
        assert_eq!(summary.tick_to_engine.sample_count, 1);
        assert_eq!(summary.tick_to_engine.p50_us, 0.0);
        assert_eq!(summary.tick_to_engine.max_us, 0.0);
    }

    #[test]
    fn test_dropped_snapshots_and_queue_depth() {
        let mut metrics = LatencyMetrics::new();
        assert_eq!(metrics.dropped_snapshots(), 0);
        for _ in 0..5 {
            metrics.increment_dropped_snapshots();
        }
        assert_eq!(metrics.dropped_snapshots(), 5);

        metrics.record_queue_depth(128);
        assert_eq!(metrics.queue_depth(), 128);

        let summary = metrics.compute_summary();
        assert_eq!(summary.dropped_snapshots, 5);
        assert_eq!(summary.queue_depth, 128);
    }

    #[test]
    fn test_serialization() {
        let stats = PercentileStats {
            sample_count: 500,
            p50_us: 1.25,
            p95_us: 3.45,
            p99_us: 7.89,
            max_us: 15.2,
        };
        let serialized = serde_json::to_string(&stats).expect("serialization failed");
        let deserialized: PercentileStats = serde_json::from_str(&serialized).expect("deserialization failed");
        assert_eq!(stats, deserialized);

        let ts = PipelineTimestamps::new(MonoNs(100))
            .with_engine(MonoNs(200))
            .with_projection(MonoNs(300))
            .with_snapshot(MonoNs(400))
            .with_render(MonoNs(500));
        let ts_json = serde_json::to_string(&ts).expect("serialization failed");
        let ts_back: PipelineTimestamps = serde_json::from_str(&ts_json).expect("deserialization failed");
        assert_eq!(ts, ts_back);
    }

    #[test]
    fn test_clear_resets_all_state() {
        let mut metrics = LatencyMetrics::new();
        metrics.record_tick_to_engine(MonoNs(100), MonoNs(200));
        metrics.record_queue_depth(10);
        metrics.increment_dropped_snapshots();

        metrics.clear();
        assert_eq!(metrics.queue_depth(), 0);
        assert_eq!(metrics.dropped_snapshots(), 0);
        let summary = metrics.compute_summary();
        assert_eq!(summary.tick_to_engine.sample_count, 0);
    }
}
