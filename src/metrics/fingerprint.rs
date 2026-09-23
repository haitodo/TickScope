//! Broker Behaviour Fingerprint and Persistence Tracking.
//! Reference: RFC Beta 0.3 (docs/improvement.md Sections 43-47, 52, 74-76, 91 Invariant I12).
//!
//! Invariant I12: NEVER produce an aggregate single score or ranking across these dimensions.
//! Each behavioral metric remains an independent observation dimension.

use crate::contracts::models::MoveEvent;
use crate::contracts::types::*;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

/// Statistical context attached to all observational metrics (RFC Section 52).
/// Prevents conflating low-sample or short-window observations with high-confidence baselines.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SampleContext {
    pub sample_count: u64,
    pub window_ms: u64,
    pub fresh_rate: f64,
}

impl SampleContext {
    pub const fn new(sample_count: u64, window_ms: u64, fresh_rate: f64) -> Self {
        Self {
            sample_count,
            window_ms,
            fresh_rate,
        }
    }

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

/// Tracks quote persistence per broker (RFC Section 45).
/// Measures elapsed time between quote revisions (`quote_duration_ms`).
/// Calculates median (p50) and p95 quote persistence.
#[derive(Debug, Clone)]
pub struct QuotePersistenceTracker {
    broker_id: BrokerId,
    last_quote: Option<Quote>,
    quote_start_mono: Option<MonoNs>,
    durations_ms: VecDeque<f64>,
    max_history: usize,
    first_seen_ns: Option<MonoNs>,
    last_seen_ns: Option<MonoNs>,
    valid_ticks: u64,
    fresh_ticks: u64,
}

impl QuotePersistenceTracker {
    pub const DEFAULT_MAX_HISTORY: usize = 5000;

    pub fn new(broker_id: BrokerId) -> Self {
        Self::with_capacity(broker_id, Self::DEFAULT_MAX_HISTORY)
    }

    pub fn with_capacity(broker_id: BrokerId, max_history: usize) -> Self {
        Self {
            broker_id,
            last_quote: None,
            quote_start_mono: None,
            durations_ms: VecDeque::with_capacity(max_history.min(4096)),
            max_history: max_history.max(10),
            first_seen_ns: None,
            last_seen_ns: None,
            valid_ticks: 0,
            fresh_ticks: 0,
        }
    }

    pub fn broker_id(&self) -> BrokerId {
        self.broker_id
    }

    /// Feeds a quote into the persistence tracker.
    /// If the quote price (bid or ask) has revised compared to the active quote,
    /// the duration of the prior quote is recorded and returned.
    pub fn on_quote(&mut self, quote: &Quote) -> Option<f64> {
        if !quote.is_valid || quote.is_warmup {
            return None;
        }

        self.valid_ticks += 1;
        self.fresh_ticks += 1;
        self.last_seen_ns = Some(quote.rx_mono_ns);
        if self.first_seen_ns.is_none() {
            self.first_seen_ns = Some(quote.rx_mono_ns);
        }

        match self.last_quote.as_ref() {
            None => {
                self.last_quote = Some(quote.clone());
                self.quote_start_mono = Some(quote.rx_mono_ns);
                None
            }
            Some(prev) => {
                // Check if price revised
                let revised = (quote.bid - prev.bid).abs() > f64::EPSILON
                    || (quote.ask - prev.ask).abs() > f64::EPSILON;

                if revised {
                    let start_mono = self.quote_start_mono.unwrap_or(prev.rx_mono_ns);
                    let duration_ms = quote.rx_mono_ns.saturating_sub(start_mono).as_millis();

                    self.record_duration_ms(duration_ms);

                    self.last_quote = Some(quote.clone());
                    self.quote_start_mono = Some(quote.rx_mono_ns);
                    Some(duration_ms)
                } else {
                    // Quote is unchanged: the active quote persists from original quote_start_mono.
                    // Keep existing quote_start_mono and update metadata.
                    self.last_quote = Some(quote.clone());
                    None
                }
            }
        }
    }

    /// Directly record a measured quote duration in milliseconds.
    pub fn record_duration_ms(&mut self, duration_ms: f64) {
        if duration_ms < 0.0 || duration_ms.is_nan() {
            return;
        }
        if self.durations_ms.len() >= self.max_history {
            self.durations_ms.pop_front();
        }
        self.durations_ms.push_back(duration_ms);
    }

    pub fn sample_count(&self) -> u64 {
        self.durations_ms.len() as u64
    }

    /// Median (p50) quote persistence in milliseconds.
    pub fn median_persistence_ms(&self) -> Option<f64> {
        self.percentile(0.50)
    }

