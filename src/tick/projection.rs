//! Engine projection and snapshot construction.
//! Builds immutable EngineProjection snapshots for the UI exchange.

use crate::config::TimezoneRule;
use crate::core::models::*;
use crate::core::types::*;
use crate::tick::engine::TickEngine;
use std::collections::HashMap;

impl TickEngine {
    pub fn make_projection(&self, current_utc_now: UtcMs) -> EngineProjection {
        self.make_projection_at(current_utc_now, self.current_watermark)
    }

    pub fn make_projection_at(&self, current_utc_now: UtcMs, now_mono: MonoNs) -> EngineProjection {
        let mut broker_overviews = Vec::with_capacity(self.config.brokers.len());

        for b in &self.config.brokers {
            let st = self.spread_trackers.get(&b.id);
            let latest_q = self.fast_quotes.get(&b.id).or_else(|| self.latest_quotes.get(&b.id)).copied();
            let mut health = self.health_states.get(&b.id).copied().unwrap_or_default();
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
            let is_auto_offset = ch
                .map(|c| c.timezone_rule == TimezoneRule::NyClose || c.auto_utc_offset)
                .unwrap_or(b.timezone_rule == TimezoneRule::NyClose || b.auto_utc_offset);

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
            }).copied(),
            ema_lead_lag_ms: self.matcher.current_ema_ms,
        });

        let mut candle_views = HashMap::with_capacity(self.config.history.retentions.len());
        let mut mid_candle_views = HashMap::with_capacity(self.config.history.retentions.len());
        let broker_ids: Vec<BrokerId> = self.config.brokers.iter().map(|b| b.id).collect();
        for retention in &self.config.history.retentions {
            let period = retention.period_ms;
            let cv = self.candle_book.get_candle_view(
                period,
                &broker_ids,
                retention.slots,
                current_utc_now,
            );
            candle_views.insert(period, cv);
            let mid_cv = self.mid_candle_book.get_candle_view(
                period,
                &broker_ids,
                retention.slots,
                current_utc_now,
            );
            mid_candle_views.insert(period, mid_cv);
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
        let mut fingerprints = HashMap::with_capacity(self.fingerprint_trackers.len());
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

        // Detailed latency data is aggregated asynchronously only in -d mode.
        let latency_summary = crate::metrics::StageLatencySummary::default();

        // 5. Realtime Quote History
        let mut realtime_quote_points = Vec::with_capacity(self.realtime_quote_history.len());
        realtime_quote_points.extend(self.realtime_quote_history.iter().cloned());

        EngineProjection {
            revision: self.projection_revision,
            watermark_ns: self.current_watermark,
            broker_overviews,
            active_pair: self.active_pair,
            active_pair_comparison,
            candle_views,
            mid_candle_views,
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
