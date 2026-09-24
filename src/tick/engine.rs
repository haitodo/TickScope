//! Tick Engine: N-broker watermark merge, ledger, and pipeline coordination.
//! Reference: docs/blueprint/architecture.md and docs/blueprint/decisions.md

use crate::contracts::config::AppConfig;
use crate::contracts::models::*;
use crate::contracts::types::*;
use crate::metrics::burst::MultiBrokerBurstDetector;
use crate::metrics::consensus::ConsensusCalculator;
use crate::metrics::fingerprint::{BrokerFingerprintTracker, QuotePersistenceTracker, RepricingPersistenceTracker};
use crate::metrics::hypothesis::HypothesisEngine;
use crate::metrics::latency::LatencyMetrics;
use crate::metrics::lead_lag::SignificantMidMoveDetector;
use crate::metrics::price_diff::PairDifferenceTracker;
use crate::metrics::spread::SpreadTracker;
use crate::tick::candle::CandleBook;
use crate::tick::matcher::OneToOneEventMatcher;
use crate::tick::normalize::{normalize_tick, round_to_hourly_offset};
use std::collections::{HashMap, HashSet, VecDeque};

const MAX_DIAGNOSTICS: usize = 2_048;

struct BrokerChannelState {
    is_connected: bool,
    generation: u64,
    watermark: MonoNs,
    pending_frames: VecDeque<ReceivedFrame>,
    pending_frame_bytes: usize,
    max_pending_frames: usize,
    max_pending_bytes: usize,
    expected_sequence: Sequence,
    session_id: Option<SessionId>,
    seen_sequences: HashSet<Sequence>,
    recent_sequences: VecDeque<Sequence>,
    sequence_ledger_capacity: usize,
    auto_utc_offset: bool,
    active_utc_offset_sec: i32,
    utc_verified: bool,
}

pub struct TickEngine {
    config: AppConfig,
    channels: HashMap<BrokerId, BrokerChannelState>,
    candle_book: CandleBook,
    spread_trackers: HashMap<BrokerId, SpreadTracker>,
    latest_quotes: HashMap<BrokerId, Quote>,
    health_states: HashMap<BrokerId, HealthState>,
    pair_tracker: PairDifferenceTracker,
    move_detectors: HashMap<BrokerId, SignificantMidMoveDetector>,
    matcher: OneToOneEventMatcher,
    latest_pair_match: Option<LeadLagMatch>,
    active_pair: (BrokerId, BrokerId),
    projection_revision: u64,
    current_watermark: MonoNs,
    diagnostics: VecDeque<Diagnostic>,

    // Multi-Broker, Microstructure & Hypothesis additions (RFC Beta 0.3)
    pub consensus_calc: ConsensusCalculator,
    pub burst_detector: MultiBrokerBurstDetector,
    pub quote_persistence: HashMap<BrokerId, QuotePersistenceTracker>,
    pub repricing_persistence: HashMap<BrokerId, RepricingPersistenceTracker>,
    pub fingerprint_trackers: HashMap<BrokerId, BrokerFingerprintTracker>,
    pub hypothesis_engine: HypothesisEngine,
    pub latency_metrics: LatencyMetrics,
    pub realtime_quote_history: VecDeque<RealtimeQuotePoint>,
}

impl TickEngine {
    pub fn new(config: AppConfig) -> Self {
        let mut channels = HashMap::new();
        let mut spread_trackers = HashMap::new();
        let mut health_states = HashMap::new();
        let mut move_detectors = HashMap::new();

        let candle_book = CandleBook::with_retentions(&config.history.retentions);

        let mut quote_persistence = HashMap::new();
        let mut repricing_persistence = HashMap::new();
        let mut fingerprint_trackers = HashMap::new();

        for b in &config.brokers {
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
                    expected_sequence: 0,
                    session_id: None,
                    seen_sequences: HashSet::new(),
                    recent_sequences: VecDeque::with_capacity(config.history.ledger_capacity),
                    sequence_ledger_capacity: config.history.ledger_capacity,
                    auto_utc_offset: b.auto_utc_offset,
                    active_utc_offset_sec: b.utc_offset_sec,
                    utc_verified: b.utc_verified,
                },
            );

            spread_trackers.insert(b.id, SpreadTracker::new(b.id));
            quote_persistence.insert(b.id, QuotePersistenceTracker::new(b.id));
            repricing_persistence.insert(b.id, RepricingPersistenceTracker::new(b.id));
            fingerprint_trackers.insert(b.id, BrokerFingerprintTracker::new(b.id));