    /// 95th percentile (p95) quote persistence in milliseconds.
    pub fn p95_persistence_ms(&self) -> Option<f64> {
        self.percentile(0.95)
    }

    /// Calculate arbitrary percentile in [0.0, 1.0].
    pub fn percentile(&self, pct: f64) -> Option<f64> {
        if self.durations_ms.is_empty() {
            return None;
        }
        if self.durations_ms.len() == 1 {
            return Some(self.durations_ms[0]);
        }

        let mut sorted: Vec<f64> = self.durations_ms.iter().copied().collect();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        let rank = pct.clamp(0.0, 1.0) * (sorted.len() - 1) as f64;
        let lower = rank.floor() as usize;
        let upper = rank.ceil() as usize;
        let weight = rank - lower as f64;

        Some(sorted[lower] * (1.0 - weight) + sorted[upper] * weight)
    }

    /// Produces the statistical sample context for this persistence tracker.
    pub fn sample_context(&self) -> SampleContext {
        let window_ms = match (self.first_seen_ns, self.last_seen_ns) {
            (Some(first), Some(last)) => last.saturating_sub(first).as_millis() as u64,
            _ => 0,
        };
        let fresh_rate = if self.valid_ticks > 0 {
            self.fresh_ticks as f64 / self.valid_ticks as f64
        } else {
            1.0
        };

        SampleContext::new(self.sample_count(), window_ms, fresh_rate)
    }

    pub fn clear(&mut self) {
        self.last_quote = None;
        self.quote_start_mono = None;
        self.durations_ms.clear();
        self.first_seen_ns = None;
        self.last_seen_ns = None;
        self.valid_ticks = 0;
        self.fresh_ticks = 0;
    }
}

/// Pending lead state for tracking follow vs reversion on move events.
#[derive(Debug, Clone)]
pub struct PendingLead {
    pub event: MoveEvent,
    pub created_at: MonoNs,
    pub anchor_mid: f64,
}

/// Tracks repricing persistence when a broker moves first (RFC Section 44).
/// Measures whether subsequent behavior followed (`others follow`) or reverted (`leader reverts`).
#[derive(Debug, Clone)]
pub struct RepricingPersistenceTracker {
    broker_id: BrokerId,
    total_leads: u64,
    follow_count: u64,
    reversion_count: u64,
    pending_lead: Option<PendingLead>,
    window_ns: u64,
}

impl RepricingPersistenceTracker {
    pub const DEFAULT_WINDOW_MS: u64 = 500;

    pub fn new(broker_id: BrokerId) -> Self {
        Self::with_window_ms(broker_id, Self::DEFAULT_WINDOW_MS)
    }

    pub fn with_window_ms(broker_id: BrokerId, window_ms: u64) -> Self {
        Self {
            broker_id,
            total_leads: 0,
            follow_count: 0,
            reversion_count: 0,
            pending_lead: None,
            window_ns: window_ms * 1_000_000,
        }
    }

    pub fn broker_id(&self) -> BrokerId {
        self.broker_id
    }

    pub fn total_leads(&self) -> u64 {
        self.total_leads
    }

    pub fn follow_count(&self) -> u64 {
        self.follow_count
    }

    pub fn reversion_count(&self) -> u64 {
        self.reversion_count
    }

    /// Record a lead event without immediate resolution.
    pub fn record_lead(&mut self) {
        self.total_leads += 1;
    }

    /// Record that other brokers followed this broker's lead.
    pub fn record_follow(&mut self) {
        self.follow_count += 1;
        if self.total_leads < self.follow_count + self.reversion_count {
            self.total_leads = self.follow_count + self.reversion_count;
        }
    }

    /// Record that this broker reverted without others following.
    pub fn record_reversion(&mut self) {
        self.reversion_count += 1;
        if self.total_leads < self.follow_count + self.reversion_count {
            self.total_leads = self.follow_count + self.reversion_count;
        }
    }

    /// Record an explicit outcome for a lead event.
    pub fn record_outcome(&mut self, followed: bool, reverted: bool) {
        self.total_leads += 1;
        if followed {
            self.follow_count += 1;
        }
        if reverted {
            self.reversion_count += 1;
        }
    }

    /// Follow rate: proportion of times other brokers followed the lead move.
    pub fn follow_rate(&self) -> f64 {
        let total = self.total_leads.max(self.follow_count + self.reversion_count);
        if total == 0 {
            0.0
        } else {
            self.follow_count as f64 / total as f64
        }
    }

    /// Reversion rate: proportion of times the leader reverted without followers.
    pub fn reversion_rate(&self) -> f64 {
        let total = self.total_leads.max(self.follow_count + self.reversion_count);
        if total == 0 {
            0.0
        } else {
            self.reversion_count as f64 / total as f64
        }
    }

