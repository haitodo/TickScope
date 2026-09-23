//! Multi-Broker Consensus and Dispersion Engine.
//! Reference: RFC Beta 0.3 (docs/improvement.md Sections 10-15, 69-70).
//!
//! # Core Invariants
//! - "Observed Broker Median" is a mathematical consensus of fresh quotes, NOT "true market price".
//! - Deviation from consensus is named "Deviation from Broker Median" (never "error" or "bad broker").
//! - Bid A > Ask B is named "Crossed Snapshot" (never "arbitrage").
//! - Stale quotes are strictly excluded from fresh counts and consensus calculations (§14, §15).
//! - Low-N handling (§70): MAD requires N >= 4 fresh brokers; for N < 4 it returns None.

use crate::contracts::types::{BrokerId, MonoNs, Quote};
use serde::{Deserialize, Serialize};

/// Record of a broker quote's deviation from the Observed Broker Median.
///
/// RFC Beta 0.3 §12: Named "Deviation from Broker Median", never "error" or "bad".
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BrokerDeviation {
    pub broker_id: BrokerId,
    pub mid: f64,
    /// Signed deviation: `mid - consensus_mid`
    pub deviation: f64,
    /// Absolute deviation: `|mid - consensus_mid|`
    pub abs_deviation: f64,
}

/// Record of an observed crossed snapshot across two brokers.
///
/// RFC Beta 0.3 §13: Named "Crossed Snapshot", never "arbitrage opportunity".
/// This occurs when Broker A Bid > Broker B Ask due to receive timing, staleness, or latency differences.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CrossedSnapshot {
    pub broker_a: BrokerId,
    pub broker_b: BrokerId,
    pub bid_a: f64,
    pub ask_b: f64,
    /// The crossed difference: `bid_a - ask_b` (> 0.0)
    pub diff: f64,
}

/// Observed Broker Consensus and robust dispersion metrics.
/// Reference: RFC Beta 0.3 §10, §11, §69, §70.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ObservedBrokerConsensus {
    /// Number of quotes considered Fresh (rx_age <= stale_after_ms)
    pub fresh_count: usize,
    /// Total number of quotes received in snapshot
    pub total_count: usize,
    /// Observed Broker Median Mid across fresh quotes
    pub consensus_mid: Option<f64>,
    /// Minimum mid price among fresh quotes
    pub mid_min: Option<f64>,
    /// Maximum mid price among fresh quotes
    pub mid_max: Option<f64>,
    /// Range of mid prices: `mid_max - mid_min`
    pub mid_range: Option<f64>,
    /// Range of bid prices: `bid_max - bid_min`
    pub bid_range: Option<f64>,
    /// Range of ask prices: `ask_max - ask_min`
    pub ask_range: Option<f64>,
    /// Median Absolute Deviation (MAD): median(|mid_i - median|).
    /// Low-N rule (§70): Computed only for N >= 4 fresh brokers; None for N < 4.
    pub median_abs_deviation: Option<f64>,
    /// Outlier quotes deviating significantly from consensus median
    pub outliers: Vec<BrokerDeviation>,
    /// Crossed snapshots where Bid A > Ask B
    pub crossed_snapshots: Vec<CrossedSnapshot>,
}

/// Configuration and calculator for Observed Broker Consensus.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConsensusCalculator {
    /// Staleness threshold in milliseconds. Quotes older than this are excluded.
    pub stale_after_ms: u64,
    /// Optional absolute price threshold for outlier detection.
    pub outlier_threshold: Option<f64>,
    /// Multiplier for MAD-based outlier detection (default: 3.0 for > 3*MAD).
    pub mad_multiplier: f64,
}

impl Default for ConsensusCalculator {
    fn default() -> Self {
        Self {
            stale_after_ms: 1000,
            outlier_threshold: None,
            mad_multiplier: 3.0,
        }
    }
}

impl ConsensusCalculator {
    pub fn new(stale_after_ms: u64) -> Self {
        Self {
            stale_after_ms,
            outlier_threshold: None,
            mad_multiplier: 3.0,
        }
    }

    pub fn with_threshold(mut self, threshold: f64) -> Self {
        self.outlier_threshold = Some(threshold);
        self
    }

    pub fn with_mad_multiplier(mut self, multiplier: f64) -> Self {
        self.mad_multiplier = multiplier;
        self
    }

