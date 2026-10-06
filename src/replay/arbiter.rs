//! Synchronization Arbiter: Evaluates incoming messages from `TickReplay`,
//! enforces seek hysteresis to eliminate false resets during playback,
//! and coordinates clock phase locking.

use super::clock::VirtualClock;
use super::driver::mt5_to_utc_ms;
use crate::core::models::{ReplayTrade, ReplayTradeStore};
use parking_lot::RwLock;
use std::sync::Arc;

/// Actions determined by the `SyncArbiter` after evaluating external status.
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

/// Maximum historical trades retained for review overlay to minimize memory and CPU overhead.
pub const MAX_REPLAY_TRADE_HISTORY: usize = 20;

pub struct SyncArbiter {
    last_observed_mt5_ms: i64,
    last_observed_seek_epoch: u64,
    last_observed_trade_revision: u64,
}

impl Default for SyncArbiter {
    fn default() -> Self {
        Self::new()
    }
}

impl SyncArbiter {
    pub const fn new() -> Self {
        Self {
            last_observed_mt5_ms: 0,
            last_observed_seek_epoch: 0,
            last_observed_trade_revision: 0,
        }
    }

    /// Reset internal tracking (e.g. on new connection).
    pub const fn reset(&mut self) {
        self.last_observed_mt5_ms = 0;
        self.last_observed_seek_epoch = 0;
        self.last_observed_trade_revision = 0;
    }

    /// Evaluates raw JSON or typed status and returns the appropriate action.
    /// Incorporates a -250ms hysteresis deadband to prevent false SEEK resets
    /// caused by `TickReplay`'s Graceful Slowdown timestamp clamping.
    pub fn evaluate(
        &mut self,
        val: &serde_json::Value,
        clock: &VirtualClock,
        trade_store: &Arc<RwLock<ReplayTradeStore>>,
    ) -> (ArbiterAction, Option<(f64, f64, i64)>) {
        let virtual_time_msc = val.get("virtual_time_msc").and_then(serde_json::Value::as_i64).unwrap_or(0);
        if virtual_time_msc <= 0 {
            return (ArbiterAction::Ignore, None);
        }

        let is_playing = val.get("is_playing").and_then(serde_json::Value::as_bool).unwrap_or(false);
        let multiplier = val
            .get("multiplier")
            .and_then(|m| m.as_f64().or_else(|| m.as_str().and_then(|s| s.parse().ok())))
            .unwrap_or(1.0);

        let target_utc_ms = mt5_to_utc_ms(virtual_time_msc);
        let mut data_updated = false;

        // 1. Direct JFX Quote Extraction
        let jfx_quote = {
            let bid = val.get("jfx_bid").and_then(serde_json::Value::as_f64);
            let ask = val.get("jfx_ask").and_then(serde_json::Value::as_f64);
            match (bid, ask) {
                (Some(b), Some(a)) if b > 0.0 && a >= b => {
                    data_updated = true;
                    Some((b, a, target_utc_ms))
                }
                _ => None,
            }
        };

        // 2. Positions & History Overlay Sync (Optimized with trade_revision check)
        let trade_revision = val
            .get("trade_revision")
            .or_else(|| val.get("history_revision"))
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0);
        let trade_changed = trade_revision == 0 || trade_revision != self.last_observed_trade_revision;

        if trade_changed {
            let mut store = trade_store.write();
            let mut updated = false;

            if let Some(pos_val) = val.get("positions") {
                if let Ok(mut positions) = serde_json::from_value::<Vec<ReplayTrade>>(pos_val.clone()) {
                    for p in &mut positions {
                        p.open_utc_ms = mt5_to_utc_ms(p.open_time_msc);
                        p.close_utc_ms = p.close_time_msc.map(mt5_to_utc_ms);
                    }
                    store.open_positions = positions;
                    updated = true;
                }
            }

            if let Some(hist_val) = val.get("history") {
                if let Ok(mut history) = serde_json::from_value::<Vec<ReplayTrade>>(hist_val.clone()) {
                    for h in &mut history {
                        h.open_utc_ms = mt5_to_utc_ms(h.open_time_msc);
                        h.close_utc_ms = h.close_time_msc.map(mt5_to_utc_ms);
                    }
                    if history.len() > MAX_REPLAY_TRADE_HISTORY {
                        let excess = history.len() - MAX_REPLAY_TRADE_HISTORY;
                        history.drain(0..excess);
                    }
                    store.history = history;
                    updated = true;
                }
            }

            if updated {
                data_updated = true;
            }
            if trade_revision > 0 {
                self.last_observed_trade_revision = trade_revision;
            }
        }

        // 3. Adaptive SEEK Detection with seek_epoch check & Hysteresis fallback
        let seek_epoch = val.get("seek_epoch").and_then(serde_json::Value::as_u64).unwrap_or(0);
        let is_seek = if seek_epoch > 0 && self.last_observed_seek_epoch > 0 && seek_epoch != self.last_observed_seek_epoch {
            // 明示的 seek_epoch 変更による確定的シーク
            true
        } else if self.last_observed_mt5_ms == 0 {
            true
        } else if !is_playing {
            (virtual_time_msc - self.last_observed_mt5_ms).abs() > 200
        } else {
            let time_diff = virtual_time_msc - self.last_observed_mt5_ms;
            let forward_threshold = (multiplier * 2000.0).max(3000.0) as i64;
            // Negative threshold: -250ms deadband absorbs micro-clamps (Graceful Slowdown)
            time_diff < -250 || time_diff > forward_threshold
        };

