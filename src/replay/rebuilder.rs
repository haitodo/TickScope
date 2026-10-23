//! State Rebuilder: Atomic state reconstruction and warm-up pipeline for SEEK operations.

use super::clock::VirtualClock;
use super::driver::{make_ingress_tick_batch, make_ingress_tick_batch_with_mono};
use super::merge_stream::MergeStream;
use super::parquet_source::ReplayTick;
use crate::core::types::{BrokerId, MonoNs, RunId, SessionId};
use crate::protocol::IngressItem;
use crate::tick::engine::TickEngine;
use parking_lot::{Condvar, Mutex, RwLock};
use std::collections::BTreeMap;
use std::sync::Arc;

#[derive(Debug, Clone, Copy)]
pub struct RebuildTarget {
    pub target_utc_ms: i64,
    pub is_playing: bool,
    pub multiplier: f64,
    pub session_id: SessionId,
    pub run_id: RunId,
}

pub struct ReplayHandles<'a> {
    pub clock: &'a VirtualClock,
    pub engine: &'a Arc<Mutex<TickEngine>>,
    pub merge_stream: &'a Arc<RwLock<MergeStream>>,
    pub tick_wake: &'a Arc<(Mutex<bool>, Condvar)>,
}

pub struct StateRebuilder;

impl StateRebuilder {
    /// Perform atomic state rebuild for the specified target timestamp.
    /// Ingests historical ticks covering the full retention window (e.g. 2 hours)
    /// to reconstruct Candlesticks across all timeframes (S1, S5, S10, M1) and indicators instantaneously.
    pub fn rebuild_at(target: RebuildTarget, handles: ReplayHandles<'_>) {
        let RebuildTarget {
            target_utc_ms,
            is_playing,
            multiplier,
            session_id,
            run_id,
        } = target;
        let ReplayHandles {
            clock,
            engine,
            merge_stream,
            tick_wake,
        } = handles;

        log::info!(
            "[StateRebuilder] Rebuilding state at target_utc: {target_utc_ms} (session: {session_id})"
        );

        // Freeze clock immediately so PlaybackPump pauses during state reconstruction
        clock.set_playing(false);

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

        // 4. Batch load warm-up ticks covering the full retention window to reconstruct
        // all candlestick timeframes (S1, S5, S10, M1) and indicators instantaneously.
        let max_retention_ms = eng
            .config
            .history
            .retentions
            .iter()
            .map(|r| r.period_ms.saturating_mul(r.slots as i64))
            .max()
            .unwrap_or(7_200_000)
            .max(7_200_000); // Minimum 2 hours to guarantee complete M1 120-slot coverage

        let warmup_from_utc = target_utc_ms.saturating_sub(max_retention_ms);
        let warmup_ticks = stream.get_warmup_ticks(warmup_from_utc, target_utc_ms);

        if !warmup_ticks.is_empty() {
            let mut broker_groups: BTreeMap<BrokerId, Vec<ReplayTick>> = BTreeMap::new();
            for t in &warmup_ticks {
                broker_groups.entry(t.broker_id).or_default().push(*t);
            }

            for (b_id, b_ticks) in broker_groups {
                let mut seq = eng
                    .channels
                    .get(&b_id)
                    .map_or(0, |c| c.ledger.expected_sequence());
                if b_ticks.len() > 1 {
                    let hist_ticks = &b_ticks[..b_ticks.len() - 1];
                    for chunk in hist_ticks.chunks(4096) {
                        let hist_item =
                            make_ingress_tick_batch(b_id, session_id, chunk, seq, true, run_id);
                        seq += chunk.len() as u64;
                        eng.on_ingress_item(hist_item);
                    }
                }
                // Final tick sent as is_warmup = false, anchored to target_mono so
                // latest_quotes and FreshnessState::Live are immediately established at the seek point
                let last_tick = &b_ticks[b_ticks.len() - 1..];
                let live_item = make_ingress_tick_batch_with_mono(
                    b_id,
                    session_id,
                    last_tick,
                    seq,
                    false,
                    run_id,
                    Some(target_mono),
                );
                eng.on_ingress_item(live_item);
            }

            // Reconstruct Realtime Quote Path history across the full screen width upon seek
            eng.rebuild_quote_history_from_replay_ticks(&warmup_ticks, target_utc_ms);
        }

        // 5. Advance watermark for all connected brokers to target_mono so all warmup frames drain
        eng.advance_connected_watermarks(target_mono);

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
    }
}
