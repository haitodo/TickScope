//! Multi-Broker Event Cluster (Burst), Quote Geometry, and Directional Breadth.
//! Reference: RFC Beta 0.3 (docs/improvement.md Sections 33-35, 40-42).
//!
//! # Core Invariants
//! - Burst is an observed cluster of move events across N brokers in a rolling time window.
//! - Burst is NEVER labeled as BUY, SELL, ENTRY, or MOMENTUM SIGNAL (§41).
//! - Breadth (e.g. UP 4/4) is an observational count, NEVER converted to a trade signal (§42).
//! - Quote Geometry distinguishes directional moves from spread expansion, compression, and mixed quotes (§34, §35).

use crate::contracts::models::{MoveDirection, MoveEvent};
use crate::contracts::types::{BrokerId, MonoNs};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};

/// Geometric classification of quote changes.
/// Reference: RFC Beta 0.3 §34, §35.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum QuoteGeometry {
    /// Both bid and ask moved in the same direction (both up or both down).
    TwoSideDirectional,
    /// Only bid price changed; ask price remained unchanged.
    BidOnly,
    /// Only ask price changed; bid price remained unchanged.
    AskOnly,
    /// Spread expanded symmetrically around mid price (bid dropped, ask rose, mid unchanged).
    SpreadExpansion,
    /// Spread compressed symmetrically around mid price (bid rose, ask dropped, mid unchanged).
    SpreadCompression,
    /// Bid and ask moved in opposite directions outward (asymmetric spread widening).
    OppositeSideMove,
    /// Bid and ask moved in opposite directions inward or mixed (e.g. Bid +3, Ask -1 per §35).
    MixedQuote,
}

impl QuoteGeometry {
    /// Classify quote change geometry from old and new bid/ask prices.
    pub fn classify(old_bid: f64, old_ask: f64, new_bid: f64, new_ask: f64) -> Self {
        classify_quote_geometry(old_bid, old_ask, new_bid, new_ask)
    }
}

/// Classify quote change geometry from old and new bid/ask prices.
///
/// Implements RFC Beta 0.3 §34 and §35:
/// - Bid +2, Ask unchanged -> `BidOnly`
/// - Bid unchanged, Ask +2 -> `AskOnly`
/// - Bid +2, Ask +2 -> `TwoSideDirectional`
/// - Bid -1, Ask +1 -> `SpreadExpansion` (symmetric)
/// - Bid +1, Ask -1 -> `SpreadCompression` (symmetric)
/// - Bid +3, Ask -1 -> `MixedQuote` (§35)
/// - Bid -3, Ask +1 -> `OppositeSideMove`
pub fn classify_quote_geometry(old_bid: f64, old_ask: f64, new_bid: f64, new_ask: f64) -> QuoteGeometry {
    let db = new_bid - old_bid;
    let da = new_ask - old_ask;
    let eps = 1e-9;

    let b_moved = db.abs() > eps;
    let a_moved = da.abs() > eps;

    if !b_moved && !a_moved {
        return QuoteGeometry::MixedQuote;
    }
    if b_moved && !a_moved {
        return QuoteGeometry::BidOnly;
    }
    if !b_moved && a_moved {
        return QuoteGeometry::AskOnly;
    }

    // Both sides moved
    let same_sign = (db > 0.0 && da > 0.0) || (db < 0.0 && da < 0.0);
    if same_sign {
        return QuoteGeometry::TwoSideDirectional;
    }

    // Opposite signs: one up, one down
    let mid_delta = (db + da) / 2.0;
    let symmetric = mid_delta.abs() <= eps;

    if symmetric {
        if da > 0.0 && db < 0.0 {
            QuoteGeometry::SpreadExpansion
        } else {
            QuoteGeometry::SpreadCompression
        }
    } else {
        // Asymmetric opposite moves
        if db > 0.0 && da < 0.0 {
            // E.g. Bid +3, Ask -1 (RFC Beta 0.3 §35)
            QuoteGeometry::MixedQuote
        } else {
            QuoteGeometry::OppositeSideMove
        }
    }
}

