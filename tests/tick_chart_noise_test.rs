#![cfg(feature = "replay")]

//! Regression test for the tick chart "zig-zag noise" after repeated +10M seeks.
//! Every broker value stored in `realtime_quote_points` must match that broker's
//! real tick series at that timestamp (no stale / duplicated / mis-timed values).

use std::collections::HashMap;
use std::sync::Arc;

use tick_scope::config::{AppConfig, BrokerConfig, TimezoneRule};
use tick_scope::core::types::*;
use tick_scope::protocol::*;
use tick_scope::replay::clock::VirtualClock;
use tick_scope::replay::pump::PlaybackPump;
use tick_scope::replay::merge_stream::MergeStream;
use tick_scope::replay::parquet_source::BrokerParquetSource;
use tick_scope::tick::engine::TickEngine;

const NAMES: [(u32, &str); 5] = [(1, "OANDA"), (2, "Tradeview"), (3, "Dukascopy"), (4, "Axiory"), (5, "JFX")];

fn make_config() -> AppConfig {
    let mut config = AppConfig::default();
    config.brokers = NAMES
        .iter()
        .map(|&(id, name)| BrokerConfig {
            id,
            name: name.to_string(),
            host: "127.0.0.1".to_string(),
            port: 19100 + id as u16,
            symbol: "USDJPY".to_string(),
            point_size: 0.001,
            pip_size: 0.01,
            timezone_rule: TimezoneRule::NyClose,
            utc_offset_sec: 10800,
            utc_verified: true,
            auto_utc_offset: false,
            terminal_path: None,
            receive_delay_ms: None,
        })
        .collect();
    config.active_pair = (1, 2);
    config
}

fn make_sources(dir: &std::path::Path) -> Vec<BrokerParquetSource> {
    let mut v = Vec::new();
    for &(id, name) in &NAMES {
        if let Ok(mut s) = BrokerParquetSource::new(id, name, "usdjpy", dir) {
            s = s.with_receive_delay_profile();
            let _ = s.load_partition(2026, 8);
            v.push(s);
        }
    }
    v
}

/// Returns (mismatched, total, `max_abs_err`) of broker values vs ground truth.
fn audit(
    points: &[tick_scope::core::models::RealtimeQuotePoint],
    truth: &HashMap<BrokerId, Vec<(i64, f64)>>,
) -> (usize, usize, f64, usize) {
    let mut bad = 0;
    let mut total = 0;
    let mut max_err = 0.0f64;
    let mut inversions = 0;
    for w in points.windows(2) {
        if w[0].mono_ns > w[1].mono_ns {
            inversions += 1;
        }
    }
    for p in points {
        let t_ms = (p.mono_ns.0 / 1_000_000) as i64;
        for (bid, mid) in &p.broker_mids {
            let Some(series) = truth.get(bid) else { continue };
            // latest truth tick with eff <= t_ms (+ small tolerance)
            let idx = series.partition_point(|(e, _)| *e <= t_ms + 25);
            if idx == 0 {
                continue;
            }
            total += 1;
            let err = (series[idx - 1].1 - mid).abs();
            // Allow a value from any of the last 3 ticks (merge tolerance)
            let lo = idx.saturating_sub(3);
            let best = series[lo..idx].iter().map(|(_, m)| (m - mid).abs()).fold(f64::MAX, f64::min);
            if best > 0.0005 {
                bad += 1;
                max_err = max_err.max(err);
            }
        }
    }
    (bad, total, max_err, inversions)
}

#[test]
fn test_quote_history_matches_truth_after_repeated_seeks() {
    let tick_dir = std::path::Path::new(r"D:\Drehis\tick");
    if !tick_dir.exists() {
        eprintln!("tick dir missing; skipping");
        return;
    }
    let config = make_config();
    let run_id = RunId::new_random();
    let clock = VirtualClock::new(run_id, 0);
    let engine = Arc::new(parking_lot::Mutex::new(TickEngine::new(config.clone())));
    let merge_stream = Arc::new(parking_lot::RwLock::new(MergeStream::new(make_sources(tick_dir))));
    let tick_wake = Arc::new((parking_lot::Mutex::new(false), parking_lot::Condvar::new()));

    // Ground-truth stream (independent cursor)
    let mut truth_stream = MergeStream::new(make_sources(tick_dir));
    let start = 1_787_227_200_000i64;
    let all = truth_stream.get_warmup_ticks(start - 600_000, start + 4_000_000);
    let mut truth: HashMap<BrokerId, Vec<(i64, f64)>> = HashMap::new();
    for t in &all {
        truth.entry(t.broker_id).or_default().push((t.effective_utc_ms(), (t.bid + t.ask) / 2.0));
    }
    let mut cur = start;
    for round in 0..4 {
        println!("=== round {round} seek to {cur} ===");
        tick_scope::replay::rebuilder::StateRebuilder::rebuild_at(
            cur, true, 1.0, round as u64 + 1, run_id, &clock, &engine, &merge_stream, &tick_wake,
        );
        {
            let eng = engine.lock();
            let proj = eng.make_projection_at(UtcMs(cur), MonoNs(cur as u64 * 1_000_000));
            let (bad, total, max_err, inv) = audit(&proj.realtime_quote_points, &truth);
            println!("  after seek: points={} bad={}/{} max_err={:.4} inversions={}", proj.realtime_quote_points.len(), bad, total, max_err, inv);
            assert_eq!(inv, 0);
            assert!(bad * 100 <= total, "after seek: {bad}/{total} broker values do not match real ticks (max err {max_err:.4})");
        }

        // Simulate PlaybackPump: a 2 s catch-up burst (WS clock phase-sync after a slow
        // rebuild) followed by 20 ms passes.
        let session = round as u64 + 1;
        let mut seqs: HashMap<BrokerId, u64> = HashMap::new();
        for step in 1..=250 {
            let now = if step == 1 { cur + 2000 } else { cur + 2000 + (step - 1) * 20 };
            let ticks = merge_stream.write().pop_up_to(now, 4096);
            let mut eng = engine.lock();
            PlaybackPump::dispatch_replay_ticks(&mut eng, &ticks, session, run_id, &mut seqs);
            for &(b, _) in &NAMES {
                eng.on_ingress_item(IngressItem::Progress { broker_id: b, watermark_ns: MonoNs(now as u64 * 1_000_000) });
            }
        }
        let now = cur + 2000 + 249 * 20;
        {
            let eng = engine.lock();
            let proj = eng.make_projection_at(UtcMs(now), MonoNs(now as u64 * 1_000_000));
            let (bad, total, max_err, inv) = audit(&proj.realtime_quote_points, &truth);
            println!("  after play: points={} bad={}/{} max_err={:.4} inversions={}", proj.realtime_quote_points.len(), bad, total, max_err, inv);
            assert_eq!(inv, 0);
            assert!(bad * 100 <= total, "after play: {bad}/{total} broker values do not match real ticks (max err {max_err:.4})");
        }
        cur += 600_000;
    }
}
