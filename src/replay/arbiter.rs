//! Synchronization Arbiter: Evaluates incoming messages from TickReplay,
//! enforces seek hysteresis to eliminate false resets during playback,
//! and coordinates clock phase locking.

use super::clock::VirtualClock;
use super::driver::mt5_to_utc_ms;
use crate::core::models::{ReplayTrade, ReplayTradeStore};
use parking_lot::RwLock;
use std::sync::Arc;

/// Actions determined by the SyncArbiter after evaluating external status.
#[derive(Debug, Clone, PartialEq)]
pub enum ArbiterAction {
    /// Full state rebuild required (SEEK, loop reset, or initial sync).
    Seek {
        target_utc_ms: i64,
        is_playing: bool,
        multiplier: f64,
    },
    /// Smooth playback update (phase locked, multiplier/play-state sync).
    PlaybackUpdate {
        target_utc_ms: i64,
        is_playing: bool,
        multiplier: f64,
        data_updated: bool,
    },
    /// Message contained no actionable time update or was invalid.
    Ignore,
}

pub struct SyncArbiter {
    last_observed_mt5_ms: i64,
}

impl Default for SyncArbiter {
    fn default() -> Self {
        Self::new()
    }
}

impl SyncArbiter {
    pub fn new() -> Self {
        Self {
            last_observed_mt5_ms: 0,
        }
    }

    /// Reset internal tracking (e.g. on new connection).
    pub fn reset(&mut self) {
        self.last_observed_mt5_ms = 0;
    }

    /// Evaluates raw JSON or typed status and returns the appropriate action.
    /// Incorporates a -250ms hysteresis deadband to prevent false SEEK resets
    /// caused by TickReplay's Graceful Slowdown timestamp clamping.
    pub fn evaluate(
        &mut self,
        val: &serde_json::Value,
        clock: &VirtualClock,
        trade_store: &Arc<RwLock<ReplayTradeStore>>,
    ) -> (ArbiterAction, Option<(f64, f64, i64)>) {
        let virtual_time_msc = val.get("virtual_time_msc").and_then(|v| v.as_i64()).unwrap_or(0);
        if virtual_time_msc <= 0 {
            return (ArbiterAction::Ignore, None);
        }

        let is_playing = val.get("is_playing").and_then(|p| p.as_bool()).unwrap_or(false);
        let multiplier = val
            .get("multiplier")
            .and_then(|m| m.as_f64().or_else(|| m.as_str().and_then(|s| s.parse().ok())))
            .unwrap_or(1.0);

        let target_utc_ms = mt5_to_utc_ms(virtual_time_msc);
        let mut data_updated = false;

        // 1. Direct JFX Quote Extraction
        let jfx_quote = {
            let bid = val.get("jfx_bid").and_then(|v| v.as_f64());
            let ask = val.get("jfx_ask").and_then(|v| v.as_f64());
            match (bid, ask) {
                (Some(b), Some(a)) if b > 0.0 && a >= b => {
                    data_updated = true;
                    Some((b, a, target_utc_ms))
                }
                _ => None,
            }
        };

        // 2. Positions & History Overlay Sync
        if val.get("positions").is_some() || val.get("history").is_some() {
            let mut positions: Vec<ReplayTrade> = val
                .get("positions")
                .and_then(|p| serde_json::from_value(p.clone()).ok())
                .unwrap_or_default();
            let mut history: Vec<ReplayTrade> = val
                .get("history")
                .and_then(|h| serde_json::from_value(h.clone()).ok())
                .unwrap_or_default();

            for p in &mut positions {
                p.open_utc_ms = mt5_to_utc_ms(p.open_time_msc);
                p.close_utc_ms = p.close_time_msc.map(mt5_to_utc_ms);
            }
            for h in &mut history {
                h.open_utc_ms = mt5_to_utc_ms(h.open_time_msc);
                h.close_utc_ms = h.close_time_msc.map(mt5_to_utc_ms);
            }

            let mut store = trade_store.write();
            store.open_positions = positions;
            store.history = history;
            data_updated = true;
        }

        // 3. Adaptive SEEK Detection with Hysteresis
        let is_seek = if self.last_observed_mt5_ms == 0 {
            true
        } else if !is_playing {
            (virtual_time_msc - self.last_observed_mt5_ms).abs() > 200
        } else {
            let time_diff = virtual_time_msc - self.last_observed_mt5_ms;
            let forward_threshold = (multiplier * 2000.0).max(3000.0) as i64;
            // Negative threshold: -250ms deadband absorbs micro-clamps (Graceful Slowdown)
            time_diff < -250 || time_diff > forward_threshold
        };

        self.last_observed_mt5_ms = virtual_time_msc;

        let action = if is_seek {
            ArbiterAction::Seek {
                target_utc_ms,
                is_playing,
                multiplier,
            }
        } else {
            // Apply PLL phase locking during regular playback
            clock.sync_phase(target_utc_ms);
            clock.set_playing(is_playing);
            clock.set_multiplier(multiplier);

            ArbiterAction::PlaybackUpdate {
                target_utc_ms,
                is_playing,
                multiplier,
                data_updated,
            }
        };

        (action, jfx_quote)
    }

    /// Read last observed MT5 timestamp.
    pub fn last_observed_mt5_ms(&self) -> i64 {
        self.last_observed_mt5_ms
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::types::RunId;

    #[test]
    fn test_hysteresis_ignores_graceful_slowdown() {
        let clock = VirtualClock::new(RunId::new_random(), 1_000_000);
        let trade_store = Arc::new(RwLock::new(ReplayTradeStore::default()));
        let mut arbiter = SyncArbiter::new();

        // 1. Initial message is SEEK
        let v1 = serde_json::json!({
            "virtual_time_msc": 100_000,
            "is_playing": true,
            "multiplier": 1.0
        });
        let (action, _) = arbiter.evaluate(&v1, &clock, &trade_store);
        assert!(matches!(action, ArbiterAction::Seek { .. }));

        // 2. Small forward movement -> PlaybackUpdate
        let v2 = serde_json::json!({
            "virtual_time_msc": 100_050,
            "is_playing": true,
            "multiplier": 1.0
        });
        let (action, _) = arbiter.evaluate(&v2, &clock, &trade_store);
        assert!(matches!(action, ArbiterAction::PlaybackUpdate { .. }));

        // 3. Graceful Slowdown micro-clamp (-40ms) -> Must NOT be SEEK!
        let v3 = serde_json::json!({
            "virtual_time_msc": 100_010,
            "is_playing": true,
            "multiplier": 1.0
        });
        let (action, _) = arbiter.evaluate(&v3, &clock, &trade_store);
        assert!(matches!(action, ArbiterAction::PlaybackUpdate { .. }), "Micro-clamp must not trigger SEEK");

        // 4. Genuine rewind (-500ms) -> Must trigger SEEK
        let v4 = serde_json::json!({
            "virtual_time_msc": 99_500,
            "is_playing": true,
            "multiplier": 1.0
        });
        let (action, _) = arbiter.evaluate(&v4, &clock, &trade_store);
        assert!(matches!(action, ArbiterAction::Seek { .. }), "Large backward jump must trigger SEEK");
    }
}