/// Multi-Broker Event Cluster / Burst observation across N brokers.
///
/// RFC Beta 0.3 §40, §41:
/// Represents a cluster of move events across fresh brokers in a rolling time window.
/// Must NEVER be converted into BUY, SELL, ENTRY, or confidence probability signals.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EventCluster {
    /// Move direction of the cluster (Up or Down)
    pub direction: MoveDirection,
    /// Broker IDs participating in the cluster (in order of first observation)
    pub participating_brokers: Vec<BrokerId>,
    /// Number of fresh brokers at cluster detection time
    pub fresh_count: usize,
    /// Total number of configured/known brokers
    pub total_brokers: usize,
    /// Elapsed time from first observed move to last observed move (in milliseconds)
    pub observed_span_ms: f64,
    /// Broker ID that first exhibited the move
    pub first_observed: BrokerId,
    /// Broker ID that last exhibited the move
    pub last_observed: BrokerId,
    /// Monotonic timestamp of the first observed move event
    pub start_mono: MonoNs,
    /// Monotonic timestamp of the last observed move event
    pub end_mono: MonoNs,
}

/// Directional Move Breadth across N brokers in a rolling window.
///
/// RFC Beta 0.3 §42:
/// Displays count ratios such as UP: 4/4 or DOWN: 1/4.
/// Strict invariant: UP 4/4 is NEVER a Buy signal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MoveBreadth {
    /// Number of brokers that observed an UP move in the rolling window
    pub up_count: usize,
    /// Number of brokers that observed a DOWN move in the rolling window
    pub down_count: usize,
    /// Number of brokers marked stale
    pub stale_count: usize,
    /// Total number of brokers
    pub total_brokers: usize,
}

impl MoveBreadth {
    /// Human-readable ratio string for UP breadth (e.g. "4/4").
    pub fn up_ratio_str(&self) -> String {
        format!("{}/{}", self.up_count, self.total_brokers)
    }

    /// Human-readable ratio string for DOWN breadth (e.g. "1/4").
    pub fn down_ratio_str(&self) -> String {
        format!("{}/{}", self.down_count, self.total_brokers)
    }
}

/// Rolling multi-broker burst and breadth detector.
#[derive(Debug, Clone)]
pub struct MultiBrokerBurstDetector {
    /// Time window in nanoseconds (e.g. 100ms = 100_000_000 ns)
    pub window_ns: u64,
    /// Minimum participating brokers required to detect an EventCluster
    pub min_brokers: usize,
    recent_events: VecDeque<MoveEvent>,
}

impl Default for MultiBrokerBurstDetector {
    fn default() -> Self {
        Self::new(100, 2)
    }
}

impl MultiBrokerBurstDetector {
    /// Create a new detector with window in milliseconds and minimum brokers.
    pub fn new(window_ms: u64, min_brokers: usize) -> Self {
        Self {
            window_ns: window_ms.saturating_mul(1_000_000),
            min_brokers: min_brokers.max(1),
            recent_events: VecDeque::with_capacity(128),
        }
    }

    pub fn window_ms(&self) -> u64 {
        self.window_ns / 1_000_000
    }

    pub fn clear(&mut self) {
        self.recent_events.clear();
    }

    /// Prune events older than `now_mono - window_ns`.
    pub fn prune_older_than(&mut self, now_mono: MonoNs) {
        let cutoff = now_mono.0.saturating_sub(self.window_ns);
        while let Some(front) = self.recent_events.front() {
            if front.rx_mono_ns.0 < cutoff {
                self.recent_events.pop_front();
            } else {
                break;
            }
        }
    }

    /// Record a new move event and detect whether an EventCluster is active.
    pub fn on_event(
        &mut self,
        event: MoveEvent,
        total_brokers: usize,
        fresh_count: usize,
    ) -> Option<EventCluster> {
        let cutoff = event.rx_mono_ns.0.saturating_sub(self.window_ns);
        while let Some(front) = self.recent_events.front() {
            if front.rx_mono_ns.0 < cutoff {
                self.recent_events.pop_front();
            } else {
                break;
            }
        }

        let direction = event.direction;
        self.recent_events.push_back(event);
        self.detect_cluster(direction, total_brokers, fresh_count)
    }