    /// Event-driven hook when this broker initiates a lead move.
    pub fn on_lead_event(&mut self, event: &MoveEvent) {
        if event.broker_id != self.broker_id {
            return;
        }

        // Finalize previous pending lead if not yet resolved
        self.pending_lead = None;
        self.total_leads += 1;

        self.pending_lead = Some(PendingLead {
            event: event.clone(),
            created_at: event.rx_mono_ns,
            anchor_mid: event.anchor_mid,
        });
    }

    /// Event-driven hook for subsequent move events from any broker.
    pub fn on_subsequent_event(&mut self, event: &MoveEvent) {
        let pending = match self.pending_lead.as_ref() {
            Some(p) => p,
            None => return,
        };

        // Check if window expired
        if event.rx_mono_ns.saturating_sub(pending.created_at).0 > self.window_ns {
            self.pending_lead = None;
            return;
        }

        if event.broker_id != self.broker_id {
            // Other broker move: if direction matches, other broker followed!
            if event.direction == pending.event.direction {
                self.follow_count += 1;
                self.pending_lead = None;
            }
        } else {
            // Leader broker subsequent move: check for reversion (opposite direction)
            if event.direction != pending.event.direction {
                self.reversion_count += 1;
                self.pending_lead = None;
            }
        }
    }

