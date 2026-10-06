//! Broker Behaviour Fingerprint.
//!
//! Invariant I12: NEVER produce an aggregate single score or ranking across these dimensions.
//! Each behavioral metric remains an independent observation dimension.

use crate::core::types::{BrokerId, MonoNs};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

/// Statistical context attached to all observational metrics.
/// Prevents conflating low-sample or short-window observations with high-confidence baselines.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SampleContext {
    pub sample_count: u64,
    pub window_ms: u64,
    pub fresh_rate: f64,
}

impl SampleContext {
    #[must_use]
    pub const fn new(sample_count: u64, window_ms: u64, fresh_rate: f64) -> Self {
        Self {
            sample_count,
            window_ms,
            fresh_rate,
        }
    }

    #[must_use]
    pub const fn empty() -> Self {
        Self {
            sample_count: 0,
            window_ms: 0,
            fresh_rate: 0.0,
        }
    }
}

impl Default for SampleContext {
    fn default() -> Self {
        Self::empty()
    }
}

impl std::fmt::Display for SampleContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "n={}, window={}ms, fresh={:.1}%",
            self.sample_count,
            self.window_ms,
            self.fresh_rate * 100.0
        )
    }
}

/// Broker Behaviour Fingerprint.
/// Holds mid/long-term statistical behavioral characteristics of a broker feed.
///
/// Invariant I12: NEVER produce an aggregate single score or ranking across these dimensions.
/// Each metric is an independent observation dimension.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BrokerFingerprint {
    pub broker_id: BrokerId,
    pub observed_lead_freq: f64,
    pub observed_follow_freq: f64,
    pub median_delay_ms: f64,
    pub spread_expansion_freq: f64,
    pub stale_freq: f64,
    pub outlier_freq: f64,
    pub consensus_deviation_pips: f64,
    pub sample_context: SampleContext,
}

impl BrokerFingerprint {
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub const fn new(
        broker_id: BrokerId,
        observed_lead_freq: f64,
        observed_follow_freq: f64,
        median_delay_ms: f64,
        spread_expansion_freq: f64,
        stale_freq: f64,
        outlier_freq: f64,
        consensus_deviation_pips: f64,
        sample_context: SampleContext,
    ) -> Self {
        Self {
            broker_id,
            observed_lead_freq,
            observed_follow_freq,
            median_delay_ms,
            spread_expansion_freq,
            stale_freq,
            outlier_freq,
            consensus_deviation_pips,
            sample_context,
        }
    }

    #[must_use]
    pub const fn empty(broker_id: BrokerId) -> Self {
        Self {
            broker_id,
            observed_lead_freq: 0.0,
            observed_follow_freq: 0.0,
            median_delay_ms: 0.0,
            spread_expansion_freq: 0.0,
            stale_freq: 0.0,
            outlier_freq: 0.0,
            consensus_deviation_pips: 0.0,
            sample_context: SampleContext::empty(),
        }
    }

    pub const fn update_sample_context(&mut self, context: SampleContext) {
        self.sample_context = context;
    }
}

/// Accumulator and compiler for `BrokerFingerprint`.
#[derive(Debug, Clone)]
pub struct BrokerFingerprintTracker {
    broker_id: BrokerId,
    lead_events: u64,
    follow_events: u64,
    total_move_events: u64,
    delays_ms: VecDeque<f64>,
    spread_expansion_events: u64,
    total_spread_events: u64,
    stale_ticks: u64,
    outlier_ticks: u64,
    total_ticks: u64,
    fresh_ticks: u64,
    consensus_deviations_sum: f64,
    consensus_deviations_count: u64,
    first_seen_ns: Option<MonoNs>,
    last_seen_ns: Option<MonoNs>,
    max_history: usize,
}

impl BrokerFingerprintTracker {
    pub const DEFAULT_MAX_HISTORY: usize = 2048;

    #[must_use]
    pub fn new(broker_id: BrokerId) -> Self {
        Self {
            broker_id,
            lead_events: 0,
            follow_events: 0,
            total_move_events: 0,
            delays_ms: VecDeque::with_capacity(512),
            spread_expansion_events: 0,
            total_spread_events: 0,
            stale_ticks: 0,
            outlier_ticks: 0,
            total_ticks: 0,
            fresh_ticks: 0,
            consensus_deviations_sum: 0.0,
            consensus_deviations_count: 0,
            first_seen_ns: None,
            last_seen_ns: None,
            max_history: Self::DEFAULT_MAX_HISTORY,
        }
    }

    #[must_use]
    pub const fn broker_id(&self) -> BrokerId {
        self.broker_id
    }

    pub const fn record_lead(&mut self) {
        self.lead_events += 1;
        self.total_move_events += 1;
    }

    pub fn record_follow(&mut self, delay_ms: f64) {
        self.follow_events += 1;
        self.total_move_events += 1;
        if delay_ms >= 0.0 && !delay_ms.is_nan() {
            if self.delays_ms.len() >= self.max_history {
                self.delays_ms.pop_front();
            }
            self.delays_ms.push_back(delay_ms);
        }
    }

    pub const fn record_spread_update(&mut self, is_expansion: bool) {
        self.total_spread_events += 1;
        if is_expansion {
            self.spread_expansion_events += 1;
        }
    }

    pub const fn record_tick(
        &mut self,
        is_stale: bool,
        is_outlier: bool,
        is_fresh: bool,
        rx_mono: MonoNs,
    ) {
        self.total_ticks += 1;
        if is_stale {
            self.stale_ticks += 1;
        }
        if is_outlier {
            self.outlier_ticks += 1;
        }
        if is_fresh {
            self.fresh_ticks += 1;
        }
        if self.first_seen_ns.is_none() {
            self.first_seen_ns = Some(rx_mono);
        }
        self.last_seen_ns = Some(rx_mono);
    }

