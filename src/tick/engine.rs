//! Tick Engine: N-broker watermark merge, ledger, and pipeline coordination.

use crate::config::{AppConfig, TimezoneRule};
use crate::core::models::{LeadLagMatch, PriceMode, RealtimeQuotePoint};
use crate::core::types::{
    BrokerId, ConnectionState, Diagnostic, DiagnosticSeverity, FreshnessState, HealthState,
    HeartbeatState, IntegrityState, MonoNs, NormalizationState, ObservedTick, PhaseState, Quote,
    SequenceDisposition, SessionId, TickId, UtcMs,
};
use crate::metrics::burst::MultiBrokerBurstDetector;
use crate::metrics::consensus::ConsensusCalculator;
use crate::metrics::diagnostics::{DiagnosticStage, DiagnosticsHandle};
use crate::metrics::fingerprint::BrokerFingerprintTracker;
use crate::metrics::hypothesis::HypothesisEngine;
use crate::metrics::lead_lag::SignificantMidMoveDetector;
use crate::metrics::persistence::{QuotePersistenceTracker, RepricingPersistenceTracker};
use crate::metrics::price_diff::PairDifferenceTracker;
use crate::metrics::spread::SpreadTracker;
use crate::protocol::{
    FramePayload, IngressItem, ReceivedFrame, HB_FLAG_HAS_OFFSET_SAMPLE, HEADER_FLAG_WARMUP,
    PHASE_WARMING,
};
use crate::tick::candle::CandleBook;
use crate::tick::ledger::SequenceLedger;
use crate::tick::matcher::OneToOneEventMatcher;
use crate::tick::normalize::{normalize_tick, round_to_hourly_offset};
use std::collections::{HashMap, VecDeque};
use std::time::Instant;

const MAX_DIAGNOSTICS: usize = 2_048;

pub struct BrokerChannelState {
    pub is_connected: bool,
    pub generation: u64,
    pub watermark: MonoNs,
    pub pending_frames: VecDeque<ReceivedFrame>,
    pub pending_frame_bytes: usize,
    pub max_pending_frames: usize,
    pub max_pending_bytes: usize,
    pub ledger: SequenceLedger,
    pub session_id: Option<SessionId>,
    pub timezone_rule: TimezoneRule,
    pub auto_utc_offset: bool,
    pub active_utc_offset_sec: i32,
    pub utc_verified: bool,
}

/// Broker name for log messages, or `"Unknown"` when the id is not configured.
fn broker_name(brokers: &[crate::config::BrokerConfig], broker_id: BrokerId) -> &str {
    brokers
        .iter()
        .find(|b| b.id == broker_id)
        .map_or("Unknown", |b| b.name.as_str())
}
pub struct TickEngine {
    pub(crate) config: AppConfig,
    pub channels: HashMap<BrokerId, BrokerChannelState>,
    pub(crate) candle_book: CandleBook,
    pub(crate) mid_candle_book: CandleBook,
    pub(crate) spread_trackers: HashMap<BrokerId, SpreadTracker>,
    pub(crate) latest_quotes: HashMap<BrokerId, Quote>,
    pub(crate) fast_quotes: HashMap<BrokerId, Quote>,
    pub(crate) health_states: HashMap<BrokerId, HealthState>,
    pub(crate) pair_tracker: PairDifferenceTracker,
    pub(crate) move_detectors: HashMap<BrokerId, SignificantMidMoveDetector>,
    pub(crate) matcher: OneToOneEventMatcher,
    pub(crate) latest_pair_match: Option<LeadLagMatch>,
    pub(crate) active_pair: (BrokerId, BrokerId),
    pub(crate) projection_revision: u64,
    pub(crate) current_watermark: MonoNs,
    pub(crate) diagnostics: VecDeque<Diagnostic>,

    // Multi-Broker, Microstructure & Hypothesis additions (RFC Beta 0.3)
    pub consensus_calc: ConsensusCalculator,
    pub burst_detector: MultiBrokerBurstDetector,
    pub quote_persistence: HashMap<BrokerId, QuotePersistenceTracker>,
    pub repricing_persistence: HashMap<BrokerId, RepricingPersistenceTracker>,
    pub fingerprint_trackers: HashMap<BrokerId, BrokerFingerprintTracker>,
    pub hypothesis_engine: HypothesisEngine,
    pub(crate) performance_diagnostics: Option<DiagnosticsHandle>,
    pub(crate) processing_mono_ns: MonoNs,
    pub realtime_quote_history: VecDeque<RealtimeQuotePoint>,
}

impl TickEngine {
    #[must_use]
    pub fn new(config: AppConfig) -> Self {
        let mut channels = HashMap::new();
        let mut spread_trackers = HashMap::new();
        let mut health_states = HashMap::new();
        let mut move_detectors = HashMap::new();

        let candle_book = CandleBook::with_retentions(&config.history.retentions);
        let mid_candle_book = CandleBook::with_retentions(&config.history.retentions);

        let mut quote_persistence = HashMap::new();
        let mut repricing_persistence = HashMap::new();
        let mut fingerprint_trackers = HashMap::new();

        let now_sec = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs() as i64);

