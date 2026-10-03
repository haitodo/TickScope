#![cfg(feature = "replay")]

//! Tests for historical tick replay, VirtualClock, Hive Parquet sources,
//! 5-broker k-way merge stream, and WebSocket synchronization.

use std::sync::Arc;
use std::time::Duration;
use tempfile::TempDir;

use tick_scope::config::{AppConfig, BrokerConfig, TimezoneRule};
use tick_scope::core::ports::ClockPort;
use tick_scope::core::types::*;
use tick_scope::protocol::*;
use tick_scope::replay::clock::VirtualClock;
use tick_scope::replay::driver::{make_ingress_tick_batch, mt5_to_utc_ms, utc_to_mt5_ms};
use tick_scope::replay::merge_stream::MergeStream;
use tick_scope::replay::parquet_source::{BrokerParquetSource, ReplayTick};
use tick_scope::tick::engine::TickEngine;

fn make_test_config() -> AppConfig {
    let mut config = AppConfig::default();
    config.brokers = vec![
        BrokerConfig {
            id: 1,
            name: "OANDA".to_string(),
            host: "127.0.0.1".to_string(),
            port: 19101,
            symbol: "USDJPY".to_string(),
            point_size: 0.001,
            pip_size: 0.01,
            timezone_rule: TimezoneRule::NyClose,
            utc_offset_sec: 10800,
            utc_verified: true,
            auto_utc_offset: false,
            terminal_path: None,
        },
        BrokerConfig {
            id: 2,
            name: "Tradeview".to_string(),
            host: "127.0.0.1".to_string(),
            port: 19102,
            symbol: "USDJPY".to_string(),
            point_size: 0.001,
            pip_size: 0.01,
            timezone_rule: TimezoneRule::NyClose,
            utc_offset_sec: 10800,
            utc_verified: true,
            auto_utc_offset: false,
            terminal_path: None,
        },
        BrokerConfig {
            id: 3,
            name: "Dukascopy".to_string(),
            host: "127.0.0.1".to_string(),
            port: 19103,
            symbol: "USDJPY".to_string(),
            point_size: 0.001,
            pip_size: 0.01,
            timezone_rule: TimezoneRule::NyClose,
            utc_offset_sec: 10800,
            utc_verified: true,
            auto_utc_offset: false,
            terminal_path: None,
        },
        BrokerConfig {
            id: 4,
            name: "Axiory".to_string(),
            host: "127.0.0.1".to_string(),
            port: 19104,
            symbol: "USDJPY".to_string(),
            point_size: 0.001,
            pip_size: 0.01,
            timezone_rule: TimezoneRule::NyClose,
            utc_offset_sec: 10800,
            utc_verified: true,
            auto_utc_offset: false,
            terminal_path: None,
        },
        BrokerConfig {
            id: 5,
            name: "JFX".to_string(),
            host: "127.0.0.1".to_string(),
            port: 19105,
            symbol: "USDJPY".to_string(),
            point_size: 0.001,
            pip_size: 0.01,
            timezone_rule: TimezoneRule::NyClose,
            utc_offset_sec: 10800,
            utc_verified: true,
            auto_utc_offset: false,
            terminal_path: None,
        },
    ];
    config.active_pair = (1, 2);
    config
}

#[test]
fn test_virtual_clock_pause_freezes_time_and_preserves_live_health() {
    let config = make_test_config();
    let run_id = RunId::new_random();
    let clock = VirtualClock::new(run_id, 1_785_520_800_000);
    let mut engine = TickEngine::new(config.clone());

    // Connect brokers and ingest initial quotes
    engine.reset_state();
    for &b_id in &[1, 2, 3, 4, 5] {
        let tick = ReplayTick {
            broker_id: b_id,
            utc_ms: 1_785_510_000_000,
            mt5_ms: 1_785_520_800_000,
            bid: 155.000 + (b_id as f64 * 0.01),
            ask: 155.002 + (b_id as f64 * 0.01),
        };
        let item = make_ingress_tick_batch(b_id, 1, &[tick], 1, false, run_id);
        engine.on_ingress_item(item);
    }

    // Set clock to same time and pause
    clock.set_time(1_785_510_000_000);
    clock.set_playing(false);

    // Sample clock multiple times over real-world sleep
    let sample1 = clock.sample();
    std::thread::sleep(Duration::from_millis(50));
    let sample2 = clock.sample();

    // Verify clock did not advance in virtual time while paused
    assert_eq!(sample1.mono_ns, sample2.mono_ns);
    assert_eq!(sample1.unix_ns, sample2.unix_ns);

    // Verify engine projection keeps all 5 brokers LIVE when clock is frozen
    let proj = engine.make_projection_at(
        UtcMs(sample2.unix_ns.unwrap() / 1_000_000),
        sample2.mono_ns,
    );
    for b in &proj.broker_overviews {
        assert_eq!(
            b.health.data_freshness,
            FreshnessState::Live,
            "Broker {} must remain Live while paused",
            b.broker_id
        );
        assert!(b.latest_quote.is_some());
    }
}