    pub fn record_consensus_deviation(&mut self, deviation_pips: f64) {
        if deviation_pips >= 0.0 && !deviation_pips.is_nan() {
            self.consensus_deviations_sum += deviation_pips;
            self.consensus_deviations_count += 1;
        }
    }

    #[must_use]
    pub fn compile(&self) -> BrokerFingerprint {
        self.compile_fingerprint()
    }

    #[must_use]
    pub fn compile_fingerprint(&self) -> BrokerFingerprint {
        let observed_lead_freq = if self.total_move_events > 0 {
            self.lead_events as f64 / self.total_move_events as f64
        } else {
            0.0
        };

        let observed_follow_freq = if self.total_move_events > 0 {
            self.follow_events as f64 / self.total_move_events as f64
        } else {
            0.0
        };

        let median_delay_ms = if self.delays_ms.is_empty() {
            0.0
        } else {
            let mut sorted: Vec<f64> = self.delays_ms.iter().copied().collect();
            sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            let mid = sorted.len() / 2;
            if sorted.len().is_multiple_of(2) {
                (sorted[mid - 1] + sorted[mid]) / 2.0
            } else {
                sorted[mid]
            }
        };

        let spread_expansion_freq = if self.total_spread_events > 0 {
            self.spread_expansion_events as f64 / self.total_spread_events as f64
        } else {
            0.0
        };

        let stale_freq = if self.total_ticks > 0 {
            self.stale_ticks as f64 / self.total_ticks as f64
        } else {
            0.0
        };

        let outlier_freq = if self.total_ticks > 0 {
            self.outlier_ticks as f64 / self.total_ticks as f64
        } else {
            0.0
        };

        let consensus_deviation_pips = if self.consensus_deviations_count > 0 {
            self.consensus_deviations_sum / self.consensus_deviations_count as f64
        } else {
            0.0
        };

        let window_ms = match (self.first_seen_ns, self.last_seen_ns) {
            (Some(f), Some(l)) => l.saturating_sub(f).as_millis() as u64,
            _ => 0,
        };

        let fresh_rate = if self.total_ticks > 0 {
            self.fresh_ticks as f64 / self.total_ticks as f64
        } else {
            1.0
        };

        let sample_context = SampleContext::new(self.total_ticks, window_ms, fresh_rate);

        BrokerFingerprint {
            broker_id: self.broker_id,
            observed_lead_freq,
            observed_follow_freq,
            median_delay_ms,
            spread_expansion_freq,
            stale_freq,
            outlier_freq,
            consensus_deviation_pips,
            sample_context,
        }
    }

    pub fn update_fingerprint(&self, fp: &mut BrokerFingerprint) {
        *fp = self.compile_fingerprint();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_broker_fingerprint_tracker_and_update() {
        let mut tracker = BrokerFingerprintTracker::new(42);

        // Leads and follows
        tracker.record_lead();
        tracker.record_lead();
        tracker.record_follow(8.0);
        tracker.record_follow(12.0);

        // Spreads
        tracker.record_spread_update(true);
        tracker.record_spread_update(false);

        // Ticks
        tracker.record_tick(false, false, true, MonoNs(1_000_000));
        tracker.record_tick(true, false, true, MonoNs(2_000_000));
        tracker.record_tick(false, true, false, MonoNs(3_000_000));

        // Consensus deviation
        tracker.record_consensus_deviation(0.6);
        tracker.record_consensus_deviation(0.4);

        let fp = tracker.compile_fingerprint();
        assert_eq!(fp.broker_id, 42);
        assert_eq!(fp.observed_lead_freq, 0.5); // 2 / 4
        assert_eq!(fp.observed_follow_freq, 0.5); // 2 / 4
        assert_eq!(fp.median_delay_ms, 10.0); // (8 + 12) / 2
        assert_eq!(fp.spread_expansion_freq, 0.5); // 1 / 2
        assert_eq!(fp.stale_freq, 1.0 / 3.0);
        assert_eq!(fp.outlier_freq, 1.0 / 3.0);
        assert!((fp.consensus_deviation_pips - 0.5).abs() < 1e-6);
        assert_eq!(fp.sample_context.sample_count, 3);
        assert_eq!(fp.sample_context.window_ms, 2);
        assert_eq!(fp.sample_context.fresh_rate, 2.0 / 3.0);

        // Updating an existing fingerprint
        let mut target_fp = BrokerFingerprint::empty(42);
        tracker.update_fingerprint(&mut target_fp);
        assert_eq!(target_fp, fp);
    }

    #[test]
    fn test_invariant_i12_no_single_score() {
        // Invariant I12 states that Broker Behaviour Fingerprints NEVER produce an aggregate single score or ranking.
        // Each metric must remain an independent dimension.
        let ctx = SampleContext::new(100, 10_000, 0.99);
        let fp = BrokerFingerprint::new(1, 0.45, 0.20, 8.5, 0.15, 0.02, 0.01, 0.35, ctx);

        // Verify independent observation dimensions
        assert_eq!(fp.observed_lead_freq, 0.45);
        assert_eq!(fp.observed_follow_freq, 0.20);
        assert_eq!(fp.median_delay_ms, 8.5);
        assert_eq!(fp.spread_expansion_freq, 0.15);
        assert_eq!(fp.stale_freq, 0.02);
        assert_eq!(fp.outlier_freq, 0.01);
        assert_eq!(fp.consensus_deviation_pips, 0.35);
        assert_eq!(fp.sample_context.sample_count, 100);
    }
}
