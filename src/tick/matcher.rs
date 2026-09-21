//! 1-to-1 Event Matcher for Lead/Lag.
//! Reference: docs/blueprint/semantics-lead-lag.md

use crate::contracts::models::{LeadLagMatch, MoveEvent};
use crate::contracts::types::*;
use std::collections::VecDeque;

#[derive(Debug, Clone)]
pub struct OneToOneEventMatcher {
    pub broker_a: BrokerId,
    pub broker_b: BrokerId,
    pub window_ns: u64,
    pub ema_alpha: f64,
    pub pending_a: VecDeque<MoveEvent>,
    pub pending_b: VecDeque<MoveEvent>,
    pub current_ema_ms: Option<f64>,
    pub match_count: u64,
    pub capacity: usize,
    pub segment_id: AnalysisSegmentId,
}

impl OneToOneEventMatcher {
    pub fn new(
        broker_a: BrokerId,
        broker_b: BrokerId,
        window_ms: u64,
        ema_alpha: f64,
        capacity: usize,
        segment_id: AnalysisSegmentId,
    ) -> Self {
        Self {
            broker_a,
            broker_b,
            window_ns: window_ms * 1_000_000,
            ema_alpha,
            pending_a: VecDeque::with_capacity(capacity),
            pending_b: VecDeque::with_capacity(capacity),
            current_ema_ms: None,
            match_count: 0,
            capacity,
            segment_id,
        }
    }

    pub fn reset(&mut self, new_segment_id: AnalysisSegmentId) {
        self.pending_a.clear();
        self.pending_b.clear();
        self.current_ema_ms = None;
        self.match_count = 0;
        self.segment_id = new_segment_id;
    }

    pub fn on_event(&mut self, event: MoveEvent) -> Option<LeadLagMatch> {
        if event.segment_id != self.segment_id {
            return None;
        }

        if event.broker_id == self.broker_a {
            // Find match in pending_b
            if let Some(match_idx) = self.find_best_match(&event, &self.pending_b) {
                let other_event = self.pending_b.remove(match_idx).unwrap();
                return Some(self.create_match(event, other_event));
            } else {
                if self.pending_a.len() >= self.capacity {
                    self.pending_a.pop_front();
                }
                self.pending_a.push_back(event);
            }
        } else if event.broker_id == self.broker_b {
            // Find match in pending_a
            if let Some(match_idx) = self.find_best_match(&event, &self.pending_a) {
                let other_event = self.pending_a.remove(match_idx).unwrap();
                return Some(self.create_match(other_event, event));
            } else {
                if self.pending_b.len() >= self.capacity {
                    self.pending_b.pop_front();
                }
                self.pending_b.push_back(event);
            }
        }

        None
    }

    fn find_best_match(&self, incoming: &MoveEvent, candidates: &VecDeque<MoveEvent>) -> Option<usize> {
        let mut best_idx = None;
        let mut min_diff = u64::MAX;

        for (i, cand) in candidates.iter().enumerate() {
            // 1. Same direction
            if cand.direction != incoming.direction {
                continue;
            }

            // 2. Strict positive time difference
            let t_inc = incoming.rx_mono_ns.0;
            let t_cand = cand.rx_mono_ns.0;
            if t_inc == t_cand {
                continue; // Delta 0 is not matched
            }

            let diff = t_inc.abs_diff(t_cand);

            // 3. Within window
            if diff <= self.window_ns {
                if diff < min_diff {
                    min_diff = diff;
                    best_idx = Some(i);
                } else if diff == min_diff {
                    // Tie-break by earlier sequence
                    if let Some(curr_best) = best_idx {
                        if cand.trigger_sequence < candidates[curr_best].trigger_sequence {
                            best_idx = Some(i);
                        }
                    }
                }
            }
        }

        best_idx
    }

    fn create_match(&mut self, event_a: MoveEvent, event_b: MoveEvent) -> LeadLagMatch {
        self.match_count += 1;
        let t_a = event_a.rx_mono_ns;
        let t_b = event_b.rx_mono_ns;

        // signed_delta_ns = t_B - t_A
        // If t_B > t_A: signed is positive (A leads)
        // If t_A > t_B: signed is negative (B leads)
        let signed_delta_ns = (t_b.0 as i64) - (t_a.0 as i64);
        let abs_delta_ns = signed_delta_ns.unsigned_abs();
        let raw_delta_ms = (signed_delta_ns as f64) / 1_000_000.0;

        let (leader, follower) = if signed_delta_ns > 0 {
            (self.broker_a, self.broker_b)
        } else {
            (self.broker_b, self.broker_a)
        };

        let ema = match self.current_ema_ms {
            Some(prev) => self.ema_alpha * raw_delta_ms + (1.0 - self.ema_alpha) * prev,
            None => raw_delta_ms,
        };
        self.current_ema_ms = Some(ema);

        LeadLagMatch {
            match_id: self.match_count,
            leader,
            follower,
            leader_event: if leader == self.broker_a { event_a.clone() } else { event_b.clone() },
            follower_event: if follower == self.broker_a { event_a } else { event_b },
            t_leader: if leader == self.broker_a { t_a } else { t_b },
            t_follower: if follower == self.broker_a { t_a } else { t_b },
            signed_delta_ns,
            abs_delta_ns,
            raw_delta_ms,
            ema_delta_ms: Some(ema),
            segment_id: self.segment_id,
        }
    }

    pub fn advance_watermark(&mut self, watermark: MonoNs) {
        let threshold = watermark.0.saturating_sub(self.window_ns);
        while let Some(front) = self.pending_a.front() {
            if front.rx_mono_ns.0 < threshold {
                self.pending_a.pop_front();
            } else {
                break;
            }
        }
        while let Some(front) = self.pending_b.front() {
            if front.rx_mono_ns.0 < threshold {
                self.pending_b.pop_front();
            } else {
                break;
            }
        }
    }
}