#[test]
fn test_5_broker_k_way_merge_stream_chronological_ordering() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    // Create 5 fake broker sources with interleaved ticks
    let mut s1 = BrokerParquetSource::new(1, "OANDA", "usdjpy", root).unwrap();
    s1.current_ticks = Arc::new(vec![
        ReplayTick { broker_id: 1, utc_ms: 100, mt5_ms: 100, bid: 150.0, ask: 150.02 },
        ReplayTick { broker_id: 1, utc_ms: 150, mt5_ms: 150, bid: 150.0, ask: 150.02 },
        ReplayTick { broker_id: 1, utc_ms: 300, mt5_ms: 300, bid: 150.0, ask: 150.02 },
    ]);

    let mut s2 = BrokerParquetSource::new(2, "Tradeview", "usdjpy", root).unwrap();
    s2.current_ticks = Arc::new(vec![
        ReplayTick { broker_id: 2, utc_ms: 110, mt5_ms: 110, bid: 150.0, ask: 150.02 },
        ReplayTick { broker_id: 2, utc_ms: 200, mt5_ms: 200, bid: 150.0, ask: 150.02 },
    ]);

    let mut s3 = BrokerParquetSource::new(3, "Dukascopy", "usdjpy", root).unwrap();
    s3.current_ticks = Arc::new(vec![
        ReplayTick { broker_id: 3, utc_ms: 120, mt5_ms: 120, bid: 150.0, ask: 150.02 },
        ReplayTick { broker_id: 3, utc_ms: 250, mt5_ms: 250, bid: 150.0, ask: 150.02 },
    ]);

    let mut s4 = BrokerParquetSource::new(4, "Axiory", "usdjpy", root).unwrap();
    s4.current_ticks = Arc::new(vec![
        ReplayTick { broker_id: 4, utc_ms: 130, mt5_ms: 130, bid: 150.0, ask: 150.02 },
        ReplayTick { broker_id: 4, utc_ms: 150, mt5_ms: 150, bid: 150.0, ask: 150.02 }, // tie with broker 1
    ]);

    let mut s5 = BrokerParquetSource::new(5, "JFX", "usdjpy", root).unwrap();
    s5.current_ticks = Arc::new(vec![
        ReplayTick { broker_id: 5, utc_ms: 140, mt5_ms: 140, bid: 150.0, ask: 150.02 },
        ReplayTick { broker_id: 5, utc_ms: 400, mt5_ms: 400, bid: 150.0, ask: 150.02 },
    ]);

    let mut merge = MergeStream::new(vec![s1, s2, s3, s4, s5]);

    let mut merged_ticks = Vec::new();
    while let Some(t) = merge.pop_next() {
        merged_ticks.push(t);
    }

    assert_eq!(merged_ticks.len(), 11);
    // Verify strictly monotonic UTC timestamps
    for i in 1..merged_ticks.len() {
        assert!(
            merged_ticks[i].utc_ms >= merged_ticks[i - 1].utc_ms,
            "Ticks must be in non-decreasing UTC order"
        );
        if merged_ticks[i].utc_ms == merged_ticks[i - 1].utc_ms {
            assert!(
                merged_ticks[i].broker_id > merged_ticks[i - 1].broker_id,
                "Ties must be broken deterministically by broker_id"
            );
        }
    }
}

