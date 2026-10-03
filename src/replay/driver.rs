//! Replay Driver: WebSocket synchronization with TickReplay (ws://127.0.0.1:49210),
//! high-speed k-way merge tick streaming, and instant SEEK state rebuild.

use super::clock::VirtualClock;
use super::merge_stream::MergeStream;
use super::parquet_source::ReplayTick;
use crate::core::types::{BrokerId, MonoNs, RunId, SessionId};
use crate::protocol::*;
use crate::tick::engine::TickEngine;
use futures_util::{SinkExt, StreamExt};
use parking_lot::{Condvar, Mutex, RwLock};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;
use tokio_tungstenite::tungstenite::Message;

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

fn default_multiplier() -> f64 {
    1.0
}

/// Helper to convert MT5 server milliseconds to true UTC using standard US DST rules.
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

pub fn is_us_dst(year: i32, month: u32, day: u32) -> bool {
    if !(3..=11).contains(&month) {
        return false;
    }
    if month > 3 && month < 11 {
        return true;
    }

    if month == 3 {
        // Second Sunday of March
        let march1_day = chrono::NaiveDate::from_ymd_opt(year, 3, 1)
            .map(|d| {
                use chrono::Datelike;
                d.weekday().num_days_from_sunday()
            })
            .unwrap_or(0);
        let second_sunday = 1 + (if march1_day == 0 { 7 } else { 7 - march1_day + 7 });
        day >= second_sunday
    } else if month == 11 {
        // First Sunday of November
        let nov1_day = chrono::NaiveDate::from_ymd_opt(year, 11, 1)
            .map(|d| {
                use chrono::Datelike;
                d.weekday().num_days_from_sunday()
            })
            .unwrap_or(0);
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
    ) -> Self {
        Self {
            run_id,
            clock,
            engine,
            merge_stream,
            running: Arc::new(AtomicBool::new(true)),
            tick_wake,
            session_epoch: Arc::new(AtomicU64::new(1)),
            ws_cmd_tx: Arc::new(Mutex::new(None)),
            threads: Vec::new(),
        }
    }

    /// Start the replay driver with background WebSocket listener and tick playback loop.
    pub fn start(&mut self, ws_url: String) {
        log::info!("[ReplayDriver] Starting ReplayDriver (ws: {})", ws_url);

        let run_id = self.run_id;
        let clock = self.clock.clone();
        let engine = self.engine.clone();
        let merge_stream = self.merge_stream.clone();
        let running = self.running.clone();
        let tick_wake = self.tick_wake.clone();
        let session_epoch = self.session_epoch.clone();

        // 1. Spawn Playback Worker Thread
        let clock_pb = clock.clone();
        let engine_pb = engine.clone();
        let merge_pb = merge_stream.clone();
        let run_pb = running.clone();
        let wake_pb = tick_wake.clone();
        let session_pb = session_epoch.clone();

        let pb_handle = thread::spawn(move || {
            Self::playback_worker_loop(
                run_id,
                clock_pb,
                engine_pb,
                merge_pb,
                run_pb,
                wake_pb,
                session_pb,
            );
        });
        self.threads.push(pb_handle);

        // 2. Spawn Tokio WebSocket Sync Client Thread
        let ws_cmd_tx_holder = self.ws_cmd_tx.clone();
        let clock_ws = clock.clone();
        let engine_ws = engine.clone();
        let merge_ws = merge_stream.clone();
        let run_ws = running.clone();
        let wake_ws = tick_wake.clone();
        let session_ws = session_epoch.clone();

        let ws_handle = thread::spawn(move || {
            let rt = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(e) => {
                    log::error!("[ReplayDriver] Failed to build Tokio runtime: {}", e);
                    return;
                }
            };

            rt.block_on(async move {
                Self::ws_sync_loop(
                    ws_url,
                    run_id,
                    clock_ws,
                    engine_ws,
                    merge_ws,
                    run_ws,
                    wake_ws,
                    session_ws,
                    ws_cmd_tx_holder,
                )
                .await;
            });
        });
        self.threads.push(ws_handle);
    }

    /// Send a command string (e.g. PLAY, PAUSE, SEEK) to TickReplay WebSocket server.
    pub fn send_command(&self, cmd: &str) {
        if let Some(tx) = &*self.ws_cmd_tx.lock() {
            let _ = tx.send(cmd.to_string());
        }
    }

    /// High-speed tick playback loop when `clock.is_playing() == true`.
    fn playback_worker_loop(
        run_id: RunId,
        clock: VirtualClock,
        engine: Arc<Mutex<TickEngine>>,
        merge_stream: Arc<RwLock<MergeStream>>,
        running: Arc<AtomicBool>,
        tick_wake: Arc<(Mutex<bool>, Condvar)>,
        session_epoch: Arc<AtomicU64>,
    ) {
        let mut next_sequences: HashMap<BrokerId, u64> = HashMap::new();
        let mut last_session = session_epoch.load(Ordering::SeqCst);

        while running.load(Ordering::SeqCst) {
            let current_session = session_epoch.load(Ordering::SeqCst);
            if current_session != last_session {
                next_sequences.clear();
                last_session = current_session;
            }

            if !clock.is_playing() {
                thread::sleep(Duration::from_millis(10));
                continue;
            }

            let current_utc = clock.current_utc_ms();
            let ticks = {
                let mut stream = merge_stream.write();
                stream.pop_up_to(current_utc, 256)
            };

            let post_pop_session = session_epoch.load(Ordering::SeqCst);
            if post_pop_session != current_session {
                // A seek occurred while popping ticks; discard stale ticks
                continue;
            }

            let current_mono = MonoNs((current_utc.max(0) as u64).saturating_mul(1_000_000));

            let mut eng = engine.lock();
            // Re-check session under lock to prevent any race condition
            if session_epoch.load(Ordering::SeqCst) != current_session {
                drop(eng);
                continue;
            }

            if !ticks.is_empty() {
                // Group ticks by broker for efficient WireFrame dispatch
                let mut broker_groups: HashMap<BrokerId, Vec<ReplayTick>> = HashMap::new();
                for t in ticks {
                    broker_groups.entry(t.broker_id).or_default().push(t);
                }

                for (b_id, b_ticks) in broker_groups {
                    let seq = next_sequences.entry(b_id).or_insert(1);
                    let ingress_item = make_ingress_tick_batch(
                        b_id,
                        current_session,
                        &b_ticks,
                        *seq,
                        false,
                        run_id,
                    );
                    *seq += b_ticks.len() as u64;
                    eng.on_ingress_item(ingress_item);
                }
            }

            // Advance watermark for all connected channels to current_mono so quiet brokers
            // do not stall the global watermark merge
            let active_broker_ids: Vec<BrokerId> = eng
                .channels
                .iter()
                .filter(|(_, ch)| ch.is_connected)
                .map(|(&id, _)| id)
                .collect();

            for b_id in active_broker_ids {
                eng.on_ingress_item(IngressItem::Progress {
                    broker_id: b_id,
                    watermark_ns: current_mono,
                });
            }

            drop(eng);

            // Wake publisher immediately to reflect latest quotes & chart updates
            let (lock, cvar) = &*tick_wake;
            let mut pending = lock.lock();
            *pending = true;
            cvar.notify_one();

            // Yield briefly when caught up with virtual time
            thread::yield_now();
            thread::sleep(Duration::from_millis(2));
        }
    }

    /// Background WebSocket loop connecting to ws://127.0.0.1:49210.
    async fn ws_sync_loop(
        ws_url: String,
        run_id: RunId,
        clock: VirtualClock,
        engine: Arc<Mutex<TickEngine>>,
        merge_stream: Arc<RwLock<MergeStream>>,
        running: Arc<AtomicBool>,
        tick_wake: Arc<(Mutex<bool>, Condvar)>,
        session_epoch: Arc<AtomicU64>,
        ws_cmd_tx_holder: Arc<Mutex<Option<tokio::sync::mpsc::UnboundedSender<String>>>>,
    ) {
        let mut last_observed_mt5_ms = 0i64;

        while running.load(Ordering::SeqCst) {
            log::info!("[ReplayDriver] Connecting to TickReplay WS at {}...", ws_url);
            match tokio_tungstenite::connect_async(&ws_url).await {
                Ok((ws_stream, _resp)) => {
                    log::info!("[ReplayDriver] Connected to TickReplay WebSocket server!");
                    let (mut write, mut read) = ws_stream.split();
                    let (cmd_tx, mut cmd_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
                    *ws_cmd_tx_holder.lock() = Some(cmd_tx);

                    loop {
                        tokio::select! {
                            msg_opt = read.next() => {
                                match msg_opt {
                                    Some(Ok(Message::Text(text))) => {
                                        Self::handle_ws_message(
                                            &text,
                                            run_id,
                                            &clock,
                                            &engine,
                                            &merge_stream,
                                            &tick_wake,
                                            &session_epoch,
                                            &mut last_observed_mt5_ms,
                                        );
                                    }
                                    Some(Ok(Message::Ping(p))) => {
                                        let _ = write.send(Message::Pong(p)).await;
                                    }
                                    Some(Ok(Message::Close(_))) | None => {
                                        log::warn!("[ReplayDriver] TickReplay WS connection closed.");
                                        break;
                                    }
                                    Some(Err(e)) => {
                                        log::warn!("[ReplayDriver] WS read error: {}", e);
                                        break;
                                    }
                                    _ => {}
                                }
                            }
                            cmd_opt = cmd_rx.recv() => {
                                match cmd_opt {
                                    Some(cmd) => {
                                        if let Err(e) = write.send(Message::Text(cmd.into())).await {
                                            log::warn!("[ReplayDriver] WS write error: {}", e);
                                            break;
                                        }
                                    }
                                    None => break,
                                }
                            }
                        }
                    }

                    *ws_cmd_tx_holder.lock() = None;
                }
                Err(e) => {
                    log::debug!("[ReplayDriver] WS connect failed ({}), retrying in 1s...", e);
                }
            }

            tokio::time::sleep(Duration::from_millis(1000)).await;
        }
    }

    /// Process status message received from TickReplay WebSocket server.
    fn handle_ws_message(
        text: &str,
        run_id: RunId,
        clock: &VirtualClock,
        engine: &Arc<Mutex<TickEngine>>,
        merge_stream: &Arc<RwLock<MergeStream>>,
        tick_wake: &Arc<(Mutex<bool>, Condvar)>,
        session_epoch: &Arc<AtomicU64>,
        last_observed_mt5_ms: &mut i64,
    ) {
        let val: serde_json::Value = match serde_json::from_str(text) {
            Ok(v) => v,
            Err(_) => return,
        };

        let virtual_time_msc = val.get("virtual_time_msc").and_then(|v| v.as_i64()).unwrap_or(0);
        if virtual_time_msc <= 0 {
            return;
        }

        let is_playing = val.get("is_playing").and_then(|p| p.as_bool()).unwrap_or(false);
        let multiplier = val.get("multiplier")
            .and_then(|m| m.as_f64().or_else(|| m.as_str().and_then(|s| s.parse().ok())))
            .unwrap_or(1.0);

        let target_utc_ms = mt5_to_utc_ms(virtual_time_msc);

        // Adaptive SEEK Detection:
        // - Initial message is always a seek/rebuild
        // - While paused, any movement > 200ms is a user seek
        // - While playing, any backward movement is a seek (or loop jump),
        //   and forward movement exceeding speed-scaled threshold is a seek
        let is_seek = if *last_observed_mt5_ms == 0 {
            true
        } else if !is_playing {
            (virtual_time_msc - *last_observed_mt5_ms).abs() > 200
        } else {
            let threshold = (multiplier * 2000.0).max(3000.0) as i64;
            virtual_time_msc < *last_observed_mt5_ms
                || (virtual_time_msc - *last_observed_mt5_ms) > threshold
        };

        if is_seek {
            let session = session_epoch.fetch_add(1, Ordering::SeqCst) + 1;
            log::info!(
                "[ReplayDriver] SEEK detected (target_mt5: {}, target_utc: {}, session: {}) -> Instant state rebuild",
                virtual_time_msc,
                target_utc_ms,
                session
            );

            // 1. Reset engine transient state
            let mut eng = engine.lock();
            eng.reset_state();

            // 2. Ensure partitions are loaded for target time
            let mut stream = merge_stream.write();
            let _ = stream.load_for_utc_ms(target_utc_ms);

            let target_mono = MonoNs((target_utc_ms.max(0) as u64).saturating_mul(1_000_000));

            // 3. Reconcile broker connection states
            for s in &stream.sources {
                if s.partitions.is_empty() || s.current_ticks.is_empty() {
                    eng.on_ingress_item(IngressItem::End {
                        broker_id: s.broker_id,
                        generation: 1,
                        reason: format!("No historical data for broker {}", s.broker_name),
                    });
                } else {
                    eng.on_ingress_item(IngressItem::Connected {
                        broker_id: s.broker_id,
                        generation: 1,
                        connected_at_mono: target_mono,
                    });
                }
            }

            // 4. Batch load 60-second warm-up ticks to instantaneously rebuild candlestick history & metrics
            let warmup_from_utc = target_utc_ms.saturating_sub(60_000);
            let warmup_ticks = stream.get_warmup_ticks(warmup_from_utc, target_utc_ms);

            if !warmup_ticks.is_empty() {
                let mut broker_groups: HashMap<BrokerId, Vec<ReplayTick>> = HashMap::new();
                for t in warmup_ticks {
                    broker_groups.entry(t.broker_id).or_default().push(t);
                }

                for (b_id, b_ticks) in broker_groups {
                    if b_ticks.len() > 1 {
                        let hist_ticks = &b_ticks[..b_ticks.len() - 1];
                        let hist_item = make_ingress_tick_batch(
                            b_id,
                            session,
                            hist_ticks,
                            1,
                            true,
                            run_id,
                        );
                        eng.on_ingress_item(hist_item);
                    }
                    // Final tick sent as is_warmup = false so latest_quotes and FreshnessState::Live are established
                    let last_tick = &b_ticks[b_ticks.len() - 1..];
                    let live_item = make_ingress_tick_batch(
                        b_id,
                        session,
                        last_tick,
                        b_ticks.len() as u64,
                        false,
                        run_id,
                    );
                    eng.on_ingress_item(live_item);
                }
            }

            // 5. Advance watermark for all connected brokers to target_mono so all warmup frames drain
            let connected_ids: Vec<BrokerId> = eng
                .channels
                .iter()
                .filter(|(_, ch)| ch.is_connected)
                .map(|(&id, _)| id)
                .collect();

            for b_id in connected_ids {
                eng.on_ingress_item(IngressItem::Progress {
                    broker_id: b_id,
                    watermark_ns: target_mono,
                });
            }

            drop(eng);

            // 6. Seek stream cursor to target timestamp
            stream.seek_to_utc(target_utc_ms);
            drop(stream);

            // 7. Update clock
            clock.set_time(target_utc_ms);
            clock.set_playing(is_playing);
            clock.set_multiplier(multiplier);

            // 8. Signal snapshot publisher immediately
            let (lock, cvar) = &**tick_wake;
            let mut pending = lock.lock();
            *pending = true;
            cvar.notify_one();
        } else {
            // Smooth progress update
            clock.set_playing(is_playing);
            clock.set_multiplier(multiplier);
        }

        *last_observed_mt5_ms = virtual_time_msc;
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
pub fn make_ingress_tick_batch(
    broker_id: BrokerId,
    session_id: SessionId,
    ticks: &[ReplayTick],
    start_seq: u64,
    is_warmup: bool,
    run_id: RunId,
) -> IngressItem {
    let mut raw_ticks = Vec::with_capacity(ticks.len());
    let mut max_utc = 0;
    for (idx, t) in ticks.iter().enumerate() {
        if t.utc_ms > max_utc {
            max_utc = t.utc_ms;
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

    let rx_mono_ns = MonoNs((max_utc as u64).saturating_mul(1_000_000));
    let rx_unix_ns = Some(max_utc.saturating_mul(1_000_000));

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
        let winter_mt5 = 1768478400000; // 2026-01-15 12:00:00 UTC+2
        let winter_utc = mt5_to_utc_ms(winter_mt5);
        assert_eq!(winter_mt5 - winter_utc, 2 * 3600 * 1000);
        assert_eq!(utc_to_mt5_ms(winter_utc), winter_mt5);

        // Summer: 2026-08-15 12:00:00 MT5 -> UTC+3 -> subtract 3 hours
        let summer_mt5 = 1786795200000; // 2026-08-15 12:00:00 UTC+3
        let summer_utc = mt5_to_utc_ms(summer_mt5);
        assert_eq!(summer_mt5 - summer_utc, 3 * 3600 * 1000);
        assert_eq!(utc_to_mt5_ms(summer_utc), summer_mt5);
    }
}