            let h = HealthState {
                broker_id: b.id,
                normalization: if b.utc_verified {
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
        ).with_max_points(config.display.visible_ticks);
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
        let latency_metrics = LatencyMetrics::default();
        let realtime_quote_history = VecDeque::with_capacity(config.display.visible_ticks);

        Self {
            config,
            channels,
            candle_book,
            spread_trackers,
            latest_quotes: HashMap::new(),
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
            latency_metrics,
            realtime_quote_history,
        }
    }

    fn push_diagnostic(&mut self, diagnostic: Diagnostic) {
        if self.diagnostics.len() == MAX_DIAGNOSTICS {
            self.diagnostics.pop_front();
        }
        self.diagnostics.push_back(diagnostic);
    }

    pub fn set_active_pair(&mut self, pair: (BrokerId, BrokerId)) {
        if pair.0 != pair.1 && pair != self.active_pair
            && self.channels.contains_key(&pair.0) && self.channels.contains_key(&pair.1) {
            self.active_pair = pair;
            self.latest_pair_match = None;
            self.pair_tracker = PairDifferenceTracker::new(
                pair.0,
                pair.1,
                self.config.display.visible_seconds,
            ).with_max_points(self.config.display.visible_ticks);
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

    pub fn on_ingress_item(&mut self, item: IngressItem) {
        let broker_id = item.broker_id();
        let mut close_diagnostic = None;
        let is_disconnected = {
            let ch = match self.channels.get_mut(&broker_id) {
                Some(c) => c,
                None => return,
            };

            match item {
                IngressItem::Connected { generation, .. } => {
                    ch.is_connected = true;
                    ch.generation = generation;
                    self.latest_quotes.remove(&broker_id);
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
                    ch.pending_frame_bytes = ch.pending_frame_bytes
                        .saturating_add(rf.raw_wire_bytes.len());
                    ch.pending_frames.push_back(rf);
                }
                IngressItem::End { generation, reason, .. } => {
                    if ch.generation == generation {
                        ch.is_connected = false;
                        self.latest_quotes.remove(&broker_id);
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

        self.force_drain_overflow(broker_id);
        self.drain_and_process_merge();
        if is_disconnected {
            self.latest_quotes.remove(&broker_id);
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
            self.channels.values().map(|c| c.watermark).max().unwrap_or(MonoNs::ZERO)
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
                    if front.rx_mono_ns <= global_watermark {
                        let key = (front.rx_mono_ns, bid, front.connection_generation, front.frame_index);
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

    fn pop_pending_frame(&mut self, broker_id: BrokerId) -> Option<ReceivedFrame> {
        let channel = self.channels.get_mut(&broker_id)?;
        let frame = channel.pending_frames.pop_front()?;
        channel.pending_frame_bytes = channel
            .pending_frame_bytes
            .saturating_sub(frame.raw_wire_bytes.len());
        Some(frame)
    }

    /// The merge watermark normally preserves a deterministic multi-feed
    /// ordering. If one feed is delayed long enough to exhaust its explicitly
    /// configured budget, preserving raw observations takes precedence over
    /// holding unbounded memory: process the oldest durable frame and expose
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
                session_id: self.channels.get(&broker_id).and_then(|channel| channel.session_id),
                mono_ns: self.current_watermark,
                sequence_range: None,
                known_count: Some(forced),
                detail_value: 0,
                message: "Merge wait budget exhausted; processed durable frames out of watermark order".to_string(),
            });
        }
    }

    fn process_frame(&mut self, rf: ReceivedFrame) {
        let broker_id = rf.frame.header.broker_id;
        let mut sequence_gaps = Vec::new();

        // 1. Process payload according to message type
        match &rf.frame.payload {
            FramePayload::TickBatch(ticks) => {
                let ch = self.channels.get_mut(&broker_id).unwrap();
                if ch.session_id != Some(rf.frame.header.session_id) {
                    ch.expected_sequence = 0;
                    ch.seen_sequences.clear();
                    ch.recent_sequences.clear();
                }
                ch.session_id = Some(rf.frame.header.session_id);

                // Auto-detection of UTC offset from ticks if enabled
                if ch.auto_utc_offset {
                    if let Some(unix_ns) = rf.rx_unix_ns {
                        if let Some(first_tick) = ticks.first() {
                            let pc_sec = (unix_ns / 1_000_000_000) as f64;
                            let broker_sec = (first_tick.broker_time_msc as f64) / 1000.0;
                            let raw_diff = broker_sec - pc_sec;
                            ch.active_utc_offset_sec = round_to_hourly_offset(raw_diff);
                            ch.utc_verified = true;
                        }
                    }
                }

                let utc_offset = ch.active_utc_offset_sec;
                let utc_verified = ch.utc_verified;

                for tick in ticks {
                    // Sequence ledger
                    let disp = if ch.seen_sequences.contains(&tick.sequence) {
                        SequenceDisposition::DuplicateExact
                    } else if tick.sequence < ch.expected_sequence {
                        SequenceDisposition::OutOfOrderUnverified
                    } else if tick.sequence > ch.expected_sequence {
                        // Gap!
                        sequence_gaps.push((ch.expected_sequence, tick.sequence.saturating_sub(1)));
                        ch.seen_sequences.insert(tick.sequence);
                        ch.recent_sequences.push_back(tick.sequence);
                        ch.expected_sequence = tick.sequence.saturating_add(1);
                        SequenceDisposition::New
                    } else {
                        ch.seen_sequences.insert(tick.sequence);
                        ch.recent_sequences.push_back(tick.sequence);
                        ch.expected_sequence = tick.sequence.saturating_add(1);
                        SequenceDisposition::New
                    };
                    while ch.recent_sequences.len() > ch.sequence_ledger_capacity {
                        if let Some(expired) = ch.recent_sequences.pop_front() {
                            ch.seen_sequences.remove(&expired);
                        }
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
                            is_valid: tick.bid.is_finite() && tick.ask.is_finite()
                                && tick.bid > 0.0 && tick.ask > 0.0 && tick.ask >= tick.bid,
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
                                self.candle_book.on_tick(&norm, PriceMode::Bid, norm.utc_ms);
                            }
                        }

                        // Update quote persistence tracker
                        if let Some(qp) = self.quote_persistence.get_mut(&broker_id) {
                            qp.on_quote(&quote);
                        }

                        // Update fingerprint tracker tick count
                        if let Some(ft) = self.fingerprint_trackers.get_mut(&broker_id) {
                            ft.record_tick(false, false, !is_warmup && quote.is_valid, rf.rx_mono_ns);
                        }

                        // Store latest quote
                        if quote.is_valid {
                            self.latest_quotes.insert(broker_id, quote.clone());
                        }

                        // Evaluate move detectors for ALL brokers to drive multi-broker bursts and fingerprints
                        if quote.is_valid && !quote.is_warmup {
                            let move_event = self.move_detectors.get_mut(&broker_id)
                                .and_then(|md| md.on_quote(&quote));
                            if let Some(move_ev) = move_event {
                                let total_b = self.config.brokers.len();
                                let fresh_c = self.latest_quotes.values().filter(|q| {
                                    q.is_valid && !q.is_warmup && q.mid.is_finite()
                                        && rf.rx_mono_ns.0.saturating_sub(q.rx_mono_ns.0)
                                            <= self.config.health.stale_after_ms.saturating_mul(1_000_000)
                                }).count();

                                let cluster_opt = self.burst_detector.on_event(move_ev.clone(), total_b, fresh_c);
                                if let Some(cluster) = cluster_opt {
                                    if let Some(ft) = self.fingerprint_trackers.get_mut(&cluster.first_observed) {
                                        ft.record_lead();
                                    }
                                    for &b in &cluster.participating_brokers {
                                        if b != cluster.first_observed {
                                            if let Some(ft) = self.fingerprint_trackers.get_mut(&b) {
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
                        if (broker_id == a || broker_id == b) && quote.is_valid && !quote.is_warmup {
                            let fresh = |id| self.latest_quotes.get(&id).filter(|q| {
                                q.is_valid && !q.is_warmup
                                    && rf.rx_mono_ns.0.saturating_sub(q.rx_mono_ns.0)
                                        <= self.config.health.stale_after_ms.saturating_mul(1_000_000)
                            });
                            let q_a = fresh(a);
                            let q_b = fresh(b);
                            let synchronized = q_a.zip(q_b).filter(|(qa, qb)| {
                                qa.rx_mono_ns.0.abs_diff(qb.rx_mono_ns.0)
                                    <= self.config.matcher.max_quote_skew_ms.saturating_mul(1_000_000)
                            });
                            self.pair_tracker.compute_and_record(
                                synchronized.map(|(qa, _)| qa),
                                synchronized.map(|(_, qb)| qb),
                                rf.rx_mono_ns,
                            );
                        }

                        // Append point to Realtime Quote Path history for all active brokers
                        let mut mids = HashMap::new();
                        for (&bid, q) in &self.latest_quotes {
                            if q.is_valid && !q.is_warmup && q.mid.is_finite()
                                && rf.rx_mono_ns.0.saturating_sub(q.rx_mono_ns.0)
                                    <= self.config.health.stale_after_ms.saturating_mul(1_000_000)
                            {
                                mids.insert(bid, q.mid);
                            }
                        }
                        let consensus = self.consensus_calc.compute(self.latest_quotes.values(), rf.rx_mono_ns);
                        self.realtime_quote_history.push_back(RealtimeQuotePoint {
                            mono_ns: rf.rx_mono_ns,
                            broker_mids: mids,
                            consensus_mid: consensus.consensus_mid,
                        });
                        while self.realtime_quote_history.len() > self.config.display.visible_ticks {
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
                            message: "One or more source tick sequences were not observed".to_string(),
                        });
                    }
                }
            }
            FramePayload::Heartbeat(hb) => {
                let ch = self.channels.get_mut(&broker_id).unwrap();
                if ch.session_id != Some(hb.session_id) {
                    ch.expected_sequence = 0;
                    ch.seen_sequences.clear();
                    ch.recent_sequences.clear();
                }
                ch.session_id = Some(hb.session_id);
                if let Some(h) = self.health_states.get_mut(&broker_id) {
                    h.last_heartbeat_rx_mono = Some(rf.rx_mono_ns);
                    h.heartbeat = HeartbeatState::Ok;
                }

                // Auto-detection from Heartbeat offset sample if enabled
                if ch.auto_utc_offset
                    && (rf.frame.header.header_flags & HB_FLAG_HAS_OFFSET_SAMPLE != 0
                        || hb.server_utc_offset_sec != 0)
                {
                    ch.active_utc_offset_sec = round_to_hourly_offset(hb.server_utc_offset_sec as f64);
                    ch.utc_verified = true;
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
    }

    pub fn make_projection(&self, current_utc_now: UtcMs) -> EngineProjection {
        self.make_projection_at(current_utc_now, self.current_watermark)
    }

    pub fn make_projection_at(&self, current_utc_now: UtcMs, now_mono: MonoNs) -> EngineProjection {
        let mut broker_overviews = Vec::new();

        for b in &self.config.brokers {
            let st = self.spread_trackers.get(&b.id);
            let latest_q = self.latest_quotes.get(&b.id).cloned();
            let mut health = self.health_states.get(&b.id).cloned().unwrap_or_default();
            if health.connection == ConnectionState::Connected {
                health.data_freshness = match health.last_live_tick_rx_mono {
                    Some(last) if now_mono.0.saturating_sub(last.0)
                        <= self.config.health.stale_after_ms.saturating_mul(1_000_000) => FreshnessState::Live,
                    Some(_) => FreshnessState::Stale,
                    None => FreshnessState::Unknown,
                };
                health.heartbeat = match health.last_heartbeat_rx_mono {
                    Some(last) if now_mono.0.saturating_sub(last.0)
                        <= self.config.health.heartbeat_timeout_ms.saturating_mul(1_000_000) => HeartbeatState::Ok,
                    Some(_) => HeartbeatState::Timeout,
                    None => HeartbeatState::Unknown,
                };
            }
            let ch = self.channels.get(&b.id);
            let active_utc_offset_sec = ch.map(|c| c.active_utc_offset_sec).unwrap_or(b.utc_offset_sec);
            let is_auto_offset = ch.map(|c| c.auto_utc_offset).unwrap_or(b.auto_utc_offset);

            broker_overviews.push(BrokerOverview {
                broker_id: b.id,
                name: b.name.clone(),
                symbol: b.symbol.clone(),
                latest_quote: latest_q,
                min_spread: st.and_then(|s| s.min_spread()),
                max_spread: st.and_then(|s| s.max_spread()),
                health,
                tick_rate_1s: st.map(|s| s.tick_rate_1s_at(now_mono)).unwrap_or(0.0),
                active_utc_offset_sec,
                is_auto_offset,
            });
        }

        let (a, b) = self.active_pair;
        let fresh_quote = |broker_id: BrokerId| {
            self.latest_quotes.get(&broker_id).filter(|q| {
                self.channels.get(&broker_id).is_some_and(|ch| ch.is_connected)
                    && q.is_valid && !q.is_warmup && q.mid.is_finite()
                    && now_mono.0.saturating_sub(q.rx_mono_ns.0)
                        <= self.config.health.stale_after_ms.saturating_mul(1_000_000)
            })
        };
        let q_a = fresh_quote(a);
        let q_b = fresh_quote(b);
        let synchronized_pair = q_a.zip(q_b).filter(|(qa, qb)| {
            qa.rx_mono_ns.0.abs_diff(qb.rx_mono_ns.0)
                <= self.config.matcher.max_quote_skew_ms.saturating_mul(1_000_000)
        });

        let active_pair_comparison = Some(PairComparison {
            broker_a: a,
            broker_b: b,
            as_of_mono_ns: now_mono,
            bid_diff: if let Some((qa, qb)) = synchronized_pair {
                Some(qa.bid - qb.bid)
            } else {
                None
            },
            ask_diff: if let Some((qa, qb)) = synchronized_pair {
                Some(qa.ask - qb.ask)
            } else {
                None
            },
            mid_diff: if let Some((qa, qb)) = synchronized_pair {
                Some(qa.mid - qb.mid)
            } else {
                None
            },
            spread_diff: if let Some((qa, qb)) = synchronized_pair {
                Some(qa.spread - qb.spread)
            } else {
                None
            },
            recent_diff_series: self.pair_tracker.series(),
            latest_match: self.latest_pair_match.as_ref().filter(|m| {
                q_a.is_some() && q_b.is_some()
                    && now_mono.0.saturating_sub(m.t_follower.0) <= 5_000_000_000
            }).cloned(),
            ema_lead_lag_ms: self.matcher.current_ema_ms,
        });

        let mut candle_views = HashMap::new();
        let broker_ids: Vec<BrokerId> = self.config.brokers.iter().map(|b| b.id).collect();
        for retention in &self.config.history.retentions {
            let period = retention.period_ms;
            let cv = self.candle_book.get_candle_view(period, &broker_ids, 20, current_utc_now);
            candle_views.insert(period, cv);
        }

        // 1. Observed Broker Consensus & Dispersion
        let stale_brokers: Vec<BrokerId> = self.config.brokers.iter().filter(|b| {
            !self.channels.get(&b.id).is_some_and(|ch| ch.is_connected)
                || !self.latest_quotes.get(&b.id).is_some_and(|q| {
                    q.is_valid && !q.is_warmup && q.mid.is_finite()
                        && now_mono.0.saturating_sub(q.rx_mono_ns.0)
                            <= self.config.health.stale_after_ms.saturating_mul(1_000_000)
                })
        }).map(|b| b.id).collect();
        let mut consensus = self.consensus_calc.compute(
            self.latest_quotes.values().filter(|q| !stale_brokers.contains(&q.tick_id.broker_id)),
            now_mono,
        );
        consensus.total_count = self.config.brokers.len();

        // 2. Breadth & Active Burst Clusters
        let current_breadth = Some(self.burst_detector.compute_breadth_with_stale_brokers(
            self.config.brokers.len(), &stale_brokers, now_mono,
        ));
        let mut active_clusters = Vec::new();
        if let Some(c_up) = self.burst_detector.detect_cluster_at(MoveDirection::Up, self.config.brokers.len(), consensus.fresh_count, now_mono, &stale_brokers) {
            active_clusters.push(c_up);
        }
        if let Some(c_down) = self.burst_detector.detect_cluster_at(MoveDirection::Down, self.config.brokers.len(), consensus.fresh_count, now_mono, &stale_brokers) {
            active_clusters.push(c_down);
        }

        // 3. Broker Fingerprints & Hypotheses
        let mut fingerprints = HashMap::new();
        for (&bid, ft) in &self.fingerprint_trackers {
            fingerprints.insert(bid, ft.compile());
        }

        let mut hypotheses = Vec::new();
        for (&bid, fp) in &fingerprints {
            let repricing = self.repricing_persistence.get(&bid);
            let persistence = self.quote_persistence.get(&bid);
            let broker_hypotheses = self.hypothesis_engine.evaluate(fp, repricing, persistence);
            hypotheses.extend(broker_hypotheses);
        }

        // 4. Latency Summary
        let latency_summary = self.latency_metrics.compute_summary();

        // 5. Realtime Quote History
        let realtime_quote_points: Vec<RealtimeQuotePoint> = self.realtime_quote_history.iter().cloned().collect();

        EngineProjection {
            revision: self.projection_revision,
            watermark_ns: self.current_watermark,
            broker_overviews,
            active_pair: self.active_pair,
            active_pair_comparison,
            candle_views,
            global_diagnostics: self.diagnostics.iter().cloned().collect(),
            consensus: Some(consensus),
            active_clusters,
            current_breadth,
            fingerprints,
            hypotheses,
            latency_summary,
            realtime_quote_points,
        }
    }
}
