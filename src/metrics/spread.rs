//! Spread and tick rate tracking.
//! Reference: docs/blueprint/interfaces.md

use crate::contracts::types::*;
use std::collections::VecDeque;

#[derive(Debug, Clone)]
pub struct SpreadTracker {
    broker_id: BrokerId,
    min_spread: Option<f64>,
    max_spread: Option<f64>,
    recent_ticks: VecDeque<MonoNs>,
}

impl SpreadTracker {
    pub fn new(broker_id: BrokerId) -> Self {
        Self {
            broker_id,
            min_spread: None,
            max_spread: None,
            recent_ticks: VecDeque::with_capacity(2048),
        }
    }

    pub fn broker_id(&self) -> BrokerId {
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

        self.recent_ticks.push_back(rx_mono);
        // Prune older than 1 second (1_000_000_000 ns)
        let one_sec_ago = rx_mono.0.saturating_sub(1_000_000_000);
        while let Some(front) = self.recent_ticks.front() {
            if front.0 < one_sec_ago {
                self.recent_ticks.pop_front();
            } else {
                break;
            }
        }
    }

    pub fn min_spread(&self) -> Option<f64> {
        self.min_spread
    }

    pub fn max_spread(&self) -> Option<f64> {
        self.max_spread
    }

    pub fn tick_rate_1s(&self) -> f64 {
        self.recent_ticks.len() as f64
    }
}