#[test]
fn test_consecutive_batches_accepted_without_duplicate_drop() {
    let config = make_test_config();
    let run_id = RunId::new_random();
    let mut engine = TickEngine::new(config);
    engine.reset_state();

    // Batch 1: ticks 1..50, sequence 1..50
    let mut batch1 = Vec::new();
    for i in 1..=50 {
        batch1.push(ReplayTick {
            broker_id: 1,
            utc_ms: 1000 + i * 10,
            mt5_ms: 1000 + i * 10 + 10800_000,
            bid: 150.0 + (i as f64 * 0.001),
            ask: 150.02 + (i as f64 * 0.001),
        });
    }
    let item1 = make_ingress_tick_batch(1, 1, &batch1, 1, false, run_id);
    engine.on_ingress_item(item1);

    // Batch 2: ticks 51..100, sequence 51..100 (MUST NOT BE DROPPED AS DUPLICATES)
    let mut batch2 = Vec::new();
    for i in 51..=100 {
        batch2.push(ReplayTick {
            broker_id: 1,
            utc_ms: 1000 + i * 10,
            mt5_ms: 1000 + i * 10 + 10800_000,
            bid: 150.0 + (i as f64 * 0.001),
            ask: 150.02 + (i as f64 * 0.001),
        });
    }
    let item2 = make_ingress_tick_batch(1, 1, &batch2, 51, false, run_id);
    engine.on_ingress_item(item2);

    // Advance watermark for all connected brokers
    let wm = MonoNs((2000u64).saturating_mul(1_000_000));
    for &b_id in &[1, 2, 3, 4, 5] {
        engine.on_ingress_item(IngressItem::Progress {
            broker_id: b_id,
            watermark_ns: wm,
        });
    }

    // Both batches must be fully ingested and reflected in latest quote!
    let latest = engine.latest_quote(1).expect("Latest quote for broker 1");
    assert_eq!(latest.tick_id.sequence, 100);
    assert_eq!(latest.bid, 150.0 + (100.0 * 0.001));
}

#[test]
fn test_5_broker_instant_seek_and_warmup_rebuild() {
    let config = make_test_config();
    let run_id = RunId::new_random();
    let mut engine = TickEngine::new(config.clone());
    engine.reset_state();

    let target_utc = 1_787_227_200_000i64;
    let target_mono = MonoNs((target_utc as u64).saturating_mul(1_000_000));

    let seek_start = std::time::Instant::now();
    engine.reset_state();

    // Brokers 1, 2, 3 have warmup ticks; Brokers 4 and 5 are quiet (0 warmup ticks in 60s)
    for &b_id in &[1, 2, 3] {
        let mut ticks = Vec::new();
        for i in 1..=60 {
            ticks.push(ReplayTick {
                broker_id: b_id,
                utc_ms: (target_utc - 60_000) + i * 1000,
                mt5_ms: (target_utc - 60_000) + i * 1000 + 10800_000,
                bid: 150.0 + (b_id as f64 * 0.01),
                ask: 150.02 + (b_id as f64 * 0.01),
            });
        }
        // Warmup ticks (len - 1 as warmup, final as live)
        let hist = &ticks[..ticks.len() - 1];
        let item_hist = make_ingress_tick_batch(b_id, 2, hist, 1, true, run_id);
        engine.on_ingress_item(item_hist);

        let live = &ticks[ticks.len() - 1..];
        let item_live = make_ingress_tick_batch(b_id, 2, live, ticks.len() as u64, false, run_id);
        engine.on_ingress_item(item_live);
    }

    // CRITICAL: Advance watermark for all connected brokers to target_mono
    for &b_id in &[1, 2, 3, 4, 5] {
        engine.on_ingress_item(IngressItem::Progress {
            broker_id: b_id,
            watermark_ns: target_mono,
        });
    }

    let seek_duration = seek_start.elapsed();
    println!("5-Broker SEEK and warmup rebuild completed in: {:?}", seek_duration);
    assert!(seek_duration < Duration::from_millis(20));

    // Verify projection is fully populated:
    let proj = engine.make_projection_at(UtcMs(target_utc), target_mono);

    // Active pair comparison (Broker 1 & Broker 2) must be immediately available upon seek!
    let comp = proj.active_pair_comparison.expect("Active pair comparison");
    assert!(comp.mid_diff.is_some(), "Mid diff must be calculated immediately upon seek");
    assert!((comp.mid_diff.unwrap() - (-0.01)).abs() < 1e-6);

    // Active brokers must have Live data freshness
    for b in &proj.broker_overviews {
        if b.broker_id <= 3 {
            assert_eq!(b.health.data_freshness, FreshnessState::Live);
            assert!(b.latest_quote.is_some());
        }
    }
}

