//! Incremental 0.2p noise-cancelled tick candlestick generator.
//!
//! Faithfully ports the real-time processing algorithm from MQL5 `replay-TickCandle.mq5`.
//! Ticks are processed sequentially:
//! 1. Sub-epsilon price changes (< 0.1 point = 0.01 pip) are skipped.
//! 2. Unconfirmed candles that experience an equal and opposite rebound within `threshold_pips` (0.2p)
//!    are noise-cancelled (folded back into open) and marked for slot reuse.
//! 3. When `is_minute_changed` is checked, it compares `time_sec / 60` with the previous candle's `time_sec / 60`.
//! 4. Maintains a ring buffer bounded by `max_candles` (default 200).

use crate::core::models::TickCandleBar;
use crate::core::types::BrokerId;

#[derive(Debug, Clone)]
pub struct TickCandleTracker {
    pub broker_id: BrokerId,
    pub max_candles: usize,
    pub threshold_pips: f64,
    pub pip_size: f64,
    pub candles: Vec<TickCandleBar>,
    pub last_price: f64,
    pub has_unconfirmed: bool,
    pub is_reusing_candle: bool,
}

impl TickCandleTracker {
    #[must_use]
    pub fn new(broker_id: BrokerId, max_candles: usize, threshold_pips: f64, pip_size: f64) -> Self {
        Self {
            broker_id,
            max_candles,
            threshold_pips,
            pip_size: if pip_size > 0.0 { pip_size } else { 0.01 },
            candles: Vec::with_capacity(max_candles),
            last_price: 0.0,
            has_unconfirmed: false,
            is_reusing_candle: false,
        }
    }

    pub fn reset(&mut self) {
        self.candles.clear();
        self.last_price = 0.0;
        self.has_unconfirmed = false;
        self.is_reusing_candle = false;
    }

    /// Process one incoming tick price and broker timestamp (in seconds).
    pub fn on_tick(&mut self, price: f64, time_sec: i64) {
        if price <= 0.0 || !price.is_finite() || self.pip_size <= 0.0 {
            return;
        }

        if self.last_price == 0.0 {
            self.last_price = price;
            return;
        }

        let diff = price - self.last_price;
        // In MT5: PriceEpsilon() = MathMax(_Point * 0.1, 1e-10)
        // Since PipSize = _Point * 10, _Point * 0.1 = PipSize * 0.01 (0.1 point).
        let epsilon = (self.pip_size * 0.01).max(1e-9);
        if diff.abs() < epsilon {
            return;
        }

        let diff_pips = diff / self.pip_size;
        let abs_diff_pips = diff_pips.abs();

        if self.has_unconfirmed && !self.candles.is_empty() && !self.is_reusing_candle {
            let last_idx = self.candles.len() - 1;
            let prev_diff = self.candles[last_idx].close - self.candles[last_idx].open;
            let prev_diff_pips = prev_diff / self.pip_size;

            let is_opposite = prev_diff_pips * diff_pips < 0.0;
            let is_same_size = (prev_diff_pips.abs() - abs_diff_pips).abs() < 1e-4;
            let is_within_threshold = abs_diff_pips <= self.threshold_pips + 1e-4;

            if is_opposite && is_same_size && is_within_threshold {
                let open = self.candles[last_idx].open;
                self.candles[last_idx].close = open;
                self.candles[last_idx].high = open;
                self.candles[last_idx].low = open;
                self.candles[last_idx].is_confirmed = false;

                self.last_price = open;
                self.is_reusing_candle = true;
                return;
            } else {
                self.candles[last_idx].is_confirmed = true;
            }
        }

        if self.is_reusing_candle && !self.candles.is_empty() {
            let target_idx = self.candles.len() - 1;
            let open = self.candles[target_idx].open;
            self.candles[target_idx].close = price;
            self.candles[target_idx].high = open.max(price);
            self.candles[target_idx].low = open.min(price);
            self.candles[target_idx].time_sec = time_sec;

            if target_idx > 0 {
                let prev_time = self.candles[target_idx - 1].time_sec;
                self.candles[target_idx].is_minute_changed = time_sec / 60 != prev_time / 60;
            }
            self.is_reusing_candle = false;
        } else {
            if self.candles.len() >= self.max_candles {
                self.candles.remove(0);
            }

            let is_minute_changed = if let Some(prev) = self.candles.last() {
                time_sec / 60 != prev.time_sec / 60
            } else {
                false
            };

            self.candles.push(TickCandleBar {
                open: self.last_price,
                high: self.last_price.max(price),
                low: self.last_price.min(price),
                close: price,
                time_sec,
                is_minute_changed,
                is_confirmed: false,
            });
        }

        self.has_unconfirmed = true;
        self.last_price = price;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tracker_noise_cancellation() {
        let mut tracker = TickCandleTracker::new(1, 200, 0.2, 0.01);
        tracker.on_tick(150.000, 1000);
        // +0.1 pip move (1 point = 0.001)
        tracker.on_tick(150.001, 1001);
        assert_eq!(tracker.candles.len(), 1);
        assert_eq!(tracker.candles[0].close, 150.001);

        // -0.1 pip opposite move (sub-threshold cancellation back to open)
        tracker.on_tick(150.000, 1002);
        assert!(tracker.is_reusing_candle);
        assert_eq!(tracker.candles[0].close, 150.000);

        // Next move reuses the slot
        tracker.on_tick(150.005, 1003);
        assert_eq!(tracker.candles.len(), 1);
        assert_eq!(tracker.candles[0].close, 150.005);
        assert_eq!(tracker.candles[0].open, 150.000);
    }

    #[test]
    fn test_tracker_minute_transition() {
        let mut tracker = TickCandleTracker::new(1, 200, 0.2, 0.01);
        tracker.on_tick(150.000, 59); // 00:00:59
        tracker.on_tick(150.005, 59);
        assert!(!tracker.candles[0].is_minute_changed);

        // Next tick in new minute (60 = 00:01:00)
        tracker.on_tick(150.010, 60);
        assert_eq!(tracker.candles.len(), 2);
        assert!(tracker.candles[1].is_minute_changed);
    }

    #[test]
    fn test_tracker_capacity_bounded() {
        let mut tracker = TickCandleTracker::new(1, 10, 0.2, 0.01);
        tracker.on_tick(150.000, 0);
        for i in 1..=50 {
            tracker.on_tick(150.000 + (i as f64) * 0.01, i * 60);
        }
        assert_eq!(tracker.candles.len(), 10);
    }
}
