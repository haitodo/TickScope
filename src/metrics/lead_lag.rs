//! Significant Mid-move event detector for Lead/Lag.
//! Reference: docs/blueprint/semantics-lead-lag.md

use crate::contracts::models::{MoveDirection, MoveEvent, MoveQuality};
use crate::contracts::types::*;

#[derive(Debug, Clone)]
pub struct SignificantMidMoveDetector {
    pub broker_id: BrokerId,
    pub point_size: f64,
    pub trigger_move_points: f64,
    pub cooldown_ns: u64,
    pub anchor_quote: Option<Quote>,
    pub cooldown_until: MonoNs,
    pub segment_id: AnalysisSegmentId,
}

impl SignificantMidMoveDetector {
    pub fn new(
        broker_id: BrokerId,
        point_size: f64,
        trigger_move_points: f64,
        cooldown_ms: u64,
        segment_id: AnalysisSegmentId,
    ) -> Self {
        Self {
            broker_id,
            point_size,
            trigger_move_points,
            cooldown_ns: cooldown_ms * 1_000_000,
            anchor_quote: None,
            cooldown_until: MonoNs::ZERO,
            segment_id,
        }
    }

    pub fn reset(&mut self, new_segment_id: AnalysisSegmentId) {
        self.anchor_quote = None;
        self.cooldown_until = MonoNs::ZERO;
        self.segment_id = new_segment_id;
    }

    pub fn on_quote(&mut self, quote: &Quote) -> Option<MoveEvent> {
        if !quote.is_valid || quote.is_warmup {
            return None;
        }

        let anchor = match self.anchor_quote.as_ref() {
            Some(a) => a,
            None => {
                // First valid quote sets anchor, does not fire
                self.anchor_quote = Some(quote.clone());
                return None;
            }
        };

        // Invariant I13: During cooldown, anchor does NOT follow price, and event is suppressed
        if quote.rx_mono_ns < self.cooldown_until {
            return None;
        }

        let threshold = self.trigger_move_points * self.point_size;
        let mid_diff = quote.mid - anchor.mid;

        if mid_diff.abs() >= threshold {
            let direction = if mid_diff > 0.0 {
                MoveDirection::Up
            } else {
                MoveDirection::Down
            };

            let bid_delta = quote.bid - anchor.bid;
            let ask_delta = quote.ask - anchor.ask;
            let spread_delta = quote.spread - anchor.spread;
            let mid_delta_points = if self.point_size > 0.0 {
                mid_diff.abs() / self.point_size
            } else {
                0.0
            };

            let quality = if bid_delta != 0.0 && ask_delta != 0.0 {
                if spread_delta.abs() > 0.0000001 && ((bid_delta > 0.0) != (ask_delta > 0.0)) {
                    MoveQuality::SpreadDriven
                } else {
                    MoveQuality::BothSides
                }
            } else if bid_delta != 0.0 {
                MoveQuality::BidOnly
            } else if ask_delta != 0.0 {
                MoveQuality::AskOnly
            } else {
                MoveQuality::BothSides
            };

            let event = MoveEvent {
                segment_id: self.segment_id,
                broker_id: self.broker_id,
                trigger_sequence: quote.tick_id.sequence,
                rx_mono_ns: quote.rx_mono_ns,
                direction,
                anchor_mid: anchor.mid,
                current_mid: quote.mid,
                mid_delta_points,
                bid_delta,
                ask_delta,
                mid_delta: mid_diff,
                spread_delta,
                quality,
            };

            // Invariant I13: Immediately re-anchor to current quote, set cooldown
            self.anchor_quote = Some(quote.clone());
            self.cooldown_until = MonoNs(quote.rx_mono_ns.0 + self.cooldown_ns);

            Some(event)
        } else {
            None
        }
    }
}