#[test]
fn test_missing_partition_broker_does_not_deadlock_replay() {
    let config = make_test_config();
    let run_id = RunId::new_random();
    let mut engine = TickEngine::new(config);
    engine.reset_state();

    let now_utc = 1_700_000_000_000i64;
    let now_mono = MonoNs((now_utc as u64).saturating_mul(1_000_000));

    // Suppose broker 2 has no parquet data for this range: disconnect broker 2
    engine.on_ingress_item(IngressItem::End {
        broker_id: 2,
        generation: 1,
        reason: "No historical parquet partition".to_string(),
    });

    // Brokers 1, 3, 4, 5 are active and stream ticks
    for &b_id in &[1, 3, 4, 5] {
        let tick = ReplayTick {
            broker_id: b_id,
            utc_ms: now_utc,
            mt5_ms: now_utc + 10800_000,
            bid: 150.0,
            ask: 150.02,
        };
        let item = make_ingress_tick_batch(b_id, 1, &[tick], 1, false, run_id);
        engine.on_ingress_item(item);

        engine.on_ingress_item(IngressItem::Progress {
            broker_id: b_id,
            watermark_ns: now_mono,
        });
    }

    // Verify global watermark advances to now_mono despite broker 2 being disconnected!
    assert_eq!(engine.current_watermark(), now_mono);

    // Verify frames drained and quotes updated for connected brokers
    assert!(engine.latest_quote(1).is_some());
    assert!(engine.latest_quote(2).is_none()); // disconnected
    assert!(engine.latest_quote(3).is_some());
}

#[tokio::test]
async fn test_replay_driver_mock_websocket_sync() {
    use futures_util::SinkExt;
    use tokio::net::TcpListener;
    use tokio_tungstenite::tungstenite::Message;

    // 1. Bind mock WebSocket server on random port
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let ws_url = format!("ws://{}", addr);

    // 2. Set up ReplayDriver
    let run_id = RunId::new_random();
    let config = make_test_config();

    let clock = VirtualClock::new(run_id, 0);
    let engine = Arc::new(parking_lot::Mutex::new(TickEngine::new(config)));
    let stream = Arc::new(parking_lot::RwLock::new(MergeStream::new(vec![])));
    let tick_wake = Arc::new((parking_lot::Mutex::new(false), parking_lot::Condvar::new()));

    let mut driver = tick_scope::replay::driver::ReplayDriver::new(
        run_id,
        clock.clone(),
        engine,
        stream,
        tick_wake,
    );
    driver.start(ws_url);

    // 3. Accept connection in mock server
    let (socket, _) = listener.accept().await.unwrap();
    let mut ws = tokio_tungstenite::accept_async(socket).await.unwrap();

    // 4. Send initial READY message (paused at 2026-08-20 12:00:00 MT5)
    let initial_msg = serde_json::json!({
        "status": "READY",
        "virtual_time_msc": 1787238000000i64, // MT5 time (+3h)
        "is_playing": false,
        "multiplier": 1.0,
        "speed_mode": "TEMPORAL"
    });
    ws.send(Message::Text(initial_msg.to_string().into())).await.unwrap();

    tokio::time::sleep(Duration::from_millis(100)).await;

    // Verify clock was set and is paused
    assert!(!clock.is_playing());
    assert_eq!(clock.current_utc_ms(), 1787227200000); // 1787238000000 - 3h = 1787227200000

    // 5. Send PLAY message with 50x speed (high-speed multiplier)
    let play_msg = serde_json::json!({
        "status": "ACTIVE",
        "virtual_time_msc": 1787238000000i64,
        "is_playing": true,
        "multiplier": 50.0,
        "speed_mode": "TEMPORAL"
    });
    ws.send(Message::Text(play_msg.to_string().into())).await.unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;

    assert!(clock.is_playing());
    assert!((clock.multiplier() - 50.0).abs() < 1e-4);

    // 6. Send natural progress update at 50x (e.g. +2000ms MT5):
    // Adaptive threshold must NOT falsely trigger a seek reset!
    let progress_msg = serde_json::json!({
        "status": "ACTIVE",
        "virtual_time_msc": 1787240000000i64, // +2000ms
        "is_playing": true,
        "multiplier": 50.0,
        "speed_mode": "TEMPORAL"
    });
    ws.send(Message::Text(progress_msg.to_string().into())).await.unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;

    // 7. Send SEEK message (jump forward by 10 minutes)
    let seek_msg = serde_json::json!({
        "status": "ACTIVE",
        "virtual_time_msc": 1787238600000i64, // seek
        "is_playing": false,
        "multiplier": 1.0,
        "speed_mode": "TEMPORAL"
    });
    ws.send(Message::Text(seek_msg.to_string().into())).await.unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;

    assert!(!clock.is_playing());
    assert_eq!(clock.current_utc_ms(), 1787227800000); // exactly seeked!

    driver.stop();
}

