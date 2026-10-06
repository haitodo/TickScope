//! Replay Driver: WebSocket synchronization with `TickReplay` (<ws://127.0.0.1:49210>),
//! high-speed k-way merge tick streaming, and instant SEEK state rebuild.

use super::arbiter::{ArbiterAction, SyncArbiter};
use super::clock::VirtualClock;
use super::merge_stream::MergeStream;
use super::parquet_source::ReplayTick;
use super::pump::PlaybackPump;
use super::rebuilder::StateRebuilder;
use super::sync_client::WsSyncClient;
use crate::core::types::{BrokerId, MonoNs, RunId, SessionId, UtcMs};
use crate::protocol::{
    Frame, FramePayload, Header, IngressItem, ReceivedFrame, TickRecord, HEADER_FLAG_WARMUP,
    HEADER_LENGTH, MAGIC_TICK, MSG_TYPE_TICK_BATCH, PROTOCOL_VERSION, TICK_RECORD_LENGTH,
};
use crate::tick::engine::TickEngine;
use parking_lot::{Condvar, Mutex, RwLock};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;

pub const DEFAULT_SYNC_URL: &str = "ws://127.0.0.1:49210";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplaySyncStatus {
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub virtual_time_msc: i64,
    #[serde(default)]
    pub is_playing: bool,
    #[serde(default = "default_multiplier")]
    pub multiplier: f64,
    #[serde(default)]
    pub speed_mode: String,
}

const fn default_multiplier() -> f64 {
    1.0
}

/// Helper to convert MT5 server milliseconds to true UTC using standard US DST rules.
#[must_use]
pub fn mt5_to_utc_ms(mt5_ms: i64) -> i64 {
    let sec = mt5_ms / 1000;
    let dt = chrono::DateTime::from_timestamp(sec, 0).map(|d| d.naive_utc());
    let offset_hours = match dt {
        Some(d) => {
            use chrono::Datelike;
            if is_us_dst(d.year(), d.month(), d.day()) {
                3
            } else {
                2
            }
        }
        None => 2,
    };
    mt5_ms - offset_hours * 3600 * 1000
}

/// Helper to convert true UTC milliseconds to MT5 server milliseconds using standard US DST rules.
#[must_use]
pub fn utc_to_mt5_ms(utc_ms: i64) -> i64 {
    let sec = utc_ms / 1000;
    let dt = chrono::DateTime::from_timestamp(sec, 0).map(|d| d.naive_utc());
    let offset_hours = match dt {
        Some(d) => {
            use chrono::Datelike;
            if is_us_dst(d.year(), d.month(), d.day()) {
                3
            } else {
                2
            }
        }
        None => 2,
    };
    utc_ms + offset_hours * 3600 * 1000
}

#[must_use]
pub fn is_us_dst(year: i32, month: u32, day: u32) -> bool {
    if !(3..=11).contains(&month) {
        return false;
    }
    if month > 3 && month < 11 {
        return true;
    }

    if month == 3 {
        // Second Sunday of March
        let march1_day = chrono::NaiveDate::from_ymd_opt(year, 3, 1).map_or(0, |d| {
            use chrono::Datelike;
            d.weekday().num_days_from_sunday()
        });
        let second_sunday = 1
            + (if march1_day == 0 {
                7
            } else {
                7 - march1_day + 7
            });
        day >= second_sunday
    } else if month == 11 {
        // First Sunday of November
        let nov1_day = chrono::NaiveDate::from_ymd_opt(year, 11, 1).map_or(0, |d| {
            use chrono::Datelike;
            d.weekday().num_days_from_sunday()
        });
        let first_sunday = 1 + (if nov1_day == 0 { 0 } else { 7 - nov1_day });
        day < first_sunday
    } else {
        false
    }
}

pub struct ReplayDriver {
    pub run_id: RunId,
    pub clock: VirtualClock,
    pub engine: Arc<Mutex<TickEngine>>,
    pub merge_stream: Arc<RwLock<MergeStream>>,
    pub running: Arc<AtomicBool>,
    pub tick_wake: Arc<(Mutex<bool>, Condvar)>,
    pub session_epoch: Arc<AtomicU64>,
    pub trade_store: Arc<RwLock<crate::core::models::ReplayTradeStore>>,
    pub arbiter: Arc<Mutex<SyncArbiter>>,
    ws_cmd_tx: Arc<Mutex<Option<tokio::sync::mpsc::UnboundedSender<String>>>>,
    threads: Vec<JoinHandle<()>>,
}