    /// Compute Observed Broker Consensus from an iterable of quotes.
    pub fn compute<'a, I>(&self, quotes: I, now_mono: MonoNs) -> ObservedBrokerConsensus
    where
        I: IntoIterator<Item = &'a Quote>,
    {
        let stale_after_ns = self.stale_after_ms.saturating_mul(1_000_000);

        let mut total_count = 0;
        let mut fresh_quotes: Vec<&'a Quote> = Vec::new();

        for q in quotes {
            total_count += 1;
            if !q.is_valid || q.is_warmup || q.bid.is_nan() || q.ask.is_nan() || q.mid.is_nan() {
                continue;
            }
            let age_ns = now_mono.0.saturating_sub(q.rx_mono_ns.0);
            if age_ns <= stale_after_ns {
                fresh_quotes.push(q);
            }
        }

        let fresh_count = fresh_quotes.len();
        if fresh_count == 0 {
            return ObservedBrokerConsensus {
                fresh_count: 0,
                total_count,
                consensus_mid: None,
                mid_min: None,
                mid_max: None,
                mid_range: None,
                bid_range: None,
                ask_range: None,
                median_abs_deviation: None,
                outliers: Vec::new(),
                crossed_snapshots: Vec::new(),
            };
        }

        // Mid, Bid, Ask ranges
        let mut mid_min = f64::INFINITY;
        let mut mid_max = f64::NEG_INFINITY;
        let mut bid_min = f64::INFINITY;
        let mut bid_max = f64::NEG_INFINITY;
        let mut ask_min = f64::INFINITY;
        let mut ask_max = f64::NEG_INFINITY;

        // Copy fresh mids into stack buffer if <= 32 brokers (zero dynamic allocation)
        let mut stack_mids = [0.0f64; 32];
        let mut heap_mids = Vec::new();

        let mids: &mut [f64] = if fresh_count <= 32 {
            for (i, q) in fresh_quotes.iter().enumerate() {
                stack_mids[i] = q.mid;
                if q.mid < mid_min { mid_min = q.mid; }
                if q.mid > mid_max { mid_max = q.mid; }
                if q.bid < bid_min { bid_min = q.bid; }
                if q.bid > bid_max { bid_max = q.bid; }
                if q.ask < ask_min { ask_min = q.ask; }
                if q.ask > ask_max { ask_max = q.ask; }
            }
            &mut stack_mids[..fresh_count]
        } else {
            heap_mids.reserve(fresh_count);
            for q in &fresh_quotes {
                heap_mids.push(q.mid);
                if q.mid < mid_min { mid_min = q.mid; }
                if q.mid > mid_max { mid_max = q.mid; }
                if q.bid < bid_min { bid_min = q.bid; }
                if q.bid > bid_max { bid_max = q.bid; }
                if q.ask < ask_min { ask_min = q.ask; }
                if q.ask > ask_max { ask_max = q.ask; }
            }
            &mut heap_mids[..]
        };

        let consensus_mid = compute_median_in_place(mids);

        // Low-N MAD calculation (§70): N >= 4 computes MAD; N < 4 returns None
        let median_abs_deviation = if fresh_count >= 4 {
            if let Some(med) = consensus_mid {
                compute_mad(fresh_quotes.iter().map(|q| q.mid), fresh_count, med)
            } else {
                None
            }
        } else {
            None
        };

        // Outlier detection (> threshold or > 3*MAD)
        let mut outliers = Vec::new();
        if let Some(med) = consensus_mid {
            for q in &fresh_quotes {
                let diff = q.mid - med;
                let abs_diff = diff.abs();
                let mut is_outlier = false;

                if let Some(mad) = median_abs_deviation {
                    if mad > 1e-9 && abs_diff > self.mad_multiplier * mad {
                        is_outlier = true;
                    }
                }
                if let Some(thresh) = self.outlier_threshold {
                    if abs_diff > thresh {
                        is_outlier = true;
                    }
                }

                if is_outlier {
                    outliers.push(BrokerDeviation {
                        broker_id: q.tick_id.broker_id,
                        mid: q.mid,
                        deviation: diff,
                        abs_deviation: abs_diff,
                    });
                }
            }
        }

        // Crossed snapshot detection: Bid A > Ask B
        let mut crossed_snapshots = Vec::new();
        for (i, qa) in fresh_quotes.iter().enumerate() {
            for (j, qb) in fresh_quotes.iter().enumerate() {
                if i != j && qa.bid > qb.ask {
                    let diff = qa.bid - qb.ask;
                    crossed_snapshots.push(CrossedSnapshot {
                        broker_a: qa.tick_id.broker_id,
                        broker_b: qb.tick_id.broker_id,
                        bid_a: qa.bid,
                        ask_b: qb.ask,
                        diff,
                    });
                }
            }
        }

        ObservedBrokerConsensus {
            fresh_count,
            total_count,
            consensus_mid,
            mid_min: Some(mid_min),
            mid_max: Some(mid_max),
            mid_range: Some(mid_max - mid_min),
            bid_range: Some(bid_max - bid_min),
            ask_range: Some(ask_max - ask_min),
            median_abs_deviation,
            outliers,
            crossed_snapshots,
        }
    }