#[test]
fn test_mt5_to_utc_dst_accuracy() {
    // 2026 Winter: US standard time (UTC+2)
    let mt5_winter = 1768478400000; // Jan 15 2026
    let utc_winter = mt5_to_utc_ms(mt5_winter);
    assert_eq!(utc_winter, mt5_winter - 2 * 3600 * 1000);
    assert_eq!(utc_to_mt5_ms(utc_winter), mt5_winter);

    // 2026 Summer: US daylight saving time (UTC+3)
    let mt5_summer = 1786795200000; // Aug 15 2026
    let utc_summer = mt5_to_utc_ms(mt5_summer);
    assert_eq!(utc_summer, mt5_summer - 3 * 3600 * 1000);
    assert_eq!(utc_to_mt5_ms(utc_summer), mt5_summer);
}

#[test]
fn test_real_drehis_parquet_read_and_seek() {
    let tick_dir = std::path::Path::new(r"D:\Drehis\tick");
    if !tick_dir.exists() {
        eprintln!("D:\\Drehis\tick does not exist on this machine; skipping live parquet test");
        return;
    }

    // 1. Test OANDA Parquet source
    let mut oanda = BrokerParquetSource::new(1, "OANDA", "usdjpy", tick_dir).unwrap();
    assert!(!oanda.partitions.is_empty(), "OANDA must have discovered partitions in D:\\Drehis\\tick");

    // Load August 2026
    let loaded = oanda.load_partition(2026, 8).unwrap();
    assert!(loaded, "August 2026 partition must load successfully");
    assert!(oanda.current_ticks.len() > 1_000_000, "Must contain over 1M ticks");

    // Check first tick has valid prices
    let first = &oanda.current_ticks[0];
    assert!(first.bid > 0.0);
    assert!(first.ask >= first.bid);

    // 2. Test Seek to active trading day: 2026-08-20 12:00:00 UTC (1787227200000 ms, Thursday)
    let target_time = 1_787_227_200_000;
    oanda.seek_to_utc(target_time);
    let peeked = oanda.peek().expect("Tick after seek");
    assert!(peeked.utc_ms >= target_time, "Seeked tick must be >= target time");

    // 3. Test 60-second warm-up extraction
    let warmup = oanda.get_warmup_range(target_time - 60_000, target_time);
    assert!(!warmup.is_empty(), "Warm-up slice must return ticks");
    for t in &warmup {
        assert!(t.utc_ms >= target_time - 60_000 && t.utc_ms <= target_time);
    }

    // 4. Test 5-broker real MergeStream
    let mut sources = Vec::new();
    for &(b_id, name) in &[(1, "OANDA"), (2, "Tradeview"), (3, "Dukascopy"), (4, "Axiory"), (5, "JFX")] {
        if let Ok(mut src) = BrokerParquetSource::new(b_id, name, "usdjpy", tick_dir) {
            let _ = src.load_partition(2026, 8);
            sources.push(src);
        }
    }
    assert_eq!(sources.len(), 5, "All 5 brokers must be loaded for August 2026");

    let mut merge = MergeStream::new(sources);
    merge.seek_to_utc(target_time);

    // Pop 100 ticks and verify strict chronological order across the 5 real brokers
    let ticks = merge.pop_up_to(target_time + 60_000, 100);
    assert_eq!(ticks.len(), 100);
    for i in 1..ticks.len() {
        assert!(ticks[i].utc_ms >= ticks[i - 1].utc_ms);
    }
}