    /// Checks if active pending lead has timed out.
    pub fn check_expiration(&mut self, now_mono: MonoNs) {
        if let Some(pending) = self.pending_lead.as_ref() {
            if now_mono.saturating_sub(pending.created_at).0 > self.window_ns {
                self.pending_lead = None;
            }
        }
    }
}

/// Broker Behaviour Fingerprint (RFC Beta 0.3 Sections 43, 75).
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
    pub fn new(
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

    pub fn empty(broker_id: BrokerId) -> Self {
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

    pub fn update_sample_context(&mut self, context: SampleContext) {
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

    pub fn broker_id(&self) -> BrokerId {
        self.broker_id
    }

    pub fn record_lead(&mut self) {
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

    pub fn record_spread_update(&mut self, is_expansion: bool) {
        self.total_spread_events += 1;
        if is_expansion {
            self.spread_expansion_events += 1;
        }
    }

    pub fn record_tick(
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

    pub fn compile(&self) -> BrokerFingerprint {
        self.compile_fingerprint()
    }

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
            if sorted.len() % 2 == 0 {
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
    use crate::contracts::models::MoveDirection;

    fn dummy_quote(broker_id: BrokerId, bid: f64, ask: f64, mono_ms: u64) -> Quote {
        Quote {
            tick_id: TickId {
                broker_id,
                session_id: 1,
                sequence: 100,
            },
            bid,
            ask,
            mid: (bid + ask) / 2.0,
            spread: ask - bid,
            rx_mono_ns: MonoNs(mono_ms * 1_000_000),
            utc_ms: None,
            is_warmup: false,
            is_valid: true,
        }
    }

    #[test]
    fn test_quote_persistence_basic() {
        let mut tracker = QuotePersistenceTracker::new(1);
        assert_eq!(tracker.median_persistence_ms(), None);
        assert_eq!(tracker.p95_persistence_ms(), None);

        // Quote 1 at 100ms
        let q1 = dummy_quote(1, 1.1000, 1.1002, 100);
        assert_eq!(tracker.on_quote(&q1), None);

        // Quote 2 revised at 120ms (duration of q1 = 20ms)
        let q2 = dummy_quote(1, 1.1001, 1.1003, 120);
        let d1 = tracker.on_quote(&q2);
        assert_eq!(d1, Some(20.0));

        // Quote 3 revised at 180ms (duration of q2 = 60ms)
        let q3 = dummy_quote(1, 1.1002, 1.1004, 180);
        let d2 = tracker.on_quote(&q3);
        assert_eq!(d2, Some(60.0));

        // Quote 4 revised at 220ms (duration of q3 = 40ms)
        let q4 = dummy_quote(1, 1.1003, 1.1005, 220);
        let d3 = tracker.on_quote(&q4);
        assert_eq!(d3, Some(40.0));

        assert_eq!(tracker.sample_count(), 3);
        // Durations: [20.0, 40.0, 60.0] -> median is 40.0
        assert_eq!(tracker.median_persistence_ms(), Some(40.0));

        let p95 = tracker.p95_persistence_ms().unwrap();
        assert!(p95 >= 40.0 && p95 <= 60.0);
    }

    #[test]
    fn test_quote_persistence_identical_quotes() {
        let mut tracker = QuotePersistenceTracker::new(2);

        // Quote at 100ms
        tracker.on_quote(&dummy_quote(2, 1.2000, 1.2002, 100));

        // Unchanged quotes arriving at 110ms and 120ms
        assert_eq!(
            tracker.on_quote(&dummy_quote(2, 1.2000, 1.2002, 110)),
            None
        );
        assert_eq!(
            tracker.on_quote(&dummy_quote(2, 1.2000, 1.2002, 120)),
            None
        );

        // Price revision arrives at 160ms -> persisted for 60ms (160 - 100)
        let dur = tracker.on_quote(&dummy_quote(2, 1.2001, 1.2003, 160));
        assert_eq!(dur, Some(60.0));
        assert_eq!(tracker.sample_count(), 1);
        assert_eq!(tracker.median_persistence_ms(), Some(60.0));
    }

    #[test]
    fn test_quote_persistence_edge_cases() {
        let mut tracker = QuotePersistenceTracker::new(3);
        assert_eq!(tracker.sample_count(), 0);
        assert_eq!(tracker.median_persistence_ms(), None);
        assert_eq!(tracker.p95_persistence_ms(), None);

        // Direct recording
        tracker.record_duration_ms(15.5);
        assert_eq!(tracker.sample_count(), 1);
        assert_eq!(tracker.median_persistence_ms(), Some(15.5));
        assert_eq!(tracker.p95_persistence_ms(), Some(15.5));

        // Invalid durations ignored
        tracker.record_duration_ms(-5.0);
        tracker.record_duration_ms(f64::NAN);
        assert_eq!(tracker.sample_count(), 1);
    }

    #[test]
    fn test_repricing_persistence_tracking() {
        let mut tracker = RepricingPersistenceTracker::new(1);
        assert_eq!(tracker.follow_rate(), 0.0);
        assert_eq!(tracker.reversion_rate(), 0.0);

        // Record outcomes: 3 followed, 1 reverted
        tracker.record_outcome(true, false);
        tracker.record_outcome(true, false);
        tracker.record_outcome(true, false);
        tracker.record_outcome(false, true);

        assert_eq!(tracker.total_leads(), 4);
        assert_eq!(tracker.follow_count(), 3);
        assert_eq!(tracker.reversion_count(), 1);
        assert_eq!(tracker.follow_rate(), 0.75);
        assert_eq!(tracker.reversion_rate(), 0.25);
    }

    #[test]
    fn test_repricing_persistence_event_flow() {
        let mut tracker = RepricingPersistenceTracker::with_window_ms(1, 200);

        let lead_event = MoveEvent {
            segment_id: 1,
            broker_id: 1,
            trigger_sequence: 10,
            rx_mono_ns: MonoNs(100_000_000), // 100ms
            direction: MoveDirection::Up,
            anchor_mid: 1.1000,
            current_mid: 1.1005,
            mid_delta_points: 5.0,
            bid_delta: 0.0005,
            ask_delta: 0.0005,
            mid_delta: 0.0005,
            spread_delta: 0.0,
            quality: crate::contracts::models::MoveQuality::BothSides,
        };

        // Lead initiated by broker 1
        tracker.on_lead_event(&lead_event);
        assert_eq!(tracker.total_leads(), 1);
        assert_eq!(tracker.follow_count(), 0);

        // Follower event from broker 2 within window (150ms) in same direction
        let follow_event = MoveEvent {
            broker_id: 2,
            rx_mono_ns: MonoNs(150_000_000),
            direction: MoveDirection::Up,
            ..lead_event.clone()
        };
        tracker.on_subsequent_event(&follow_event);
        assert_eq!(tracker.follow_count(), 1);
        assert_eq!(tracker.reversion_count(), 0);
        assert_eq!(tracker.follow_rate(), 1.0);

        // Second lead initiated by broker 1, but reverts
        let lead_event2 = MoveEvent {
            broker_id: 1,
            rx_mono_ns: MonoNs(400_000_000),
            direction: MoveDirection::Down,
            ..lead_event.clone()
        };
        tracker.on_lead_event(&lead_event2);
        assert_eq!(tracker.total_leads(), 2);

        // Broker 1 subsequently moves opposite direction (Up) at 450ms -> reversion
        let revert_event = MoveEvent {
            broker_id: 1,
            rx_mono_ns: MonoNs(450_000_000),
            direction: MoveDirection::Up,
            ..lead_event.clone()
        };
        tracker.on_subsequent_event(&revert_event);
        assert_eq!(tracker.reversion_count(), 1);
        assert_eq!(tracker.follow_rate(), 0.5);
        assert_eq!(tracker.reversion_rate(), 0.5);
    }

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