impl ReplayDriver {
    pub fn new(
        run_id: RunId,
        clock: VirtualClock,
        engine: Arc<Mutex<TickEngine>>,
        merge_stream: Arc<RwLock<MergeStream>>,
        tick_wake: Arc<(Mutex<bool>, Condvar)>,
        trade_store: Arc<RwLock<crate::core::models::ReplayTradeStore>>,
    ) -> Self {
        Self {
            run_id,
            clock,
            engine,
            merge_stream,
            running: Arc::new(AtomicBool::new(true)),
            tick_wake,
            session_epoch: Arc::new(AtomicU64::new(1)),
            trade_store,
            arbiter: Arc::new(Mutex::new(SyncArbiter::new())),
            ws_cmd_tx: Arc::new(Mutex::new(None)),
            threads: Vec::new(),
        }
    }

    /// Start the replay driver with background WebSocket listener and adaptive tick pump.
    pub fn start(&mut self, ws_url: String) {
        log::info!("[ReplayDriver] Starting ReplayDriver (ws: {ws_url})");

        // 1. Spawn High-Throughput Playback Pump
        let pump_handle = PlaybackPump::spawn(
            self.run_id,
            self.clock.clone(),
            self.engine.clone(),
            self.merge_stream.clone(),
            self.running.clone(),
            self.tick_wake.clone(),
            self.session_epoch.clone(),
        );
        self.threads.push(pump_handle);

        // 2. Spawn WebSocket Sync Client
        let run_id = self.run_id;
        let clock = self.clock.clone();
        let engine = self.engine.clone();
        let merge_stream = self.merge_stream.clone();
        let tick_wake = self.tick_wake.clone();
        let session_epoch = self.session_epoch.clone();
        let trade_store = self.trade_store.clone();
        let arbiter = self.arbiter.clone();

        let ws_handle = WsSyncClient::spawn(
            ws_url,
            self.running.clone(),
            self.ws_cmd_tx.clone(),
            move |text| {
                Self::handle_ws_message(
                    text,
                    run_id,
                    &clock,
                    &engine,
                    &merge_stream,
                    &tick_wake,
                    &session_epoch,
                    &trade_store,
                    &arbiter,
                );
            },
        );
        self.threads.push(ws_handle);
    }

    /// Send a command string (e.g. PLAY, PAUSE, SEEK) to `TickReplay` WebSocket server.
    pub fn send_command(&self, cmd: &str) {
        if let Some(tx) = &*self.ws_cmd_tx.lock() {
            let _ = tx.send(cmd.to_string());
        }
    }

    /// Process incoming status message from `TickReplay` WebSocket server.
    fn handle_ws_message(
        text: &str,
        run_id: RunId,
        clock: &VirtualClock,
        engine: &Arc<Mutex<TickEngine>>,
        merge_stream: &Arc<RwLock<MergeStream>>,
        tick_wake: &Arc<(Mutex<bool>, Condvar)>,
        session_epoch: &Arc<AtomicU64>,
        trade_store: &Arc<RwLock<crate::core::models::ReplayTradeStore>>,
        arbiter: &Arc<Mutex<SyncArbiter>>,
    ) {
        let val: serde_json::Value = match serde_json::from_str(text) {
            Ok(v) => v,
            Err(_) => return,
        };

        let (action, jfx_quote) = arbiter.lock().evaluate(&val, clock, trade_store);

        // Handle JFX Direct Quote Synchronization (only if JFX is not streamed via parquet merge stream)
        if let Some((jfx_bid, jfx_ask, target_utc_ms)) = jfx_quote {
            let mut eng = engine.lock();
            let jfx_id = eng
                .config
                .brokers
                .iter()
                .find(|b| b.name.eq_ignore_ascii_case("JFX"))
                .map_or(5, |b| b.id);
            let has_jfx_in_parquet = merge_stream
                .read()
                .sources
                .iter()
                .any(|s| s.broker_id == jfx_id);
            if !has_jfx_in_parquet {
                let target_mono = MonoNs((target_utc_ms.max(0) as u64).saturating_mul(1_000_000));
                eng.update_direct_quote(
                    jfx_id,
                    jfx_bid,
                    jfx_ask,
                    UtcMs(target_utc_ms),
                    target_mono,
                );
            }
        }

        match action {
            ArbiterAction::Seek {
                target_utc_ms,
                is_playing,
                multiplier,
            } => {
                let session = session_epoch.fetch_add(1, Ordering::SeqCst) + 1;
                StateRebuilder::rebuild_at(
                    target_utc_ms,
                    is_playing,
                    multiplier,
                    session,
                    run_id,
                    clock,
                    engine,
                    merge_stream,
                    tick_wake,
                );
            }
            ArbiterAction::PlaybackUpdate { data_updated, .. } => {
                if data_updated {
                    let (lock, cvar) = &**tick_wake;
                    let mut pending = lock.lock();
                    *pending = true;
                    cvar.notify_one();
                }
            }
            ArbiterAction::Ignore => {}
        }
    }