        for b in &config.brokers {
            let initial_offset = b.timezone_rule.resolve_offset(now_sec, b.utc_offset_sec);
            let initial_verified = match b.timezone_rule {
                TimezoneRule::NyClose | TimezoneRule::Jst | TimezoneRule::Utc => true,
                TimezoneRule::Fixed => b.utc_verified,
            };

            channels.insert(
                b.id,
                BrokerChannelState {
                    is_connected: false,
                    generation: 0,
                    watermark: MonoNs::ZERO,
                    pending_frames: VecDeque::new(),
                    pending_frame_bytes: 0,
                    max_pending_frames: config.ingress.max_frames_per_broker,
                    max_pending_bytes: config.ingress.max_bytes_per_broker,
                    ledger: SequenceLedger::new(config.history.ledger_capacity),
                    session_id: None,
                    timezone_rule: b.timezone_rule,
                    auto_utc_offset: b.auto_utc_offset,
                    active_utc_offset_sec: initial_offset,
                    utc_verified: initial_verified,
                },
            );

            spread_trackers.insert(b.id, SpreadTracker::new(b.id));
            quote_persistence.insert(b.id, QuotePersistenceTracker::new(b.id));
            repricing_persistence.insert(b.id, RepricingPersistenceTracker::new(b.id));
            fingerprint_trackers.insert(b.id, BrokerFingerprintTracker::new(b.id));

            let h = HealthState {
                broker_id: b.id,
                normalization: if initial_verified {
                    NormalizationState::Verified
                } else {
                    NormalizationState::Unverified
                },
                ..Default::default()
            };
            health_states.insert(b.id, h);

            move_detectors.insert(
                b.id,
                SignificantMidMoveDetector::new(
                    b.id,
                    b.point_size,
                    config.matcher.trigger_move_points,
                    config.matcher.event_cooldown_ms,
                    1,
                ),
            );
        }

        let active_pair = config.active_pair;
        let pair_tracker = PairDifferenceTracker::new(
            active_pair.0,
            active_pair.1,
            config.display.visible_seconds,
        )
        .with_max_points(config.display.visible_ticks);
        let matcher = OneToOneEventMatcher::new(
            active_pair.0,
            active_pair.1,
            config.matcher.matching_window_ms,
            config.matcher.ema_alpha,
            config.matcher.pending_event_capacity,
            1,
        );

        let consensus_calc = ConsensusCalculator::new(config.health.stale_after_ms);
        let burst_detector = MultiBrokerBurstDetector::new(config.matcher.matching_window_ms, 2);
        let hypothesis_engine = HypothesisEngine::default();
        let realtime_quote_history = VecDeque::with_capacity(config.display.visible_ticks);