        if seek_epoch > 0 {
            self.last_observed_seek_epoch = seek_epoch;
        }
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
    pub const fn last_observed_mt5_ms(&self) -> i64 {
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

    #[test]
    fn test_seek_epoch_explicit_trigger() {
        let clock = VirtualClock::new(RunId::new_random(), 1_000_000);
        let trade_store = Arc::new(RwLock::new(ReplayTradeStore::default()));
        let mut arbiter = SyncArbiter::new();

        // 1. Initial message with epoch 1
        let v1 = serde_json::json!({
            "virtual_time_msc": 100_000,
            "is_playing": true,
            "multiplier": 1.0,
            "seek_epoch": 1
        });
        let (action, _) = arbiter.evaluate(&v1, &clock, &trade_store);
        assert!(matches!(action, ArbiterAction::Seek { .. }));

        // 2. Playback update same epoch
        let v2 = serde_json::json!({
            "virtual_time_msc": 100_010,
            "is_playing": true,
            "multiplier": 1.0,
            "seek_epoch": 1
        });
        let (action, _) = arbiter.evaluate(&v2, &clock, &trade_store);
        assert!(matches!(action, ArbiterAction::PlaybackUpdate { .. }));

        // 3. Increment seek_epoch -> Must trigger SEEK even if time difference is tiny (0ms)
        let v3 = serde_json::json!({
            "virtual_time_msc": 100_010,
            "is_playing": true,
            "multiplier": 1.0,
            "seek_epoch": 2
        });
        let (action, _) = arbiter.evaluate(&v3, &clock, &trade_store);
        assert!(matches!(action, ArbiterAction::Seek { .. }), "seek_epoch change must trigger SEEK");
    }

    #[test]
    fn test_trade_revision_preserves_history_when_omitted() {
        let clock = VirtualClock::new(RunId::new_random(), 1_000_000);
        let trade_store = Arc::new(RwLock::new(ReplayTradeStore::default()));
        let mut arbiter = SyncArbiter::new();

        // 1. Initial message with history and revision 1
        let v1 = serde_json::json!({
            "virtual_time_msc": 100_000,
            "trade_revision": 1,
            "history": [{
                "ticket": 101,
                "type": "BUY",
                "volume": 1.0,
                "open_price": 150.0,
                "open_time_msc": 100_000,
                "profit": 500.0
            }]
        });
        arbiter.evaluate(&v1, &clock, &trade_store);
        assert_eq!(trade_store.read().history.len(), 1);

        // 2. Next message without history (delta sync), same revision -> history must NOT be cleared!
        let v2 = serde_json::json!({
            "virtual_time_msc": 100_020,
            "trade_revision": 1,
            "is_playing": true
        });
        arbiter.evaluate(&v2, &clock, &trade_store);
        assert_eq!(trade_store.read().history.len(), 1, "History must be retained when omitted in same revision");

        // 3. New trade with revision 2
        let v3 = serde_json::json!({
            "virtual_time_msc": 100_050,
            "trade_revision": 2,
            "history": [
                {
                    "ticket": 101,
                    "type": "BUY",
                    "volume": 1.0,
                    "open_price": 150.0,
                    "open_time_msc": 100_000,
                    "profit": 500.0
                },
                {
                    "ticket": 102,
                    "type": "SELL",
                    "volume": 2.0,
                    "open_price": 150.5,
                    "open_time_msc": 100_040,
                    "profit": 1000.0
                }
            ]
        });
        arbiter.evaluate(&v3, &clock, &trade_store);
        assert_eq!(trade_store.read().history.len(), 2, "History must update on revision change");

        // 4. Overfill history beyond MAX_REPLAY_TRADE_HISTORY (e.g. 25 trades) -> must retain latest 20
        let items: Vec<_> = (1..=25)
            .map(|i| {
                serde_json::json!({
                    "ticket": i,
                    "type": "BUY",
                    "volume": 0.1,
                    "open_price": 150.0 + f64::from(i) * 0.01,
                    "open_time_msc": 100_000 + i * 1_000,
                    "profit": 10.0 * f64::from(i)
                })
            })
            .collect();
        let v4 = serde_json::json!({
            "virtual_time_msc": 200_000,
            "trade_revision": 3,
            "history": items
        });
        arbiter.evaluate(&v4, &clock, &trade_store);
        let history = trade_store.read().history.clone();
        assert_eq!(history.len(), MAX_REPLAY_TRADE_HISTORY);
        assert_eq!(history.first().unwrap().ticket, 6, "Oldest items (1..=5) must be pruned");
        assert_eq!(history.last().unwrap().ticket, 25, "Latest items must be preserved");
    }
}