    /// Detect if there is a qualified EventCluster in the specified direction.
    pub fn detect_cluster(
        &self,
        direction: MoveDirection,
        total_brokers: usize,
        fresh_count: usize,
    ) -> Option<EventCluster> {
        if self.recent_events.is_empty() {
            return None;
        }

        self.detect_cluster_at(direction, total_brokers, fresh_count,
            self.recent_events.back()?.rx_mono_ns, &[])
    }

    /// Evaluate the current observation window, excluding unavailable feeds.
    pub fn detect_cluster_at(
        &self,
        direction: MoveDirection,
        total_brokers: usize,
        fresh_count: usize,
        now: MonoNs,
        stale_brokers: &[BrokerId],
    ) -> Option<EventCluster> {
        let cutoff = now.0.saturating_sub(self.window_ns);

        let mut participating_brokers = Vec::new();
        let mut first_observed = None;
        let mut last_observed = None;
        let mut start_mono = None;
        let mut end_mono = None;

        for ev in &self.recent_events {
            if ev.rx_mono_ns.0 >= cutoff && ev.rx_mono_ns <= now
                && ev.direction == direction && !stale_brokers.contains(&ev.broker_id) {
                if !participating_brokers.contains(&ev.broker_id) {
                    participating_brokers.push(ev.broker_id);
                }
                if start_mono.is_none() {
                    start_mono = Some(ev.rx_mono_ns);
                    first_observed = Some(ev.broker_id);
                }
                end_mono = Some(ev.rx_mono_ns);
                last_observed = Some(ev.broker_id);
            }
        }

        if participating_brokers.len() >= self.min_brokers {
            let start = start_mono?;
            let end = end_mono?;
            let span_ns = end.0.saturating_sub(start.0);
            let observed_span_ms = span_ns as f64 / 1_000_000.0;

            Some(EventCluster {
                direction,
                participating_brokers,
                fresh_count,
                total_brokers,
                observed_span_ms,
                first_observed: first_observed?,
                last_observed: last_observed?,
                start_mono: start,
                end_mono: end,
            })
        } else {
            None
        }
    }

    /// Compute directional MoveBreadth for the current rolling window.
    pub fn compute_breadth(
        &self,
        total_brokers: usize,
        stale_count: usize,
        now_mono: MonoNs,
    ) -> MoveBreadth {
        let cutoff = now_mono.0.saturating_sub(self.window_ns);
        let mut latest_dir: HashMap<BrokerId, MoveDirection> = HashMap::new();

        for ev in &self.recent_events {
            if ev.rx_mono_ns.0 >= cutoff && ev.rx_mono_ns <= now_mono {
                latest_dir.insert(ev.broker_id, ev.direction);
            }
        }

        let mut up_count = 0;
        let mut down_count = 0;
        for dir in latest_dir.values() {
            match dir {
                MoveDirection::Up => up_count += 1,
                MoveDirection::Down => down_count += 1,
            }
        }

        MoveBreadth {
            up_count,
            down_count,
            stale_count,
            total_brokers,
        }
    }

