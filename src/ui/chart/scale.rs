use crate::core::models::{BrokerOverview, PriceMode};
use crate::core::types::{BrokerId, ConnectionState, FreshnessState, MonoNs};
use crate::ui::settings::CandleFollowCriteria;
use egui::{Color32, Pos2, Stroke};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MarginEdgeLatchSide {
    Top,
    Bottom,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ValidQuoteCandidate {
    pub broker_id: BrokerId,
    pub broker_name: String,
    /// Selected candle price; the public Bid extractor supplies Bid here.
    pub bid: f64,
    pub rx_mono_ns: MonoNs,
    pub age_ms: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum FollowStatusInfo {
    Median {
        valid_brokers: usize,
        reference_price: f64,
    },
    MarginEdge {
        edge_broker_name: String,
        edge_age_ms: u64,
        is_top: bool,
        valid_brokers: usize,
    },
    NoValidQuotes {
        held_anchor: Option<f64>,
    },
}

pub fn extract_valid_quote_candidates(
    broker_ids: &[BrokerId],
    broker_overviews: &[BrokerOverview],
    chart_max_quote_age_ms: u64,
    now_mono: MonoNs,
) -> Vec<ValidQuoteCandidate> {
    extract_valid_quote_candidates_for_mode(
        broker_ids,
        broker_overviews,
        chart_max_quote_age_ms,
        now_mono,
        PriceMode::Bid,
    )
}

pub fn extract_valid_quote_candidates_for_mode(
    broker_ids: &[BrokerId],
    broker_overviews: &[BrokerOverview],
    chart_max_quote_age_ms: u64,
    now_mono: MonoNs,
    price_mode: PriceMode,
) -> Vec<ValidQuoteCandidate> {
    let mut candidates = Vec::with_capacity(broker_ids.len());
    let max_age_ns = chart_max_quote_age_ms.saturating_mul(1_000_000);

    for b in broker_overviews {
        if !broker_ids.contains(&b.broker_id) {
            continue;
        }
        if b.health.connection != ConnectionState::Connected {
            continue;
        }
        if b.health.data_freshness != FreshnessState::Live {
            continue;
        }
        if let Some(q) = &b.latest_quote {
            if !q.is_valid || q.is_warmup {
                continue;
            }
            if !q.bid.is_finite() || q.bid <= 0.0 || !q.ask.is_finite() || q.ask < q.bid {
                continue;
            }
            let price = match price_mode {
                PriceMode::Bid => q.bid,
                PriceMode::Ask => q.ask,
                PriceMode::Mid => q.mid,
            };
            if !price.is_finite() || price <= 0.0 {
                continue;
            }
            let age_ns = now_mono.0.saturating_sub(q.rx_mono_ns.0);
            if age_ns <= max_age_ns {
                candidates.push(ValidQuoteCandidate {
                    broker_id: b.broker_id,
                    broker_name: b.name.clone(),
                    bid: price,
                    rx_mono_ns: q.rx_mono_ns,
                    age_ms: age_ns / 1_000_000,
                });
            }
        }
    }
    candidates
}

pub fn resolve_candle_follow_scale(
    follow_criteria: CandleFollowCriteria,
    candidates: &[ValidQuoteCandidate],
    span_pips: f64,
    pip_size: f64,
    chart_anchor: &mut Option<f64>,
    margin_edge_latch: &mut Option<MarginEdgeLatchSide>,
    fallback_price: Option<f64>,
) -> ((f64, f64), FollowStatusInfo) {
    let pip_size = pip_size.max(f64::EPSILON);
    let full_span = (span_pips * pip_size).max(f64::EPSILON);
    let half_span = full_span * 0.5;
    let deadzone = full_span * 0.4; // 80% stationary zone -> ±0.4 W from center

    if candidates.is_empty() {
        *margin_edge_latch = None;
        let anchor = match *chart_anchor {
            Some(a) => a,
            None => {
                let p = fallback_price.unwrap_or(0.0);
                *chart_anchor = Some(p);
                p
            }
        };
        return (
            (anchor - half_span, anchor + half_span),
            FollowStatusInfo::NoValidQuotes {
                held_anchor: *chart_anchor,
            },
        );
    }

    match follow_criteria {
        CandleFollowCriteria::Median => {
            *margin_edge_latch = None;
            let mut sorted = candidates.to_vec();
            sorted.sort_by(|a, b| {
                a.bid
                    .partial_cmp(&b.bid)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
            let len = sorted.len();
            let median_price = if len % 2 == 1 {
                sorted[len / 2].bid
            } else {
                (sorted[len / 2 - 1].bid + sorted[len / 2].bid) * 0.5
            };

            let anchor = match *chart_anchor {
                None => {
                    *chart_anchor = Some(median_price);
                    median_price
                }
                Some(current) => {
                    let new_anchor = if median_price > current + deadzone {
                        median_price - deadzone
                    } else if median_price < current - deadzone {
                        median_price + deadzone
                    } else {
                        current
                    };
                    *chart_anchor = Some(new_anchor);
                    new_anchor
                }
            };

            (
                (anchor - half_span, anchor + half_span),
                FollowStatusInfo::Median {
                    valid_brokers: len,
                    reference_price: median_price,
                },
            )
        }
        CandleFollowCriteria::MarginEdge => {
            // Find minimum and maximum price candidates. Break ties by broker_id.
            let mut min_candidate = &candidates[0];
            let mut max_candidate = &candidates[0];
            for c in &candidates[1..] {
                if c.bid < min_candidate.bid
                    || (c.bid == min_candidate.bid && c.broker_id < min_candidate.broker_id)
                {
                    min_candidate = c;
                }
                if c.bid > max_candidate.bid
                    || (c.bid == max_candidate.bid && c.broker_id < max_candidate.broker_id)
                {
                    max_candidate = c;
                }
            }

            let initial_anchor = (min_candidate.bid + max_candidate.bid) * 0.5;
            let mut current_anchor = chart_anchor.unwrap_or(initial_anchor);

            let is_top_out = max_candidate.bid > current_anchor + deadzone;
            let is_bottom_out = min_candidate.bid < current_anchor - deadzone;

            let active_edge_is_top;
            let active_candidate;

            if is_top_out && is_bottom_out {
                // D1: Both exceeded
                match *margin_edge_latch {
                    Some(MarginEdgeLatchSide::Top) => {
                        current_anchor = max_candidate.bid - deadzone;
                        active_edge_is_top = true;
                        active_candidate = max_candidate;
                    }
                    Some(MarginEdgeLatchSide::Bottom) => {
                        current_anchor = min_candidate.bid + deadzone;
                        active_edge_is_top = false;
                        active_candidate = min_candidate;
                    }
                    None => {
                        // Tie-breaking: choose the side with the more recent reception time
                        if max_candidate.rx_mono_ns.0 > min_candidate.rx_mono_ns.0 {
                            current_anchor = max_candidate.bid - deadzone;
                            *margin_edge_latch = Some(MarginEdgeLatchSide::Top);
                            active_edge_is_top = true;
                            active_candidate = max_candidate;
                        } else if min_candidate.rx_mono_ns.0 > max_candidate.rx_mono_ns.0 {
                            current_anchor = min_candidate.bid + deadzone;
                            *margin_edge_latch = Some(MarginEdgeLatchSide::Bottom);
                            active_edge_is_top = false;
                            active_candidate = min_candidate;
                        } else {
                            // Simultaneous or indeterminate: place anchor at range midpoint
                            current_anchor = (min_candidate.bid + max_candidate.bid) * 0.5;
                            *margin_edge_latch = None;
                            active_edge_is_top = true;
                            active_candidate = max_candidate;
                        }
                    }
                }
            } else if is_top_out {
                current_anchor = max_candidate.bid - deadzone;
                *margin_edge_latch = Some(MarginEdgeLatchSide::Top);
                active_edge_is_top = true;
                active_candidate = max_candidate;
            } else if is_bottom_out {
                current_anchor = min_candidate.bid + deadzone;
                *margin_edge_latch = Some(MarginEdgeLatchSide::Bottom);
                active_edge_is_top = false;
                active_candidate = min_candidate;
            } else {
                // Neither exceeded: valid range is within stationary zone [A - 0.4W, A + 0.4W].
                // Release latch according to D1 recommendation.
                *margin_edge_latch = None;
                // For UI label: pick whichever candidate is closer to the edge
                let dist_to_top = (current_anchor + deadzone) - max_candidate.bid;
                let dist_to_bottom = min_candidate.bid - (current_anchor - deadzone);
                if dist_to_top <= dist_to_bottom {
                    active_edge_is_top = true;
                    active_candidate = max_candidate;
                } else {
                    active_edge_is_top = false;
                    active_candidate = min_candidate;
                }
            }

            *chart_anchor = Some(current_anchor);

            (
                (current_anchor - half_span, current_anchor + half_span),
                FollowStatusInfo::MarginEdge {
                    edge_broker_name: active_candidate.broker_name.clone(),
                    edge_age_ms: active_candidate.age_ms,
                    is_top: active_edge_is_top,
                    valid_brokers: candidates.len(),
                },
            )
        }
    }
}

pub fn draw_clip_marker(painter: &egui::Painter, cx: f32, cy: f32, is_top: bool, color: Color32) {
    let half_w = 3.0_f32;
    let (p1, p2, p3) = if is_top {
        (
            Pos2::new(cx, cy - 2.0),
            Pos2::new(cx - half_w, cy + 3.0),
            Pos2::new(cx + half_w, cy + 3.0),
        )
    } else {
        (
            Pos2::new(cx, cy + 2.0),
            Pos2::new(cx - half_w, cy - 3.0),
            Pos2::new(cx + half_w, cy - 3.0),
        )
    };
    painter.add(egui::Shape::convex_polygon(vec![p1, p2, p3], color, Stroke::NONE));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_valid_quote_candidates_filters_stale_and_invalid() {
        use crate::core::types::{HealthState, Quote, TickId};
        let now = MonoNs(2_000_000_000); // 2000 ms
        let brokers = vec![
            // 1. Fresh and valid
            BrokerOverview {
                broker_id: 1,
                name: "Axiory".to_string(),
                symbol: "USDJPY".to_string(),
                latest_quote: Some(Quote {
                    tick_id: TickId { broker_id: 1, session_id: 1, sequence: 1 },
                    bid: 150.10,
                    ask: 150.12,
                    mid: 150.11,
                    spread: 0.02,
                    rx_mono_ns: MonoNs(1_900_000_000), // 100ms old
                    utc_ms: None,
                    is_warmup: false,
                    is_valid: true,
                }),
                min_spread: None,
                max_spread: None,
                health: HealthState {
                    broker_id: 1,
                    connection: ConnectionState::Connected,
                    data_freshness: FreshnessState::Live,
                    ..Default::default()
                },
                tick_rate_1s: 10.0,
                active_utc_offset_sec: 0,
                is_auto_offset: false,
            },
            // 2. Stale Quote (> 500ms when chart_max_quote_age_ms is 500)
            BrokerOverview {
                broker_id: 2,
                name: "OANDA".to_string(),
                symbol: "USDJPY".to_string(),
                latest_quote: Some(Quote {
                    tick_id: TickId { broker_id: 2, session_id: 1, sequence: 2 },
                    bid: 150.15,
                    ask: 150.17,
                    mid: 150.16,
                    spread: 0.02,
                    rx_mono_ns: MonoNs(1_200_000_000), // 800ms old
                    utc_ms: None,
                    is_warmup: false,
                    is_valid: true,
                }),
                min_spread: None,
                max_spread: None,
                health: HealthState {
                    broker_id: 2,
                    connection: ConnectionState::Connected,
                    data_freshness: FreshnessState::Live,
                    ..Default::default()
                },
                tick_rate_1s: 10.0,
                active_utc_offset_sec: 0,
                is_auto_offset: false,
            },
            // 3. Disconnected
            BrokerOverview {
                broker_id: 3,
                name: "JFX".to_string(),
                symbol: "USDJPY".to_string(),
                latest_quote: Some(Quote {
                    tick_id: TickId { broker_id: 3, session_id: 1, sequence: 3 },
                    bid: 150.11,
                    ask: 150.13,
                    mid: 150.12,
                    spread: 0.02,
                    rx_mono_ns: MonoNs(1_950_000_000),
                    utc_ms: None,
                    is_warmup: false,
                    is_valid: true,
                }),
                min_spread: None,
                max_spread: None,
                health: HealthState {
                    broker_id: 3,
                    connection: ConnectionState::Disconnected,
                    data_freshness: FreshnessState::Live,
                    ..Default::default()
                },
                tick_rate_1s: 0.0,
                active_utc_offset_sec: 0,
                is_auto_offset: false,
            },
            // 4. Invalid warmup
            BrokerOverview {
                broker_id: 4,
                name: "XM".to_string(),
                symbol: "USDJPY".to_string(),
                latest_quote: Some(Quote {
                    tick_id: TickId { broker_id: 4, session_id: 1, sequence: 4 },
                    bid: 150.12,
                    ask: 150.14,
                    mid: 150.13,
                    spread: 0.02,
                    rx_mono_ns: MonoNs(1_980_000_000),
                    utc_ms: None,
                    is_warmup: true,
                    is_valid: true,
                }),
                min_spread: None,
                max_spread: None,
                health: HealthState {
                    broker_id: 4,
                    connection: ConnectionState::Connected,
                    data_freshness: FreshnessState::Live,
                    ..Default::default()
                },
                tick_rate_1s: 5.0,
                active_utc_offset_sec: 0,
                is_auto_offset: false,
            },
        ];

        let broker_ids = [1, 2, 3, 4];
        let candidates = extract_valid_quote_candidates(&broker_ids, &brokers, 500, now);
        // Only broker 1 should pass (broker 2 is 800ms old > 500ms, broker 3 is disconnected, broker 4 is warmup)
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].broker_id, 1);
        assert_eq!(candidates[0].bid, 150.10);
        assert_eq!(candidates[0].age_ms, 100);
    }

    #[test]
    fn test_median_mode_odd_even_and_deadzone_movement() {
        let pip_size = 0.01;
        let span_pips = 10.0; // W = 0.10, half_span = 0.05, deadzone = 0.04 (±4 pips)
        let mut chart_anchor = None;
        let mut latch = None;

        // 3 brokers (odd): bids 150.00, 150.05, 150.10 -> median = 150.05
        let candidates = vec![
            ValidQuoteCandidate {
                broker_id: 1,
                broker_name: "B1".to_string(),
                bid: 150.00,
                rx_mono_ns: MonoNs(100),
                age_ms: 10,
            },
            ValidQuoteCandidate {
                broker_id: 2,
                broker_name: "B2".to_string(),
                bid: 150.05,
                rx_mono_ns: MonoNs(100),
                age_ms: 10,
            },
            ValidQuoteCandidate {
                broker_id: 3,
                broker_name: "B3".to_string(),
                bid: 150.10,
                rx_mono_ns: MonoNs(100),
                age_ms: 10,
            },
        ];

        let ((min1, max1), status1) = resolve_candle_follow_scale(
            CandleFollowCriteria::Median,
            &candidates,
            span_pips,
            pip_size,
            &mut chart_anchor,
            &mut latch,
            None,
        );

        assert_eq!(chart_anchor, Some(150.05));
        assert!((min1 - 150.00).abs() < 1e-6);
        assert!((max1 - 150.10).abs() < 1e-6);
        assert_eq!(
            status1,
            FollowStatusInfo::Median {
                valid_brokers: 3,
                reference_price: 150.05
            }
        );

        // Price moves within deadzone: median moves to 150.08 (+3 pips from 150.05, <= 4 pips deadzone)
        let candidates_small_move = vec![
            ValidQuoteCandidate {
                broker_id: 1,
                broker_name: "B1".to_string(),
                bid: 150.04,
                rx_mono_ns: MonoNs(200),
                age_ms: 10,
            },
            ValidQuoteCandidate {
                broker_id: 2,
                broker_name: "B2".to_string(),
                bid: 150.08,
                rx_mono_ns: MonoNs(200),
                age_ms: 10,
            },
            ValidQuoteCandidate {
                broker_id: 3,
                broker_name: "B3".to_string(),
                bid: 150.12,
                rx_mono_ns: MonoNs(200),
                age_ms: 10,
            },
        ];

        let (_, _) = resolve_candle_follow_scale(
            CandleFollowCriteria::Median,
            &candidates_small_move,
            span_pips,
            pip_size,
            &mut chart_anchor,
            &mut latch,
            None,
        );
        // Anchor should stay at 150.05 (stationary zone holds)
        assert_eq!(chart_anchor, Some(150.05));

        // Price breaks upper deadzone: median jumps to 150.15 (+10 pips from 150.05)
        // Required shift: A = P - 0.4W = 150.15 - 0.04 = 150.11
        let candidates_break = vec![
            ValidQuoteCandidate {
                broker_id: 1,
                broker_name: "B1".to_string(),
                bid: 150.14,
                rx_mono_ns: MonoNs(300),
                age_ms: 10,
            },
            ValidQuoteCandidate {
                broker_id: 2,
                broker_name: "B2".to_string(),
                bid: 150.15,
                rx_mono_ns: MonoNs(300),
                age_ms: 10,
            },
            ValidQuoteCandidate {
                broker_id: 3,
                broker_name: "B3".to_string(),
                bid: 150.16,
                rx_mono_ns: MonoNs(300),
                age_ms: 10,
            },
        ];

        let ((min_break, max_break), _) = resolve_candle_follow_scale(
            CandleFollowCriteria::Median,
            &candidates_break,
            span_pips,
            pip_size,
            &mut chart_anchor,
            &mut latch,
            None,
        );
        assert!((chart_anchor.unwrap() - 150.11).abs() < 1e-6);
        assert!((min_break - 150.06).abs() < 1e-6);
        assert!((max_break - 150.16).abs() < 1e-6);
    }

    #[test]
    fn test_margin_edge_d1_latch_mechanism() {
        let pip_size = 0.01;
        let span_pips = 10.0; // W = 0.10, deadzone = 0.04 (±4 pips)
        let mut chart_anchor = Some(150.00);
        let mut latch = None;

        // 1. Initial state: anchor at 150.00. Deadzone is [149.96, 150.04].
        // Broker 1 jumps to 150.06 (> 150.04). Top edge exceeded.
        let candidates1 = vec![
            ValidQuoteCandidate {
                broker_id: 1,
                broker_name: "FastBroker".to_string(),
                bid: 150.06,
                rx_mono_ns: MonoNs(100),
                age_ms: 5,
            },
            ValidQuoteCandidate {
                broker_id: 2,
                broker_name: "SlowBroker".to_string(),
                bid: 150.00,
                rx_mono_ns: MonoNs(90),
                age_ms: 15,
            },
        ];

        let (_, status1) = resolve_candle_follow_scale(
            CandleFollowCriteria::MarginEdge,
            &candidates1,
            span_pips,
            pip_size,
            &mut chart_anchor,
            &mut latch,
            None,
        );

        // Anchor moves to 150.06 - 0.04 = 150.02. Latch set to Top.
        assert!((chart_anchor.unwrap() - 150.02).abs() < 1e-6);
        assert_eq!(latch, Some(MarginEdgeLatchSide::Top));
        if let FollowStatusInfo::MarginEdge { edge_broker_name, is_top, .. } = status1 {
            assert_eq!(edge_broker_name, "FastBroker");
            assert!(is_top);
        } else {
            panic!("Expected MarginEdge status");
        }

        // 2. Severe spread expansion: Both top and bottom margins exceeded!
        // Anchor is at 150.02, deadzone is [149.98, 150.06].
        // FastBroker is 150.08 (> 150.06), SlowBroker drops to 149.90 (< 149.98).
        // Since latch is Top, D1 dictates staying locked to Top to prevent jitter!
        let candidates2 = vec![
            ValidQuoteCandidate {
                broker_id: 1,
                broker_name: "FastBroker".to_string(),
                bid: 150.08,
                rx_mono_ns: MonoNs(150),
                age_ms: 5,
            },
            ValidQuoteCandidate {
                broker_id: 2,
                broker_name: "SlowBroker".to_string(),
                bid: 149.90,
                rx_mono_ns: MonoNs(160),
                age_ms: 2,
            },
        ];

        let _ = resolve_candle_follow_scale(
            CandleFollowCriteria::MarginEdge,
            &candidates2,
            span_pips,
            pip_size,
            &mut chart_anchor,
            &mut latch,
            None,
        );

        // Must still follow Top: A = 150.08 - 0.04 = 150.04, latch remains Top.
        assert!((chart_anchor.unwrap() - 150.04).abs() < 1e-6);
        assert_eq!(latch, Some(MarginEdgeLatchSide::Top));

        // 3. Convergence back into stationary zone:
        // Anchor at 150.04 -> deadzone [150.00, 150.08].
        // Both brokers inside deadzone: FastBroker = 150.05, SlowBroker = 150.02.
        let candidates3 = vec![
            ValidQuoteCandidate {
                broker_id: 1,
                broker_name: "FastBroker".to_string(),
                bid: 150.05,
                rx_mono_ns: MonoNs(200),
                age_ms: 5,
            },
            ValidQuoteCandidate {
                broker_id: 2,
                broker_name: "SlowBroker".to_string(),
                bid: 150.02,
                rx_mono_ns: MonoNs(200),
                age_ms: 5,
            },
        ];

        let _ = resolve_candle_follow_scale(
            CandleFollowCriteria::MarginEdge,
            &candidates3,
            span_pips,
            pip_size,
            &mut chart_anchor,
            &mut latch,
            None,
        );

        // Latch must be released!
        assert!((chart_anchor.unwrap() - 150.04).abs() < 1e-6);
        assert_eq!(latch, None);
    }
}
