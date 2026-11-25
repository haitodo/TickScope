//! Spread and tick rate tracking.

use crate::core::types::{BrokerId, MonoNs};
use std::collections::VecDeque;

#[derive(Debug, Clone)]
pub struct SpreadTracker {
    broker_id: BrokerId,
    min_spread: Option<f64>,
    max_spread: Option<f64>,
    recent_ticks: VecDeque<MonoNs>,
}

impl SpreadTracker {
    #[must_use]
    pub fn new(broker_id: BrokerId) -> Self {
        Self {
            broker_id,
            min_spread: None,
            max_spread: None,
            recent_ticks: VecDeque::with_capacity(2048),
        }
    }

    #[must_use]
    pub const fn broker_id(&self) -> BrokerId {
        self.broker_id
    }

    pub fn on_quote(&mut self, spread: f64, rx_mono: MonoNs) {
        if spread <= 0.0 || spread.is_nan() {
            return;
        }

        self.min_spread = Some(match self.min_spread {
            Some(curr) => curr.min(spread),
            None => spread,
        });

        self.max_spread = Some(match self.max_spread {
            Some(curr) => curr.max(spread),
            None => spread,
        });

        if self.recent_ticks.back().is_some_and(|&last| rx_mono < last) {
            let pos = self.recent_ticks.partition_point(|&t| t <= rx_mono);
            self.recent_ticks.insert(pos, rx_mono);
        } else {
            self.recent_ticks.push_back(rx_mono);
        }
        // Prune older than 1 second (1_000_000_000 ns) relative to the newest timestamp.
        let latest = self.recent_ticks.back().copied().unwrap_or(rx_mono);
        let one_sec_ago = latest.0.saturating_sub(1_000_000_000);
        while let Some(front) = self.recent_ticks.front() {
            if front.0 < one_sec_ago {
                self.recent_ticks.pop_front();
            } else {
                break;
            }
        }
    }

    #[must_use]
    pub const fn min_spread(&self) -> Option<f64> {
        self.min_spread
    }

    #[must_use]
    pub const fn max_spread(&self) -> Option<f64> {
        self.max_spread
    }

    #[must_use]
    pub fn tick_rate_1s(&self) -> f64 {
        self.recent_ticks.len() as f64
    }

    #[must_use]
    pub fn tick_rate_1s_at(&self, now_mono: MonoNs) -> f64 {
        let cutoff = now_mono.0.saturating_sub(1_000_000_000);
        let start = self.recent_ticks.partition_point(|tick| tick.0 < cutoff);
        (self.recent_ticks.len() - start) as f64
    }
}