    /// Compute directional MoveBreadth explicitly filtering out known stale brokers.
    pub fn compute_breadth_with_stale_brokers(
        &self,
        total_brokers: usize,
        stale_brokers: &[BrokerId],
        now_mono: MonoNs,
    ) -> MoveBreadth {
        let cutoff = now_mono.0.saturating_sub(self.window_ns);
        let mut latest_dir: HashMap<BrokerId, MoveDirection> = HashMap::new();

        for ev in &self.recent_events {
            if ev.rx_mono_ns.0 >= cutoff && ev.rx_mono_ns <= now_mono {
                if !stale_brokers.contains(&ev.broker_id) {
                    latest_dir.insert(ev.broker_id, ev.direction);
                }
            }
        }

        let mut up_count = 0;
        let mut down_count = 0;
        for dir in latest_dir.values() {
            match dir {
                MoveDirection::Up => up_count += 1,
                MoveDirection::Down => down_count += 1,
            }
        }

        MoveBreadth {
            up_count,
            down_count,
            stale_count: stale_brokers.len(),
            total_brokers,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contracts::models::MoveQuality;

    #[test]
    fn current_clusters_expire_and_exclude_stale_feeds() {
        let mut detector = MultiBrokerBurstDetector::new(100, 2);
        detector.on_event(make_move_event(1, MoveDirection::Up, MonoNs(1_000_000)), 2, 2);
        detector.on_event(make_move_event(2, MoveDirection::Up, MonoNs(2_000_000)), 2, 2);
        assert!(detector.detect_cluster_at(MoveDirection::Up, 2, 2, MonoNs(3_000_000), &[]).is_some());
        assert!(detector.detect_cluster_at(MoveDirection::Up, 2, 1, MonoNs(3_000_000), &[2]).is_none());
        assert!(detector.detect_cluster_at(MoveDirection::Up, 2, 2, MonoNs(200_000_000), &[]).is_none());
    }

    fn make_move_event(
        broker_id: BrokerId,
        direction: MoveDirection,
        rx_mono_ns: MonoNs,
    ) -> MoveEvent {
        MoveEvent {
            segment_id: 1,
            broker_id,
            trigger_sequence: 1,
            rx_mono_ns,
            direction,
            anchor_mid: 100.0,
            current_mid: if direction == MoveDirection::Up { 100.05 } else { 99.95 },
            mid_delta_points: 5.0,
            bid_delta: 0.05,
            ask_delta: 0.05,
            mid_delta: 0.05,
            spread_delta: 0.0,
            quality: MoveQuality::BothSides,
        }
    }

    #[test]
    fn test_quote_geometry_classification() {
        // 1. TwoSideDirectional (parallel and directional)
        assert_eq!(
            classify_quote_geometry(100.0, 100.2, 100.1, 100.3),
            QuoteGeometry::TwoSideDirectional
        );
        assert_eq!(
            classify_quote_geometry(100.0, 100.2, 99.9, 100.1),
            QuoteGeometry::TwoSideDirectional
        );
        assert_eq!(
            classify_quote_geometry(100.0, 100.2, 100.1, 100.4),
            QuoteGeometry::TwoSideDirectional
        );

        // 2. BidOnly (RFC Beta 0.3 §35)
        assert_eq!(
            classify_quote_geometry(100.0, 100.2, 100.1, 100.2),
            QuoteGeometry::BidOnly
        );

        // 3. AskOnly
        assert_eq!(
            classify_quote_geometry(100.0, 100.2, 100.0, 100.3),
            QuoteGeometry::AskOnly
        );

        // 4. SpreadExpansion (symmetric: bid -0.01, ask +0.01)
        assert_eq!(
            classify_quote_geometry(100.00, 100.02, 99.99, 100.03),
            QuoteGeometry::SpreadExpansion
        );

        // 5. SpreadCompression (symmetric: bid +0.01, ask -0.01)
        assert_eq!(
            classify_quote_geometry(100.00, 100.04, 100.01, 100.03),
            QuoteGeometry::SpreadCompression
        );

        // 6. MixedQuote (asymmetric opposite moves: Bid +0.03, Ask -0.01 per §35)
        assert_eq!(
            classify_quote_geometry(100.00, 100.05, 100.03, 100.04),
            QuoteGeometry::MixedQuote
        );

        // 7. OppositeSideMove (asymmetric expansion: Bid -0.03, Ask +0.01)
        assert_eq!(
            classify_quote_geometry(100.03, 100.05, 100.00, 100.06),
            QuoteGeometry::OppositeSideMove
        );
    }

    #[test]
    fn test_multi_broker_cluster_up_burst() {
        // Test scenario directly from RFC Beta 0.3 §40:
        // A UP @ 0ms
        // B UP @ 4ms
        // C UP @ 7ms
        // D UP @ 10ms
        // -> UP Burst, Fresh: 4/4, Duration: 10ms, First: A, Last: D
        let mut detector = MultiBrokerBurstDetector::new(100, 4);

        let t0 = MonoNs(0);
        let t1 = MonoNs(4_000_000);  // 4ms
        let t2 = MonoNs(7_000_000);  // 7ms
        let t3 = MonoNs(10_000_000); // 10ms

        assert_eq!(detector.on_event(make_move_event(1, MoveDirection::Up, t0), 4, 4), None);
        assert_eq!(detector.on_event(make_move_event(2, MoveDirection::Up, t1), 4, 4), None);
        assert_eq!(detector.on_event(make_move_event(3, MoveDirection::Up, t2), 4, 4), None);

        let cluster = detector.on_event(make_move_event(4, MoveDirection::Up, t3), 4, 4);
        assert!(cluster.is_some());
        let c = cluster.unwrap();

        assert_eq!(c.direction, MoveDirection::Up);
        assert_eq!(c.participating_brokers, vec![1, 2, 3, 4]);
        assert_eq!(c.fresh_count, 4);
        assert_eq!(c.total_brokers, 4);
        assert_eq!(c.first_observed, 1);
        assert_eq!(c.last_observed, 4);
        assert_eq!(c.start_mono, t0);
        assert_eq!(c.end_mono, t3);
        assert!((c.observed_span_ms - 10.0).abs() < 1e-6);

        // Later event outside 100ms window prunes earlier events
        let t_late = MonoNs(120_000_000); // 120ms
        let late_cluster = detector.on_event(make_move_event(5, MoveDirection::Up, t_late), 5, 5);
        // Only broker 5 is within [20ms, 120ms], so count is 1 < 4
        assert_eq!(late_cluster, None);
    }

    #[test]
    fn test_move_breadth_calculation() {
        let mut detector = MultiBrokerBurstDetector::new(100, 2);

        let t0 = MonoNs(1_000_000_000);
        detector.on_event(make_move_event(1, MoveDirection::Up, t0), 4, 4);
        detector.on_event(make_move_event(2, MoveDirection::Up, MonoNs(t0.0 + 5_000_000)), 4, 4);
        detector.on_event(make_move_event(3, MoveDirection::Up, MonoNs(t0.0 + 10_000_000)), 4, 4);
        detector.on_event(make_move_event(4, MoveDirection::Up, MonoNs(t0.0 + 15_000_000)), 4, 4);

        // 1. All 4 brokers UP
        let breadth = detector.compute_breadth(4, 0, MonoNs(t0.0 + 20_000_000));
        assert_eq!(breadth.up_count, 4);
        assert_eq!(breadth.down_count, 0);
        assert_eq!(breadth.stale_count, 0);
        assert_eq!(breadth.total_brokers, 4);
        assert_eq!(breadth.up_ratio_str(), "4/4");

        // 2. With 1 stale broker (broker 4)
        let breadth_stale = detector.compute_breadth_with_stale_brokers(4, &[4], MonoNs(t0.0 + 20_000_000));
        assert_eq!(breadth_stale.up_count, 3);
        assert_eq!(breadth_stale.down_count, 0);
        assert_eq!(breadth_stale.stale_count, 1);
        assert_eq!(breadth_stale.total_brokers, 4);
        assert_eq!(breadth_stale.up_ratio_str(), "3/4");

        // 3. Mixed UP and DOWN
        detector.on_event(make_move_event(2, MoveDirection::Down, MonoNs(t0.0 + 30_000_000)), 4, 4);
        let breadth_mixed = detector.compute_breadth(4, 0, MonoNs(t0.0 + 35_000_000));
        // Broker 1 UP, Broker 2 DOWN, Broker 3 UP, Broker 4 UP
        assert_eq!(breadth_mixed.up_count, 3);
        assert_eq!(breadth_mixed.down_count, 1);
        assert_eq!(breadth_mixed.total_brokers, 4);
        assert_eq!(breadth_mixed.down_ratio_str(), "1/4");
    }
}
