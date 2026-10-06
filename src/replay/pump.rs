//! Playback Pump: High-throughput adaptive dynamic batching tick pipeline.
//! Ingests chronological merged ticks from Parquet sources into `TickEngine` with zero latency lag.

use super::clock::VirtualClock;
use super::driver::make_ingress_tick_batch;
use super::merge_stream::MergeStream;
use crate::core::types::{BrokerId, MonoNs, RunId};
use crate::protocol::IngressItem;
use crate::tick::engine::TickEngine;
use parking_lot::{Condvar, Mutex, RwLock};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

pub const MIN_BATCH_SIZE: usize = 256;
pub const MAX_BATCH_SIZE: usize = 4096;

pub struct PlaybackPump;

impl PlaybackPump {
    /// Spawns the dedicated high-performance playback worker thread.
    pub fn spawn(
        run_id: RunId,
        clock: VirtualClock,
        engine: Arc<Mutex<TickEngine>>,
        merge_stream: Arc<RwLock<MergeStream>>,
        running: Arc<AtomicBool>,
        tick_wake: Arc<(Mutex<bool>, Condvar)>,
        session_epoch: Arc<AtomicU64>,
    ) -> thread::JoinHandle<()> {
        thread::spawn(move || {
            Self::run_loop(
                run_id,
                clock,
                engine,
                merge_stream,
                running,
                tick_wake,
                session_epoch,
            );
        })
    }

    /// Feeds chronologically merged ticks into the engine.
    ///
    /// The engine stamps every tick of a frame with the frame's `rx_mono_ns`, which is what the
    /// Realtime Quote Path uses as its X coordinate. Ticks are therefore only grouped when they
    /// share the same broker and the same effective receive time; grouping any further (e.g. a
    /// whole broker batch popped after a clock catch-up jump) would collapse many ticks onto a
    /// single X position and draw them as vertical zig-zag noise.
    pub fn dispatch_replay_ticks(
        eng: &mut TickEngine,
        ticks: &[super::parquet_source::ReplayTick],
        session: crate::core::types::SessionId,
        run_id: RunId,
        next_sequences: &mut HashMap<BrokerId, u64>,
    ) {
        let mut start = 0;
        while start < ticks.len() {
            let b_id = ticks[start].broker_id;
            let eff = ticks[start].effective_utc_ms();
            let mut end = start + 1;
            while end < ticks.len()
                && ticks[end].broker_id == b_id
                && ticks[end].effective_utc_ms() == eff
            {
                end += 1;
            }
            let group = &ticks[start..end];
            let seq = next_sequences.entry(b_id).or_insert_with(|| {
                eng.channels
                    .get(&b_id)
                    .map_or(0, |c| c.ledger.expected_sequence())
            });
            let item = make_ingress_tick_batch(b_id, session, group, *seq, false, run_id);
            *seq += group.len() as u64;
            eng.on_ingress_item(item);
            start = end;
        }
    }

    /// Adaptive dynamic batching loop.
    /// When behind virtual time (e.g. high multiplier or burst activity), pulls up to
    /// 4096 ticks per pass without sleeping to ensure instantaneous catch-up.
    /// Once caught up, performs cooperative minimal yields to maintain low CPU overhead.
    fn run_loop(
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
        let mut current_batch_limit = MIN_BATCH_SIZE;

        while running.load(Ordering::SeqCst) {
            let current_session = session_epoch.load(Ordering::SeqCst);
            if current_session != last_session {
                next_sequences.clear();
                last_session = current_session;
                current_batch_limit = MIN_BATCH_SIZE;
            }

            if !clock.is_playing() {
                thread::sleep(Duration::from_millis(10));
                continue;
            }

            let current_utc = clock.current_utc_ms();
            let ticks = {
                let mut stream = merge_stream.write();
                stream.pop_up_to(current_utc, current_batch_limit)
            };

            let post_pop_session = session_epoch.load(Ordering::SeqCst);
            if post_pop_session != current_session {
                // A seek occurred while popping ticks; discard stale ticks
                continue;
            }

            let tick_count = ticks.len();
            let has_more = tick_count >= current_batch_limit;

            // Dynamically scale batch limit: expand up to MAX_BATCH_SIZE when lagging,
            // back off to MIN_BATCH_SIZE when caught up.
            if has_more {
                current_batch_limit = (current_batch_limit * 2).min(MAX_BATCH_SIZE);
            } else {
                current_batch_limit = (current_batch_limit / 2).max(MIN_BATCH_SIZE);
            }

            let current_mono = MonoNs((current_utc.max(0) as u64).saturating_mul(1_000_000));

            let mut eng = engine.lock();
            if session_epoch.load(Ordering::SeqCst) != current_session {
                drop(eng);
                continue;
            }

            if !ticks.is_empty() {
                Self::dispatch_replay_ticks(
                    &mut eng,
                    &ticks,
                    current_session,
                    run_id,
                    &mut next_sequences,
                );
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

            // Signal snapshot publisher immediately to reflect latest quotes & candles
            let (lock, cvar) = &*tick_wake;
            let mut pending = lock.lock();
            *pending = true;
            cvar.notify_one();

            // When there are more ticks queued up to current_utc, do not sleep!
            // Immediately loop to drain remaining ticks with zero lag.
            if has_more {
                thread::yield_now();
            } else {
                // Caught up with virtual time: sleep briefly
                thread::sleep(Duration::from_millis(1));
            }
        }
    }
}