    /// Stop the driver and join threads.
    pub fn stop(&mut self) {
        self.running.store(false, Ordering::SeqCst);
        let (lock, cvar) = &*self.tick_wake;
        let mut pending = lock.lock();
        *pending = true;
        cvar.notify_all();
    }

    pub fn wait_for_shutdown(self) {
        for h in self.threads {
            let _ = h.join();
        }
    }
}

/// Helper function to create an `IngressItem::Frame` for batch tick ingestion.
#[must_use]
pub fn make_ingress_tick_batch(
    broker_id: BrokerId,
    session_id: SessionId,
    ticks: &[ReplayTick],
    start_seq: u64,
    is_warmup: bool,
    run_id: RunId,
) -> IngressItem {
    make_ingress_tick_batch_with_mono(
        broker_id, session_id, ticks, start_seq, is_warmup, run_id, None,
    )
}

/// Helper function to create an `IngressItem::Frame` with an optional explicit `rx_mono_ns` override.
/// Used during SEEK atomic state rebuild to anchor the final quote to `target_mono`, preventing false Stale flags.
#[must_use]
pub fn make_ingress_tick_batch_with_mono(
    broker_id: BrokerId,
    session_id: SessionId,
    ticks: &[ReplayTick],
    start_seq: u64,
    is_warmup: bool,
    run_id: RunId,
    rx_mono_override: Option<MonoNs>,
) -> IngressItem {
    let mut raw_ticks = Vec::with_capacity(ticks.len());
    let mut max_eff_utc = 0;
    for (idx, t) in ticks.iter().enumerate() {
        let eff = t.effective_utc_ms();
        if eff > max_eff_utc {
            max_eff_utc = eff;
        }
        raw_ticks.push(TickRecord {
            sequence: start_seq + idx as u64,
            broker_time_msc: t.mt5_ms,
            ea_elapsed_us: 0,
            bid: t.bid,
            ask: t.ask,
            last: (t.bid + t.ask) / 2.0,
            volume: 1,
            volume_real: 1.0,
            flags: 0,
            reserved: 0,
        });
    }

    let header_flags = if is_warmup { HEADER_FLAG_WARMUP } else { 0 };
    let frame = Frame {
        header: Header {
            magic: MAGIC_TICK,
            protocol_version: PROTOCOL_VERSION,
            message_type: MSG_TYPE_TICK_BATCH,
            header_length: HEADER_LENGTH,
            header_flags,
            broker_id,
            session_id,
            sequence_start: start_seq,
            tick_count: raw_ticks.len() as u32,
            payload_length: (raw_ticks.len() * TICK_RECORD_LENGTH) as u32,
        },
        payload: FramePayload::TickBatch(raw_ticks),
    };

    let rx_mono_ns = rx_mono_override
        .unwrap_or_else(|| MonoNs((max_eff_utc.max(0) as u64).saturating_mul(1_000_000)));
    let rx_unix_ns = Some(max_eff_utc.saturating_mul(1_000_000));

    IngressItem::Frame(ReceivedFrame {
        frame,
        raw_wire_bytes: Arc::new(Vec::new()),
        run_id,
        rx_mono_ns,
        rx_unix_ns,
        connection_generation: 1,
        frame_index: 1,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mt5_to_utc_winter_and_summer() {
        // Winter: 2026-01-15 12:00:00 MT5 -> UTC+2 -> subtract 2 hours
        let winter_mt5 = 1_768_478_400_000; // 2026-01-15 12:00:00 UTC+2
        let winter_utc = mt5_to_utc_ms(winter_mt5);
        assert_eq!(winter_mt5 - winter_utc, 2 * 3600 * 1000);
        assert_eq!(utc_to_mt5_ms(winter_utc), winter_mt5);

        // Summer: 2026-08-15 12:00:00 MT5 -> UTC+3 -> subtract 3 hours
        let summer_mt5 = 1_786_795_200_000; // 2026-08-15 12:00:00 UTC+3
        let summer_utc = mt5_to_utc_ms(summer_mt5);
        assert_eq!(summer_mt5 - summer_utc, 3 * 3600 * 1000);
        assert_eq!(utc_to_mt5_ms(summer_utc), summer_mt5);
    }
}
