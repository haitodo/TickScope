//! Realtime price difference and rolling waveform buffer.
//! Reference: docs/blueprint/interfaces.md and docs/blueprint/semantics-tick.md

use crate::contracts::models::DiffPoint;
use crate::contracts::types::*;
use std::collections::VecDeque;

#[derive(Debug, Clone)]
pub struct PairDifferenceTracker {
    pub broker_a: BrokerId,
    pub broker_b: BrokerId,
    pub ring_buffer: VecDeque<DiffPoint>,
    max_history_ns: u64,
}

impl PairDifferenceTracker {
    pub fn new(broker_a: BrokerId, broker_b: BrokerId, visible_seconds: u64) -> Self {
        Self {
            broker_a,
            broker_b,
            ring_buffer: VecDeque::with_capacity(4096),
            max_history_ns: visible_seconds * 1_000_000_000,
        }
    }

    pub fn compute_and_record(
        &mut self,
        quote_a: Option<&Quote>,
        quote_b: Option<&Quote>,
        now_mono: MonoNs,
    ) -> (Option<f64>, Option<f64>, Option<f64>, Option<f64>) {
        if let (Some(qa), Some(qb)) = (quote_a, quote_b) {
            if qa.is_valid && qb.is_valid {
                let bid_diff = qa.bid - qb.bid;
                let ask_diff = qa.ask - qb.ask;
                let mid_diff = qa.mid - qb.mid;
                let spread_diff = qa.spread - qb.spread;

                let point = DiffPoint {
                    mono_ns: now_mono,
                    bid_diff,
                    ask_diff,
                    mid_diff,
                    spread_diff,
                };

                self.ring_buffer.push_back(point);

                // Prune older than visible window
                let threshold = now_mono.0.saturating_sub(self.max_history_ns);
                while let Some(front) = self.ring_buffer.front() {
                    if front.mono_ns.0 < threshold {
                        self.ring_buffer.pop_front();
                    } else {
                        break;
                    }
                }

                return (
                    Some(bid_diff),
                    Some(ask_diff),
                    Some(mid_diff),
                    Some(spread_diff),
                );
            }
        }
        (None, None, None, None)
    }

    pub fn series(&self) -> Vec<DiffPoint> {
        self.ring_buffer.iter().copied().collect()
    }
}
