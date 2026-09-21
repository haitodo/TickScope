//! Tick Engine: N-broker watermark merge, ledger, and pipeline coordination.
//! Reference: docs/blueprint/architecture.md and docs/blueprint/decisions.md

use crate::contracts::config::AppConfig;
use crate::contracts::models::*;
use crate::contracts::ports::LogSinkPort;
use crate::contracts::types::*;
use crate::metrics::lead_lag::SignificantMidMoveDetector;
use crate::metrics::price_diff::PairDifferenceTracker;
use crate::metrics::spread::SpreadTracker;
use crate::tick::candle::CandleBook;
use crate::tick::matcher::OneToOneEventMatcher;
use crate::tick::normalize::{normalize_tick, round_to_hourly_offset};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;

struct BrokerChannelState {
    is_connected: bool,
    generation: u64,
    watermark: MonoNs,
    pending_frames: VecDeque<ReceivedFrame>,
    expected_sequence: Sequence,
    session_id: Option<SessionId>,
    seen_sequences: HashSet<Sequence>,
    auto_utc_offset: bool,
    active_utc_offset_sec: i32,
    utc_verified: bool,
}

pub struct TickEngine {
    config: AppConfig,
    channels: HashMap<BrokerId, BrokerChannelState>,
    log_sink: Option<Arc<dyn LogSinkPort>>,
    candle_book: CandleBook,
    spread_trackers: HashMap<BrokerId, SpreadTracker>,
    latest_quotes: HashMap<BrokerId, Quote>,
    health_states: HashMap<BrokerId, HealthState>,
    pair_tracker: PairDifferenceTracker,
    move_detectors: HashMap<BrokerId, SignificantMidMoveDetector>,
    matcher: OneToOneEventMatcher,
    active_pair: (BrokerId, BrokerId),
    projection_revision: u64,
    current_watermark: MonoNs,
    diagnostics: Vec<Diagnostic>,
}

impl TickEngine {
    pub fn new(config: AppConfig, log_sink: Option<Arc<dyn LogSinkPort>>) -> Self {
        let mut channels = HashMap::new();
        let mut spread_trackers = HashMap::new();
        let mut health_states = HashMap::new();
        let mut move_detectors = HashMap::new();

        let periods = vec![1000, 5000, 10000, 60000];
        let candle_book = CandleBook::new(periods);

        for b in &config.brokers {
            channels.insert(
                b.id,
                BrokerChannelState {
                    is_connected: false,
                    generation: 0,
                    watermark: MonoNs::ZERO,
                    pending_frames: VecDeque::new(),
                    expected_sequence: 0,
                    session_id: None,
                    seen_sequences: HashSet::new(),
                    auto_utc_offset: b.auto_utc_offset,
                    active_utc_offset_sec: b.utc_offset_sec,
                    utc_verified: b.utc_verified,
                },
            );

            spread_trackers.insert(b.id, SpreadTracker::new(b.id));

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
        );
        let matcher = OneToOneEventMatcher::new(
            active_pair.0,
            active_pair.1,
            config.matcher.matching_window_ms,
            config.matcher.ema_alpha,
            config.matcher.pending_event_capacity,
            1,
        );

        Self {
            config,
            channels,
            log_sink,
            candle_book,
            spread_trackers,
            latest_quotes: HashMap::new(),
            health_states,
            pair_tracker,
            move_detectors,
            matcher,
            active_pair,
            projection_revision: 0,
            current_watermark: MonoNs::ZERO,
            diagnostics: Vec::new(),
        }
    }

    pub fn set_active_pair(&mut self, pair: (BrokerId, BrokerId)) {
        if self.channels.contains_key(&pair.0) && self.channels.contains_key(&pair.1) {
            self.active_pair = pair;
            self.pair_tracker = PairDifferenceTracker::new(
                pair.0,
                pair.1,
                self.config.display.visible_seconds,
            );
            self.matcher = OneToOneEventMatcher::new(
                pair.0,
                pair.1,
                self.config.matcher.matching_window_ms,
                self.config.matcher.ema_alpha,
                self.config.matcher.pending_event_capacity,
                1,
            );
        }
    }