        Self {
            config,
            channels,
            candle_book,
            mid_candle_book,
            spread_trackers,
            latest_quotes: HashMap::new(),
            fast_quotes: HashMap::new(),
            health_states,
            pair_tracker,
            move_detectors,
            matcher,
            latest_pair_match: None,
            active_pair,
            projection_revision: 0,
            current_watermark: MonoNs::ZERO,
            diagnostics: VecDeque::with_capacity(MAX_DIAGNOSTICS),
            consensus_calc,
            burst_detector,
            quote_persistence,
            repricing_persistence,
            fingerprint_trackers,
            hypothesis_engine,
            performance_diagnostics: None,
            processing_mono_ns: MonoNs::ZERO,
            realtime_quote_history,
        }
    }

    #[must_use]
    pub fn with_diagnostics(mut self, diagnostics: DiagnosticsHandle) -> Self {
        self.performance_diagnostics = Some(diagnostics);
        self
    }

    fn push_diagnostic(&mut self, diagnostic: Diagnostic) {
        if self.diagnostics.len() == MAX_DIAGNOSTICS {
            self.diagnostics.pop_front();
        }
        self.diagnostics.push_back(diagnostic);
    }

    #[must_use]
    pub fn latest_quote(&self, broker_id: BrokerId) -> Option<&Quote> {
        self.latest_quotes.get(&broker_id)
    }

    #[must_use]
    pub const fn current_watermark(&self) -> MonoNs {
        self.current_watermark
    }

    pub fn set_active_pair(&mut self, pair: (BrokerId, BrokerId)) {
        if pair.0 != pair.1
            && pair != self.active_pair
            && self.channels.contains_key(&pair.0)
            && self.channels.contains_key(&pair.1)
        {
            self.active_pair = pair;
            self.latest_pair_match = None;
            self.pair_tracker =
                PairDifferenceTracker::new(pair.0, pair.1, self.config.display.visible_seconds)
                    .with_max_points(self.config.display.visible_ticks);
            self.matcher = OneToOneEventMatcher::new(
                pair.0,
                pair.1,
                self.config.matcher.matching_window_ms,
                self.config.matcher.ema_alpha,
                self.config.matcher.pending_event_capacity,
                1,
            );
            self.projection_revision += 1;
        }
    }

    /// Reset volatile engine state (candles, trackers, matcher, latest quotes, quote path history)
    /// for instantaneous SEEK, rewind, and loop replay while preserving broker channel configurations.
    pub fn reset_state(&mut self) {
        for ch in self.channels.values_mut() {
            ch.watermark = MonoNs::ZERO;
            ch.pending_frames.clear();
            ch.pending_frame_bytes = 0;
            ch.ledger.reset();
            ch.session_id = None;
            ch.is_connected = true;
        }

        self.candle_book = CandleBook::with_retentions(&self.config.history.retentions);
        self.mid_candle_book = CandleBook::with_retentions(&self.config.history.retentions);

        self.spread_trackers.clear();
        self.quote_persistence.clear();
        self.repricing_persistence.clear();
        self.fingerprint_trackers.clear();
        self.move_detectors.clear();
        self.health_states.clear();
        self.latest_quotes.clear();
        self.fast_quotes.clear();

        for b in &self.config.brokers {
            self.spread_trackers.insert(b.id, SpreadTracker::new(b.id));
            self.quote_persistence
                .insert(b.id, QuotePersistenceTracker::new(b.id));
            self.repricing_persistence
                .insert(b.id, RepricingPersistenceTracker::new(b.id));
            self.fingerprint_trackers
                .insert(b.id, BrokerFingerprintTracker::new(b.id));

            let initial_verified = match b.timezone_rule {
                TimezoneRule::NyClose | TimezoneRule::Jst | TimezoneRule::Utc => true,
                TimezoneRule::Fixed => b.utc_verified,
            };
            self.health_states.insert(
                b.id,
                HealthState {
                    broker_id: b.id,
                    connection: ConnectionState::Connected,
                    data_freshness: FreshnessState::Live,
                    heartbeat: HeartbeatState::Ok,
                    normalization: if initial_verified {
                        NormalizationState::Verified
                    } else {
                        NormalizationState::Unverified
                    },
                    ..Default::default()
                },
            );

            self.move_detectors.insert(
                b.id,
                SignificantMidMoveDetector::new(
                    b.id,
                    b.point_size,
                    self.config.matcher.trigger_move_points,
                    self.config.matcher.event_cooldown_ms,
                    1,
                ),
            );
        }

        let (a, b) = self.active_pair;
        self.pair_tracker = PairDifferenceTracker::new(a, b, self.config.display.visible_seconds)
            .with_max_points(self.config.display.visible_ticks);

        self.matcher = OneToOneEventMatcher::new(
            a,
            b,
            self.config.matcher.matching_window_ms,
            self.config.matcher.ema_alpha,
            self.config.matcher.pending_event_capacity,
            1,
        );

        self.latest_pair_match = None;
        self.current_watermark = MonoNs::ZERO;
        self.diagnostics.clear();
        self.consensus_calc = ConsensusCalculator::new(self.config.health.stale_after_ms);
        self.burst_detector =
            MultiBrokerBurstDetector::new(self.config.matcher.matching_window_ms, 2);
        self.hypothesis_engine = HypothesisEngine::default();
        self.realtime_quote_history.clear();
        self.projection_revision += 1;
    }

    /// Update a broker's latest quote directly (used for 0-ms synchronized replay rate feeds).
    pub fn update_direct_quote(
        &mut self,
        broker_id: BrokerId,
        bid: f64,
        ask: f64,
        utc_ms: UtcMs,
        rx_mono_ns: MonoNs,
    ) {
        if !bid.is_finite() || !ask.is_finite() || bid <= 0.0 || ask < bid {
            return;
        }
        let spread = ask - bid;
        let quote = Quote {
            tick_id: TickId {
                broker_id,
                session_id: 1,
                sequence: 0,
            },
            bid,
            ask,
            mid: (bid + ask) / 2.0,
            spread,
            rx_mono_ns,
            utc_ms: Some(utc_ms),
            is_warmup: false,
            is_valid: true,
        };
        self.latest_quotes.insert(broker_id, quote);
        self.fast_quotes.insert(broker_id, quote);
        if let Some(ch) = self.channels.get_mut(&broker_id) {
            ch.is_connected = true;
            if rx_mono_ns > ch.watermark {
                ch.watermark = rx_mono_ns;
            }
        }
        if let Some(h) = self.health_states.get_mut(&broker_id) {
            h.connection = ConnectionState::Connected;
            h.data_freshness = FreshnessState::Live;
            h.last_live_tick_rx_mono = Some(rx_mono_ns);
            h.heartbeat = HeartbeatState::Ok;
        }
        if let Some(st) = self.spread_trackers.get_mut(&broker_id) {
            st.on_quote(spread, rx_mono_ns);
        }
        self.projection_revision += 1;
    }

    pub fn on_ingress_item(&mut self, item: IngressItem) {
        self.on_ingress_item_at(item, MonoNs::ZERO);
    }

    pub fn on_ingress_item_at(&mut self, item: IngressItem, processing_mono_ns: MonoNs) {
        self.processing_mono_ns = processing_mono_ns;
        let broker_id = item.broker_id();
        let mut close_diagnostic = None;
        let mut fast_quote = None;

        let is_disconnected = {
            let Some(ch) = self.channels.get_mut(&broker_id) else {
                return;
            };

            match item {
                IngressItem::Connected { generation, .. } => {
                    ch.is_connected = true;
                    ch.generation = generation;
                    self.latest_quotes.remove(&broker_id);
                    self.fast_quotes.remove(&broker_id);
                    if let Some(h) = self.health_states.get_mut(&broker_id) {
                        h.connection = ConnectionState::Connected;
                        h.data_freshness = FreshnessState::Unknown;
                        h.last_live_tick_rx_mono = None;
                        h.last_heartbeat_rx_mono = None;
                        h.heartbeat = HeartbeatState::Unknown;
                    }
                }
                IngressItem::Progress { watermark_ns, .. } => {
                    if watermark_ns > ch.watermark {
                        ch.watermark = watermark_ns;
                    }
                }
                IngressItem::Frame(rf) => {
                    if rf.rx_mono_ns > ch.watermark {
                        ch.watermark = rf.rx_mono_ns;
                    }
                    ch.pending_frame_bytes = ch.pending_frame_bytes.saturating_add(rf.wire_len());
                    fast_quote = Self::extract_fast_path_quote(broker_id, &rf);
                    ch.pending_frames.push_back(rf);
                }
                IngressItem::End {
                    generation, reason, ..
                } => {
                    if generation == 0 || ch.generation == 0 || ch.generation == generation {
                        ch.is_connected = false;
                        self.latest_quotes.remove(&broker_id);
                        self.fast_quotes.remove(&broker_id);
                        if let Some(h) = self.health_states.get_mut(&broker_id) {
                            h.connection = ConnectionState::Disconnected;
                        }
                        close_diagnostic = Some(Diagnostic {
                            code: "CONNECTION_CLOSED".to_string(),
                            severity: DiagnosticSeverity::Info,
                            broker_id,
                            session_id: ch.session_id,
                            mono_ns: ch.watermark,
                            sequence_range: None,
                            known_count: None,
                            detail_value: 0,
                            message: reason,
                        });
                    }
                }
            }
            !ch.is_connected
        };
        if let Some(diagnostic) = close_diagnostic {
            self.push_diagnostic(diagnostic);
        }

        // Fast Path: Immediately expose the latest quote from the arriving frame
        // without waiting for the global merge watermark. This allows individual
        // broker quotes and headers to update with 0ms cross-broker wait latency.
        if let Some((quote, rx_mono_ns, is_warmup)) = fast_quote {
            self.apply_fast_path_quote(broker_id, quote, rx_mono_ns, is_warmup);
        }

        self.force_drain_overflow(broker_id);
        self.drain_and_process_merge();
        if is_disconnected {
            self.latest_quotes.remove(&broker_id);
            self.fast_quotes.remove(&broker_id);
        }
    }

    fn calculate_global_watermark(&self) -> MonoNs {
        let mut min_wm = MonoNs(u64::MAX);
        let mut any_connected = false;

        for ch in self.channels.values() {
            if ch.is_connected {
                any_connected = true;
                if ch.watermark < min_wm {
                    min_wm = ch.watermark;
                }
            }
        }

        if any_connected {
            min_wm
        } else {
            // If no broker is connected, advance by max watermark available
            self.channels
                .values()
                .map(|c| c.watermark)
                .max()
                .unwrap_or(MonoNs::ZERO)
        }
    }

    fn drain_and_process_merge(&mut self) {
        let global_watermark = self.calculate_global_watermark();
        self.current_watermark = global_watermark;

        loop {
            // Find candidate frame across all brokers where rx_mono_ns <= global_watermark
            let mut best_broker = None;
            let mut best_key = (MonoNs(u64::MAX), 0u32, 0u64, 0u64);

            for (&bid, ch) in &self.channels {
                if let Some(front) = ch.pending_frames.front() {
                    let is_warmup = (front.frame.header.header_flags & HEADER_FLAG_WARMUP) != 0;
                    if is_warmup || front.rx_mono_ns <= global_watermark {
                        let key = (
                            if is_warmup {
                                MonoNs::ZERO
                            } else {
                                front.rx_mono_ns
                            },
                            bid,
                            front.connection_generation,
                            front.frame_index,
                        );
                        if key < best_key {
                            best_key = key;
                            best_broker = Some(bid);
                        }
                    }
                }
            }

            match best_broker {
                Some(bid) => {
                    let rf = self.pop_pending_frame(bid).unwrap();
                    self.process_frame(rf);
                }
                None => break,
            }
        }
    }

    fn extract_fast_path_quote(
        broker_id: BrokerId,
        rf: &ReceivedFrame,
    ) -> Option<(Quote, MonoNs, bool)> {
        if let FramePayload::TickBatch(ticks) = &rf.frame.payload {
            let is_warmup = (rf.frame.header.header_flags & HEADER_FLAG_WARMUP) != 0;
            if let Some(last_tick) = ticks.iter().rev().find(|t| {
                t.bid.is_finite()
                    && t.ask.is_finite()
                    && t.bid > 0.0
                    && t.ask > 0.0
                    && t.ask >= t.bid
            }) {
                let quote = Quote {
                    tick_id: TickId {
                        broker_id,
                        session_id: rf.frame.header.session_id,
                        sequence: last_tick.sequence,
                    },
                    bid: last_tick.bid,
                    ask: last_tick.ask,
                    mid: (last_tick.bid + last_tick.ask) / 2.0,
                    spread: last_tick.ask - last_tick.bid,
                    rx_mono_ns: rf.rx_mono_ns,
                    utc_ms: Some(UtcMs(last_tick.broker_time_msc)),
                    is_warmup,
                    is_valid: true,
                };
                return Some((quote, rf.rx_mono_ns, is_warmup));
            }
        }
        None
    }

    fn apply_fast_path_quote(
        &mut self,
        broker_id: BrokerId,
        quote: Quote,
        rx_mono_ns: MonoNs,
        is_warmup: bool,
    ) {
        let should_update = match self.fast_quotes.get(&broker_id) {
            Some(existing) => {
                quote.tick_id.session_id != existing.tick_id.session_id
                    || quote.tick_id.sequence >= existing.tick_id.sequence
                    || rx_mono_ns >= existing.rx_mono_ns
            }
            None => true,
        };

        if should_update {
            if let Some(st) = self.spread_trackers.get_mut(&broker_id) {
                st.on_quote(quote.spread, rx_mono_ns);
            }
            if let Some(h) = self.health_states.get_mut(&broker_id) {
                if !is_warmup {
                    h.last_live_tick_rx_mono = Some(rx_mono_ns);
                    h.data_freshness = FreshnessState::Live;
                }
            }
            self.fast_quotes.insert(broker_id, quote);
            self.projection_revision += 1;
        }
    }

    fn pop_pending_frame(&mut self, broker_id: BrokerId) -> Option<ReceivedFrame> {
        let channel = self.channels.get_mut(&broker_id)?;
        let frame = channel.pending_frames.pop_front()?;
        channel.pending_frame_bytes = channel.pending_frame_bytes.saturating_sub(frame.wire_len());
        Some(frame)
    }

    /// The merge watermark normally preserves a deterministic multi-feed
    /// ordering. If one feed is delayed long enough to exhaust its explicitly
    /// configured budget, preserving raw observations takes precedence over
    /// holding unbounded memory: process the oldest queued frame and expose
    /// the overload condition in health/diagnostics.
    fn force_drain_overflow(&mut self, broker_id: BrokerId) {
        let mut forced = 0_u64;
        loop {
            let over_capacity = self.channels.get(&broker_id).is_some_and(|channel| {
                channel.pending_frames.len() > channel.max_pending_frames
                    || channel.pending_frame_bytes > channel.max_pending_bytes
            });
            if !over_capacity {
                break;
            }
            let Some(frame) = self.pop_pending_frame(broker_id) else {
                break;
            };
            forced = forced.saturating_add(1);
            self.process_frame(frame);
        }
        if forced > 0 {
            if let Some(health) = self.health_states.get_mut(&broker_id) {
                health.overload.engine = true;
            }
            self.push_diagnostic(Diagnostic {
                code: "MERGE_BACKPRESSURE".to_string(),
                severity: DiagnosticSeverity::Warn,
                broker_id,
                session_id: self
                    .channels
                    .get(&broker_id)
                    .and_then(|channel| channel.session_id),
                mono_ns: self.current_watermark,
                sequence_range: None,
                known_count: Some(forced),
                detail_value: 0,
                message:
                    "Merge wait budget exhausted; processed queued frames out of watermark order"
                        .to_string(),
            });
        }
    }

    fn process_frame(&mut self, rf: ReceivedFrame) {
        let frame_process_start = self
            .performance_diagnostics
            .as_ref()
            .map(|_| Instant::now());
        let broker_id = rf.frame.header.broker_id;
        let mut sequence_gaps = Vec::new();

        // 1. Process payload according to message type
        match &rf.frame.payload {
            FramePayload::TickBatch(ticks) => {
                let ch = self.channels.get_mut(&broker_id).unwrap();
                if ch.session_id != Some(rf.frame.header.session_id) {
                    ch.ledger.reset();
                }
                ch.session_id = Some(rf.frame.header.session_id);

                let is_batch_warmup = (rf.frame.header.header_flags & HEADER_FLAG_WARMUP) != 0;

                // For NyClose, check if a DST calendar transition occurred
                if ch.timezone_rule == TimezoneRule::NyClose {
                    if let Some(unix_ns) = rf.rx_unix_ns {
                        let sec = unix_ns / 1_000_000_000;
                        if sec > 0 {
                            let expected = ch
                                .timezone_rule
                                .resolve_offset(sec, ch.active_utc_offset_sec);
                            if expected != ch.active_utc_offset_sec {
                                log::info!(
                                    "Broker {} ({}) NYClose DST calendar transition: {}s -> {}s ({:+}h)",
                                    broker_id,
                                    broker_name(&self.config.brokers, broker_id),
                                    ch.active_utc_offset_sec,
                                    expected,
                                    expected / 3600
                                );
                                ch.active_utc_offset_sec = expected;
                                self.candle_book.clear_broker(broker_id);
                                self.mid_candle_book.clear_broker(broker_id);
                            }
                        }
                    }
                } else if ch.auto_utc_offset
                    && ch.timezone_rule == TimezoneRule::Fixed
                    && !is_batch_warmup
                {
                    if let Some(unix_ns) = rf.rx_unix_ns {
                        if let Some(last_tick) = ticks.last() {
                            let pc_sec = (unix_ns / 1_000_000_000) as f64;
                            let broker_sec = (last_tick.broker_time_msc as f64) / 1000.0;
                            let raw_diff = broker_sec - pc_sec;
                            let detected_offset = round_to_hourly_offset(raw_diff);
                            if ch.active_utc_offset_sec != detected_offset || !ch.utc_verified {
                                log::info!(
                                    "Broker {} ({}) UTC offset auto-detected from ticks: {}s ({:+}h, previous: {}s)",
                                    broker_id,
                                    broker_name(&self.config.brokers, broker_id),
                                    detected_offset,
                                    detected_offset / 3600,
                                    ch.active_utc_offset_sec
                                );
                                ch.active_utc_offset_sec = detected_offset;
                                ch.utc_verified = true;
                                // Purge any slots that were created with the previous/unverified offset
                                self.candle_book.clear_broker(broker_id);
                                self.mid_candle_book.clear_broker(broker_id);
                            }
                        }
                    }
                }

                let utc_offset = ch.active_utc_offset_sec;
                let utc_verified = ch.utc_verified;

                for tick in ticks {
                    let (disp, gap) = ch.ledger.observe(tick.sequence);
                    if let Some(gap_range) = gap {
                        sequence_gaps.push(gap_range);
                    }
                    if disp == SequenceDisposition::New {
                        let is_warmup = (rf.frame.header.header_flags & HEADER_FLAG_WARMUP) != 0;
                        let obs = ObservedTick {
                            tick_id: TickId {
                                broker_id,
                                session_id: rf.frame.header.session_id,
                                sequence: tick.sequence,
                            },
                            record: *tick,
                            rx_mono_ns: rf.rx_mono_ns,
                            rx_unix_ns: rf.rx_unix_ns,
                            connection_generation: rf.connection_generation,
                            frame_index: rf.frame_index,
                            is_warmup,
                            segment_id: 1,
                            disposition: disp,
                        };

                        let quote = Quote {
                            tick_id: obs.tick_id,
                            bid: tick.bid,
                            ask: tick.ask,
                            mid: (tick.bid + tick.ask) / 2.0,
                            spread: tick.ask - tick.bid,
                            rx_mono_ns: rf.rx_mono_ns,
                            utc_ms: Some(UtcMs(tick.broker_time_msc)),
                            is_warmup,
                            is_valid: tick.bid.is_finite()
                                && tick.ask.is_finite()
                                && tick.bid > 0.0
                                && tick.ask > 0.0
                                && tick.ask >= tick.bid,
                        };

                        // Update spread tracker
                        if quote.is_valid {
                            if let Some(st) = self.spread_trackers.get_mut(&broker_id) {
                                st.on_quote(quote.spread, rf.rx_mono_ns);
                            }
                        }

                        // Feed CandleBook if valid
                        if quote.is_valid {
                            if let Ok(norm) = normalize_tick(&obs, utc_offset, utc_verified, 1) {
                                let rx_utc_now =
                                    UtcMs(rf.rx_unix_ns.map_or(norm.utc_ms.0, |ns| ns / 1_000_000));
                                self.candle_book.on_tick(&norm, PriceMode::Bid, rx_utc_now);
                                self.mid_candle_book
                                    .on_tick(&norm, PriceMode::Mid, rx_utc_now);
                            }
                        }

                        // Update quote persistence tracker
                        if let Some(qp) = self.quote_persistence.get_mut(&broker_id) {
                            qp.on_quote(&quote);
                        }

                        // Update fingerprint tracker tick count
                        if let Some(ft) = self.fingerprint_trackers.get_mut(&broker_id) {
                            ft.record_tick(
                                false,
                                false,
                                !is_warmup && quote.is_valid,
                                rf.rx_mono_ns,
                            );
                        }

                        // Store latest quote in chronological merge order
                        if quote.is_valid {
                            self.latest_quotes.insert(broker_id, quote);
                            if self
                                .fast_quotes
                                .get(&broker_id)
                                .is_none_or(|fq| fq.rx_mono_ns <= rf.rx_mono_ns)
                            {
                                self.fast_quotes.insert(broker_id, quote);
                            }
                        }

                        // Evaluate move detectors for ALL brokers to drive multi-broker bursts and fingerprints
                        if quote.is_valid && !quote.is_warmup {
                            let move_event = self
                                .move_detectors
                                .get_mut(&broker_id)
                                .and_then(|md| md.on_quote(&quote));
                            if let Some(move_ev) = move_event {
                                let total_b = self.config.brokers.len();
                                let fresh_c = self
                                    .latest_quotes
                                    .values()
                                    .filter(|q| {
                                        q.is_valid
                                            && !q.is_warmup
                                            && q.mid.is_finite()
                                            && q.rx_mono_ns <= rf.rx_mono_ns
                                            && (rf.rx_mono_ns.0 - q.rx_mono_ns.0)
                                                <= self
                                                    .config
                                                    .health
                                                    .stale_after_ms
                                                    .saturating_mul(1_000_000)
                                    })
                                    .count();

                                let cluster_opt =
                                    self.burst_detector.on_event(move_ev, total_b, fresh_c);
                                if let Some(cluster) = cluster_opt {
                                    if let Some(ft) =
                                        self.fingerprint_trackers.get_mut(&cluster.first_observed)
                                    {
                                        ft.record_lead();
                                    }
                                    for &b in &cluster.participating_brokers {
                                        if b != cluster.first_observed {
                                            if let Some(ft) = self.fingerprint_trackers.get_mut(&b)
                                            {
                                                ft.record_follow(cluster.observed_span_ms);
                                            }
                                        }
                                    }
                                }

                                let (a, b) = self.active_pair;
                                if broker_id == a || broker_id == b {
                                    if let Some(pair_match) = self.matcher.on_event(move_ev) {
                                        self.latest_pair_match = Some(pair_match);
                                    }
                                }
                            }
                        }

                        // If broker is part of active pair, update price diff series
                        let (a, b) = self.active_pair;
                        if (broker_id == a || broker_id == b) && quote.is_valid && !quote.is_warmup
                        {
                            let fresh = |id| {
                                self.latest_quotes.get(&id).filter(|q| {
                                    q.is_valid
                                        && !q.is_warmup
                                        && q.rx_mono_ns <= rf.rx_mono_ns
                                        && (rf.rx_mono_ns.0 - q.rx_mono_ns.0)
                                            <= self
                                                .config
                                                .health
                                                .stale_after_ms
                                                .saturating_mul(1_000_000)
                                })
                            };
                            let q_a = fresh(a);
                            let q_b = fresh(b);
                            let synchronized = q_a.zip(q_b).filter(|(qa, qb)| {
                                qa.rx_mono_ns.0.abs_diff(qb.rx_mono_ns.0)
                                    <= self
                                        .config
                                        .matcher
                                        .max_quote_skew_ms
                                        .saturating_mul(1_000_000)
                            });
                            self.pair_tracker.compute_and_record(
                                synchronized.map(|(qa, _)| qa),
                                synchronized.map(|(_, qb)| qb),
                                rf.rx_mono_ns,
                            );
                        }

                        // Append point to Realtime Quote Path history for all active brokers
                        let tick_mono_ns = if is_warmup {
                            let tick_utc_ms = tick.broker_time_msc - (i64::from(utc_offset) * 1000);
                            if let Some(unix_ns) = rf.rx_unix_ns {
                                let now_utc_ms = unix_ns / 1_000_000;
                                let age_ms = (now_utc_ms - tick_utc_ms).max(0) as u64;
                                MonoNs(
                                    rf.rx_mono_ns
                                        .0
                                        .saturating_sub(age_ms.saturating_mul(1_000_000)),
                                )
                            } else {
                                rf.rx_mono_ns
                            }
                        } else {
                            rf.rx_mono_ns
                        };

                        let mut mids = HashMap::with_capacity(self.latest_quotes.len());
                        if quote.is_valid && quote.mid.is_finite() {
                            mids.insert(broker_id, quote.mid);
                        }
                        for (&bid, q) in &self.latest_quotes {
                            if bid != broker_id && q.is_valid && q.mid.is_finite() {
                                if is_warmup {
                                    mids.insert(bid, q.mid);
                                } else if !q.is_warmup
                                    && q.rx_mono_ns <= rf.rx_mono_ns
                                    && (rf.rx_mono_ns.0 - q.rx_mono_ns.0)
                                        <= self
                                            .config
                                            .health
                                            .stale_after_ms
                                            .saturating_mul(1_000_000)
                                {
                                    mids.insert(bid, q.mid);
                                }
                            }
                        }

                        let consensus_mid = compute_median_from_mids(&mids);

                        let pt = RealtimeQuotePoint {
                            mono_ns: tick_mono_ns,
                            broker_mids: mids,
                            consensus_mid,
                        };

                        if self.realtime_quote_history.is_empty()
                            || tick_mono_ns >= self.realtime_quote_history.back().unwrap().mono_ns
                        {
                            self.realtime_quote_history.push_back(pt);
                        } else {
                            let idx = self
                                .realtime_quote_history
                                .partition_point(|p| p.mono_ns < tick_mono_ns);
                            let merged = if idx < self.realtime_quote_history.len()
                                && self.realtime_quote_history[idx]
                                    .mono_ns
                                    .0
                                    .saturating_sub(tick_mono_ns.0)
                                    <= 20_000_000
                            {
                                let p = &mut self.realtime_quote_history[idx];
                                p.broker_mids.insert(broker_id, quote.mid);
                                p.consensus_mid = compute_median_from_mids(&p.broker_mids);
                                true
                            } else if idx > 0
                                && tick_mono_ns
                                    .0
                                    .saturating_sub(self.realtime_quote_history[idx - 1].mono_ns.0)
                                    <= 20_000_000
                            {
                                let p = &mut self.realtime_quote_history[idx - 1];
                                p.broker_mids.insert(broker_id, quote.mid);
                                p.consensus_mid = compute_median_from_mids(&p.broker_mids);
                                true
                            } else {
                                false
                            };
                            if !merged {
                                self.realtime_quote_history.insert(idx, pt);
                            }
                        }

                        while self.realtime_quote_history.len() > self.config.display.visible_ticks
                        {
                            self.realtime_quote_history.pop_front();
                        }

                        // Update health
                        if let Some(h) = self.health_states.get_mut(&broker_id) {
                            h.total_ticks_received += 1;
                            if !is_warmup && quote.is_valid {
                                h.last_live_tick_rx_mono = Some(rf.rx_mono_ns);
                                h.data_freshness = FreshnessState::Live;
                            }
                        }
                    }
                }
                if !sequence_gaps.is_empty() {
                    if let Some(health) = self.health_states.get_mut(&broker_id) {
                        health.integrity = IntegrityState::Gap;
                    }
                    for (first, last) in sequence_gaps.drain(..) {
                        self.push_diagnostic(Diagnostic {
                            code: "SEQUENCE_GAP".to_string(),
                            severity: DiagnosticSeverity::Warn,
                            broker_id,
                            session_id: Some(rf.frame.header.session_id),
                            mono_ns: rf.rx_mono_ns,
                            sequence_range: Some((first, last)),
                            known_count: Some(last.saturating_sub(first).saturating_add(1)),
                            detail_value: 0,
                            message: "One or more source tick sequences were not observed"
                                .to_string(),
                        });
                    }
                }
            }
            FramePayload::Heartbeat(hb) => {
                let ch = self.channels.get_mut(&broker_id).unwrap();
                if ch.session_id != Some(hb.session_id) {
                    ch.ledger.reset();
                }
                ch.session_id = Some(hb.session_id);
                if let Some(h) = self.health_states.get_mut(&broker_id) {
                    h.last_heartbeat_rx_mono = Some(rf.rx_mono_ns);
                    h.heartbeat = HeartbeatState::Ok;
                }

                if ch.timezone_rule == TimezoneRule::NyClose {
                    // Passive diagnostic: warn if heartbeat offset sample diverges significantly from NYClose
                    if (rf.frame.header.header_flags & HB_FLAG_HAS_OFFSET_SAMPLE != 0)
                        && (-43200..=50400).contains(&hb.server_utc_offset_sec)
                    {
                        let sample = hb.server_utc_offset_sec;
                        let sample_rounded = round_to_hourly_offset(f64::from(sample));
                        if (sample_rounded - ch.active_utc_offset_sec).abs() >= 7200 {
                            log::warn!(
                                "Broker {} ({}) heartbeat offset sample ({}s) diverges from expected NYClose ({}s). Check server settings.",
                                broker_id,
                                broker_name(&self.config.brokers, broker_id),
                                sample,
                                ch.active_utc_offset_sec
                            );
                        }
                    }
                } else if ch.auto_utc_offset
                    && ch.timezone_rule == TimezoneRule::Fixed
                    && (rf.frame.header.header_flags & HB_FLAG_HAS_OFFSET_SAMPLE != 0
                        || hb.server_utc_offset_sec != 0)
                {
                    let sample = hb.server_utc_offset_sec;
                    // Sanity check: valid FX timezone offset is between -12h (-43200s) and +14h (+50400s)
                    if (-43200..=50400).contains(&sample) {
                        let detected_offset = round_to_hourly_offset(f64::from(sample));
                        if ch.active_utc_offset_sec != detected_offset || !ch.utc_verified {
                            log::info!(
                                "Broker {} ({}) UTC offset auto-detected from heartbeat: {}s ({:+}h, previous: {}s)",
                                broker_id,
                                broker_name(&self.config.brokers, broker_id),
                                detected_offset,
                                detected_offset / 3600,
                                ch.active_utc_offset_sec
                            );
                            ch.active_utc_offset_sec = detected_offset;
                            ch.utc_verified = true;
                            self.candle_book.clear_broker(broker_id);
                            self.mid_candle_book.clear_broker(broker_id);
                        }
                    }
                }
            }
            FramePayload::Status(st) => {
                if let Some(h) = self.health_states.get_mut(&broker_id) {
                    h.phase = if st.phase == PHASE_WARMING {
                        PhaseState::Warming
                    } else {
                        PhaseState::Live
                    };
                }
            }
            FramePayload::BatchAck(_) => {}
        }

        if let Some(h) = self.health_states.get_mut(&broker_id) {
            h.total_frames_received += 1;
        }

        self.projection_revision += 1;

        let frame_processing_elapsed = frame_process_start.map(|start| start.elapsed());
        if let Some(diagnostics) = &self.performance_diagnostics {
            let rx_mono_ns = rf.rx_mono_ns;
            let processing_mono_ns = self.processing_mono_ns;
            let is_tick_batch = matches!(&rf.frame.payload, FramePayload::TickBatch(_));
            if is_tick_batch && processing_mono_ns.0 >= rx_mono_ns.0 && processing_mono_ns.0 > 0 {
                diagnostics.record_ns(
                    DiagnosticStage::IngressToEngine,
                    processing_mono_ns.saturating_sub(rx_mono_ns).0,
                );
            }
            if let Some(elapsed) = frame_processing_elapsed {
                diagnostics.record_duration(DiagnosticStage::EngineFrameProcessing, elapsed);
            }
        }
    }
}