    /// Convenience helper for computing from a slice of quotes.
    pub fn compute_slice(&self, quotes: &[Quote], now_mono: MonoNs) -> ObservedBrokerConsensus {
        self.compute(quotes.iter(), now_mono)
    }
}

/// Compute median in-place on a mutable slice of f64.
fn compute_median_in_place(values: &mut [f64]) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = values.len();
    if n % 2 == 1 {
        Some(values[n / 2])
    } else {
        Some((values[n / 2 - 1] + values[n / 2]) / 2.0)
    }
}

/// Compute Median Absolute Deviation: median(|x_i - median|).
fn compute_mad<I>(mids: I, count: usize, median: f64) -> Option<f64>
where
    I: IntoIterator<Item = f64>,
{
    if count == 0 {
        return None;
    }

    let mut stack_devs = [0.0f64; 32];
    let mut heap_devs = Vec::new();

    let devs: &mut [f64] = if count <= 32 {
        for (i, m) in mids.into_iter().enumerate() {
            stack_devs[i] = (m - median).abs();
        }
        &mut stack_devs[..count]
    } else {
        heap_devs.reserve(count);
        for m in mids {
            heap_devs.push((m - median).abs());
        }
        &mut heap_devs[..]
    };

    compute_median_in_place(devs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contracts::types::TickId;

    fn make_quote(broker_id: BrokerId, bid: f64, ask: f64, rx_mono_ns: MonoNs) -> Quote {
        Quote {
            tick_id: TickId {
                broker_id,
                session_id: 1,
                sequence: 1,
            },
            bid,
            ask,
            mid: (bid + ask) / 2.0,
            spread: ask - bid,
            rx_mono_ns,
            utc_ms: None,
            is_warmup: false,
            is_valid: true,
        }
    }

    #[test]
    fn test_normal_5_broker_consensus() {
        let calc = ConsensusCalculator::new(1000);
        let now = MonoNs(1_000_000_000);

        let quotes = vec![
            make_quote(1, 100.00, 100.05, now), // mid 100.025
            make_quote(2, 100.01, 100.06, now), // mid 100.035
            make_quote(3, 100.02, 100.07, now), // mid 100.045 (median)
            make_quote(4, 100.03, 100.08, now), // mid 100.055
            make_quote(5, 100.04, 100.09, now), // mid 100.065
        ];

        let result = calc.compute_slice(&quotes, now);
        assert_eq!(result.fresh_count, 5);
        assert_eq!(result.total_count, 5);
        assert!((result.consensus_mid.unwrap() - 100.045).abs() < 1e-9);
        assert!((result.mid_min.unwrap() - 100.025).abs() < 1e-9);
        assert!((result.mid_max.unwrap() - 100.065).abs() < 1e-9);
        assert!((result.mid_range.unwrap() - 0.04).abs() < 1e-9);
        assert!((result.bid_range.unwrap() - 0.04).abs() < 1e-9);
        assert!((result.ask_range.unwrap() - 0.04).abs() < 1e-9);

        // Deviations from 100.045: [0.02, 0.01, 0.00, 0.01, 0.02] -> sorted [0.00, 0.01, 0.01, 0.02, 0.02]
        // MAD is 0.01
        assert!(result.median_abs_deviation.is_some());
        assert!((result.median_abs_deviation.unwrap() - 0.01).abs() < 1e-9);
        assert!(result.outliers.is_empty());
        assert!(result.crossed_snapshots.is_empty());
    }

    #[test]
    fn test_stale_broker_exclusion() {
        let calc = ConsensusCalculator::new(1000); // 1000ms stale threshold
        let now = MonoNs(2_000_000_000); // 2.0s

        let quotes = vec![
            make_quote(1, 100.00, 100.02, MonoNs(1_500_000_000)), // age 500ms -> Fresh
            make_quote(2, 100.02, 100.04, MonoNs(1_800_000_000)), // age 200ms -> Fresh
            make_quote(3, 100.04, 100.06, MonoNs(1_900_000_000)), // age 100ms -> Fresh
            make_quote(4, 100.06, 100.08, MonoNs(1_950_000_000)), // age 50ms  -> Fresh
            make_quote(5, 105.00, 105.02, MonoNs(500_000_000)),   // age 1500ms -> STALE!
        ];

        let result = calc.compute_slice(&quotes, now);
        assert_eq!(result.total_count, 5);
        assert_eq!(result.fresh_count, 4);
        // Fresh mids: 100.01, 100.03, 100.05, 100.07. Median = (100.03 + 100.05) / 2 = 100.04
        assert!((result.consensus_mid.unwrap() - 100.04).abs() < 1e-9);
        assert!((result.mid_max.unwrap() - 100.07).abs() < 1e-9); // 105.01 excluded!
    }

    #[test]
    fn test_low_n_3_brokers_no_mad() {
        let calc = ConsensusCalculator::new(1000);
        let now = MonoNs(1_000_000_000);

        let quotes = vec![
            make_quote(1, 100.00, 100.02, now), // mid 100.01
            make_quote(2, 100.02, 100.04, now), // mid 100.03
            make_quote(3, 100.04, 100.06, now), // mid 100.05
        ];

        let result = calc.compute_slice(&quotes, now);
        assert_eq!(result.fresh_count, 3);
        assert!((result.consensus_mid.unwrap() - 100.03).abs() < 1e-9);
        // RFC Beta 0.3 §70: N < 4 must return None for MAD
        assert_eq!(result.median_abs_deviation, None);
    }

    #[test]
    fn test_outlier_detection_via_mad_and_threshold() {
        let calc = ConsensusCalculator::new(1000).with_threshold(0.50);
        let now = MonoNs(1_000_000_000);

        let quotes = vec![
            make_quote(1, 100.00, 100.02, now), // mid 100.01
            make_quote(2, 100.01, 100.03, now), // mid 100.02
            make_quote(3, 100.02, 100.04, now), // mid 100.03
            make_quote(4, 100.03, 100.05, now), // mid 100.04
            make_quote(5, 102.00, 102.02, now), // mid 102.01 -> Deviation from Broker Median
        ];

        let result = calc.compute_slice(&quotes, now);
        assert_eq!(result.fresh_count, 5);
        assert!((result.consensus_mid.unwrap() - 100.03).abs() < 1e-9);
        assert_eq!(result.outliers.len(), 1);
        let out = &result.outliers[0];
        assert_eq!(out.broker_id, 5);
        assert!((out.mid - 102.01).abs() < 1e-9);
        assert!((out.deviation - 1.98).abs() < 1e-9);
        assert!((out.abs_deviation - 1.98).abs() < 1e-9);
    }

    #[test]
    fn test_crossed_snapshots_detection() {
        let calc = ConsensusCalculator::new(1000);
        let now = MonoNs(1_000_000_000);

        // Broker 1 Bid is 100.08, Broker 2 Ask is 100.05 -> Bid 1 > Ask 2!
        // Broker 1 Bid is 100.08, Broker 3 Ask is 100.07 -> Bid 1 > Ask 3!
        // Broker 3 Bid is 100.06, Broker 2 Ask is 100.05 -> Bid 3 > Ask 2!
        let quotes = vec![
            make_quote(1, 100.08, 100.10, now),
            make_quote(2, 100.03, 100.05, now),
            make_quote(3, 100.06, 100.07, now),
        ];

        let result = calc.compute_slice(&quotes, now);
        assert_eq!(result.crossed_snapshots.len(), 3);
        // Pair 1: Broker 1 Bid (100.08) > Broker 2 Ask (100.05)
        let cs1 = result.crossed_snapshots.iter().find(|c| c.broker_a == 1 && c.broker_b == 2).unwrap();
        assert!((cs1.diff - 0.03).abs() < 1e-9);

        // Pair 2: Broker 1 Bid (100.08) > Broker 3 Ask (100.07)
        let cs2 = result.crossed_snapshots.iter().find(|c| c.broker_a == 1 && c.broker_b == 3).unwrap();
        assert!((cs2.diff - 0.01).abs() < 1e-9);

        // Pair 3: Broker 3 Bid (100.06) > Broker 2 Ask (100.05)
        let cs3 = result.crossed_snapshots.iter().find(|c| c.broker_a == 3 && c.broker_b == 2).unwrap();
        assert!((cs3.diff - 0.01).abs() < 1e-9);
    }
}