    pub fn on_ingress_item(&mut self, item: IngressItem) {
        let broker_id = item.broker_id();
        let ch = match self.channels.get_mut(&broker_id) {
            Some(c) => c,
            None => return,
        };

        match item {
            IngressItem::Connected { generation, .. } => {
                ch.is_connected = true;
                ch.generation = generation;
                if let Some(h) = self.health_states.get_mut(&broker_id) {
                    h.connection = ConnectionState::Connected;
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
                ch.pending_frames.push_back(rf);
            }
            IngressItem::End { generation, reason, .. } => {
                if ch.generation == generation {
                    ch.is_connected = false;
                    if let Some(h) = self.health_states.get_mut(&broker_id) {
                        h.connection = ConnectionState::Disconnected;
                    }
                    self.diagnostics.push(Diagnostic {
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

        self.drain_and_process_merge();
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
                    let rf = self.channels.get_mut(&bid).unwrap().pending_frames.pop_front().unwrap();
                    self.process_frame(rf);
                }
                None => break,
            }
        }
    }

    fn process_frame(&mut self, rf: ReceivedFrame) {
        let broker_id = rf.frame.header.broker_id;
        let mut dispositions = Vec::new();

        // 1. Process payload according to message type
        match &rf.frame.payload {
            FramePayload::TickBatch(ticks) => {
                let ch = self.channels.get_mut(&broker_id).unwrap();
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
                        ch.seen_sequences.insert(tick.sequence);
                        ch.expected_sequence = tick.sequence + 1;
                        SequenceDisposition::New
                    } else {
                        ch.seen_sequences.insert(tick.sequence);
                        ch.expected_sequence = tick.sequence + 1;
                        SequenceDisposition::New
                    };
                    dispositions.push(disp);

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
                            is_valid: tick.bid > 0.0 && tick.ask > 0.0 && tick.ask >= tick.bid,
                        };

                        // Update spread tracker
                        if let Some(st) = self.spread_trackers.get_mut(&broker_id) {
                            st.on_quote(quote.spread, rf.rx_mono_ns);
                        }

                        // Feed CandleBook if valid
                        if let Ok(norm) = normalize_tick(&obs, utc_offset, utc_verified, 1) {
                            self.candle_book.on_tick(&norm, PriceMode::Bid, norm.utc_ms);
                        }

                        // Store latest quote
                        self.latest_quotes.insert(broker_id, quote.clone());

                        // If broker is part of active pair, update price diff & lead/lag
                        let (a, b) = self.active_pair;
                        if broker_id == a || broker_id == b {
                            let q_a = self.latest_quotes.get(&a);
                            let q_b = self.latest_quotes.get(&b);
                            self.pair_tracker.compute_and_record(q_a, q_b, rf.rx_mono_ns);

                            if let Some(md) = self.move_detectors.get_mut(&broker_id) {
                                if let Some(move_ev) = md.on_quote(&quote) {
                                    self.matcher.on_event(move_ev);
                                }
                            }
                        }

                        // Update health
                        if let Some(h) = self.health_states.get_mut(&broker_id) {
                            h.total_ticks_received += 1;
                            if !is_warmup {
                                h.last_live_tick_rx_mono = Some(rf.rx_mono_ns);
                                h.data_freshness = FreshnessState::Live;
                            }
                        }
                    }
                }
            }
            FramePayload::Heartbeat(hb) => {
                if let Some(h) = self.health_states.get_mut(&broker_id) {
                    h.last_heartbeat_rx_mono = Some(rf.rx_mono_ns);
                    h.heartbeat = HeartbeatState::Ok;
                }
                let ch = self.channels.get_mut(&broker_id).unwrap();
                ch.session_id = Some(hb.session_id);

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

        // 2. Append to LogSink
        if let Some(sink) = &self.log_sink {
            let log_frame = LogRawFrame {
                broker_id,
                connection_generation: rf.connection_generation,
                frame_index: rf.frame_index,
                rx_mono_ns: rf.rx_mono_ns,
                rx_unix_ns: rf.rx_unix_ns,
                config_epoch: 1,
                analysis_segment: 1,
                raw_wire_bytes: rf.raw_wire_bytes,
                dispositions,
            };
            sink.try_append(Arc::new(LogRecord::RawFrame(log_frame)));
        }

        if let Some(h) = self.health_states.get_mut(&broker_id) {
            h.total_frames_received += 1;
        }

        self.projection_revision += 1;
    }

    pub fn make_projection(&self, current_utc_now: UtcMs) -> EngineProjection {
        let mut broker_overviews = Vec::new();

        for b in &self.config.brokers {
            let st = self.spread_trackers.get(&b.id);
            let latest_q = self.latest_quotes.get(&b.id).cloned();
            let health = self.health_states.get(&b.id).cloned().unwrap_or_default();
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
                tick_rate_1s: st.map(|s| s.tick_rate_1s()).unwrap_or(0.0),
                active_utc_offset_sec,
                is_auto_offset,
            });
        }

        let (a, b) = self.active_pair;
        let q_a = self.latest_quotes.get(&a);
        let q_b = self.latest_quotes.get(&b);

        let active_pair_comparison = Some(PairComparison {
            broker_a: a,
            broker_b: b,
            as_of_mono_ns: self.current_watermark,
            bid_diff: if let (Some(qa), Some(qb)) = (q_a, q_b) {
                Some(qa.bid - qb.bid)
            } else {
                None
            },
            ask_diff: if let (Some(qa), Some(qb)) = (q_a, q_b) {
                Some(qa.ask - qb.ask)
            } else {
                None
            },
            mid_diff: if let (Some(qa), Some(qb)) = (q_a, q_b) {
                Some(qa.mid - qb.mid)
            } else {
                None
            },
            spread_diff: if let (Some(qa), Some(qb)) = (q_a, q_b) {
                Some(qa.spread - qb.spread)
            } else {
                None
            },
            recent_diff_series: self.pair_tracker.series(),
            latest_match: None,
            ema_lead_lag_ms: self.matcher.current_ema_ms,
        });

        let mut candle_views = HashMap::new();
        let broker_ids: Vec<BrokerId> = self.config.brokers.iter().map(|b| b.id).collect();
        for &period in &[1000, 5000, 10000, 60000] {
            let cv = self.candle_book.get_candle_view(period, &broker_ids, 20, current_utc_now);
            candle_views.insert(period, cv);
        }

        EngineProjection {
            revision: self.projection_revision,
            watermark_ns: self.current_watermark,
            broker_overviews,
            active_pair: self.active_pair,
            active_pair_comparison,
            candle_views,
            global_diagnostics: self.diagnostics.clone(),
        }
    }
}