/// Helper function to calculate median mid price across active brokers
#[must_use]
pub fn compute_median_from_mids(mids: &HashMap<BrokerId, f64>) -> Option<f64> {
    if mids.is_empty() {
        return None;
    }
    let mut vals: Vec<f64> = mids.values().copied().filter(|v| v.is_finite()).collect();
    if vals.is_empty() {
        return None;
    }
    vals.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = vals.len();
    if n % 2 == 1 {
        Some(vals[n / 2])
    } else {
        Some((vals[n / 2 - 1] + vals[n / 2]) / 2.0)
    }
}

#[cfg(feature = "replay")]
impl TickEngine {
    /// Reconstructs the Realtime Quote Path history for all brokers across the full screen width
    /// (e.g. `visible_seconds` window / `visible_ticks` count) upon startup or seek operations.
    pub fn rebuild_quote_history_from_replay_ticks(
        &mut self,
        warmup_ticks: &[crate::replay::ReplayTick],
        target_utc_ms: i64,
    ) {
        if warmup_ticks.is_empty() {
            return;
        }

        let visible_sec = self.config.display.visible_seconds.max(60);
        let cutoff_utc_ms = target_utc_ms.saturating_sub((visible_sec as i64 + 10) * 1000);
        let max_ticks = self.config.display.visible_ticks.max(1200);

        // 1. Identify latest mid for each broker prior to cutoff_utc_ms so broker lines start fully connected
        let mut current_mids: HashMap<BrokerId, f64> = HashMap::new();
        let mut split_idx = 0;
        for (i, t) in warmup_ticks.iter().enumerate() {
            let eff_utc = t.effective_utc_ms();
            if eff_utc >= cutoff_utc_ms && (warmup_ticks.len() - i) <= max_ticks {
                split_idx = i;
                break;
            }
            if t.bid.is_finite() && t.ask.is_finite() && t.bid > 0.0 && t.ask >= t.bid {
                current_mids.insert(t.broker_id, (t.bid + t.ask) / 2.0);
            }
        }

        // 2. Clear transient quote history and build chronologically interleaved points
        let recent_ticks = &warmup_ticks[split_idx..];
        self.realtime_quote_history.clear();

        let mut last_mono_ns = MonoNs::ZERO;
        for t in recent_ticks {
            if t.bid.is_finite() && t.ask.is_finite() && t.bid > 0.0 && t.ask >= t.bid {
                let mid = (t.bid + t.ask) / 2.0;
                current_mids.insert(t.broker_id, mid);

                let eff_utc = t.effective_utc_ms();
                // Ensure strictly monotonic non-decreasing timestamp invariant across all brokers
                let mono_ns =
                    MonoNs((eff_utc.max(0) as u64).saturating_mul(1_000_000)).max(last_mono_ns);
                last_mono_ns = mono_ns;
                let consensus_mid = compute_median_from_mids(&current_mids);

                self.realtime_quote_history.push_back(RealtimeQuotePoint {
                    mono_ns,
                    broker_mids: current_mids.clone(),
                    consensus_mid,
                });

                while self.realtime_quote_history.len() > max_ticks {
                    self.realtime_quote_history.pop_front();
                }
            }
        }

        // 3. Anchor latest quotes up to target_mono so chart lines extend right to the seek point
        let target_mono = MonoNs((target_utc_ms.max(0) as u64).saturating_mul(1_000_000));
        if !current_mids.is_empty() && last_mono_ns < target_mono {
            let consensus_mid = compute_median_from_mids(&current_mids);
            self.realtime_quote_history.push_back(RealtimeQuotePoint {
                mono_ns: target_mono,
                broker_mids: current_mids.clone(),
                consensus_mid,
            });
            while self.realtime_quote_history.len() > max_ticks {
                self.realtime_quote_history.pop_front();
            }
        }
    }
}