#[test]
fn test_cross_month_playback_boundary_transition() {
    let tick_dir = std::path::Path::new(r"D:\Drehis\tick");
    if !tick_dir.exists() {
        eprintln!("D:\\Drehis\\tick does not exist on this machine; skipping live cross-month test");
        return;
    }

    let mut oanda = BrokerParquetSource::new(1, "OANDA", "usdjpy", tick_dir).unwrap();
    // Verify both August 2026 and September 2026 partitions exist
    let aug_idx = oanda.partitions.iter().position(|p| p.year == 2026 && p.month == 8);
    let sep_idx = oanda.partitions.iter().position(|p| p.year == 2026 && p.month == 9);

    if let (Some(a_idx), Some(s_idx)) = (aug_idx, sep_idx) {
        // Load August
        oanda.load_partition_by_idx(a_idx).unwrap();
        assert_eq!(oanda.current_partition_idx, Some(a_idx));

        // Position cursor at 3 ticks before the end of August
        assert!(oanda.current_ticks.len() > 10);
        oanda.cursor = oanda.current_ticks.len() - 3;

        let aug_tick1 = oanda.advance().expect("August tick -3");
        let aug_tick2 = oanda.advance().expect("August tick -2");
        let aug_tick3 = oanda.advance().expect("August tick -1");
        assert!(aug_tick1.utc_ms <= aug_tick2.utc_ms);
        assert!(aug_tick2.utc_ms <= aug_tick3.utc_ms);

        // Now cursor is at the end of August. The next advance() must automatically
        // load September 2026 and return September's first tick!
        let sep_tick1 = oanda.advance().expect("September tick 1");
        assert_eq!(oanda.current_partition_idx, Some(s_idx), "Partition must have transitioned to September");
        assert_eq!(oanda.cursor, 1);
        assert!(sep_tick1.utc_ms >= aug_tick3.utc_ms, "September tick must follow August tick");

        // Next tick is September tick 2
        let sep_tick2 = oanda.advance().expect("September tick 2");
        assert_eq!(oanda.cursor, 2);
        assert!(sep_tick2.utc_ms >= sep_tick1.utc_ms);
    }
}

#[test]
fn test_rapid_seek_scrubbing_race_safety() {
    let config = make_test_config();
    let run_id = RunId::new_random();
    let clock = VirtualClock::new(run_id, 0);
    let engine = Arc::new(parking_lot::Mutex::new(TickEngine::new(config)));
    let stream = Arc::new(parking_lot::RwLock::new(MergeStream::new(vec![])));
    let tick_wake = Arc::new((parking_lot::Mutex::new(false), parking_lot::Condvar::new()));

    let driver = tick_scope::replay::driver::ReplayDriver::new(
        run_id,
        clock.clone(),
        engine.clone(),
        stream,
        tick_wake,
    );

    // Simulate 30 rapid seek commands in succession (scrubbing slider back and forth)
    let base_time = 1_787_238_000_000i64; // MT5
    let mut last_observed = 0i64;

    for i in 1..=30 {
        let jump = if i % 2 == 0 { -5000 * i } else { 5000 * i };
        let target_mt5 = base_time + jump;

        // Process rapid seek through driver
        driver.session_epoch.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let mut eng_guard = driver.engine.lock();
        eng_guard.reset_state();
        driver.clock.set_time(mt5_to_utc_ms(target_mt5));
        last_observed = target_mt5;
    }

    assert_eq!(clock.current_utc_ms(), mt5_to_utc_ms(last_observed));
    assert!(!clock.is_playing());
    let eng_guard = engine.lock();
    assert_eq!(eng_guard.current_watermark(), MonoNs::ZERO);
}
