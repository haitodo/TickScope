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
            receive_delay_ms: None,
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
            receive_delay_ms: None,
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
            receive_delay_ms: None,
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
            receive_delay_ms: None,
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
            receive_delay_ms: None,
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
fn test_merge_stream_receive_delay_ordering_and_seek() {
    let temp = TempDir::new().unwrap();
    let root = temp.path();

    // Domestic broker (e.g. JFX: delay = 20ms)
    let mut s_dom = BrokerParquetSource::new(5, "JFX", "usdjpy", root).unwrap()
        .with_receive_delay_ms(20);
    s_dom.current_ticks = Arc::new(vec![
        // utc 1100 -> effective 1120
        ReplayTick { broker_id: 5, utc_ms: 1100, mt5_ms: 1100, bid: 150.0, ask: 150.02 },
        // utc 1200 -> effective 1220
        ReplayTick { broker_id: 5, utc_ms: 1200, mt5_ms: 1200, bid: 150.0, ask: 150.02 },
    ]);

    // Overseas broker (e.g. Tradeview: delay = 180ms)
    let mut s_ovs = BrokerParquetSource::new(2, "Tradeview", "usdjpy", root).unwrap()
        .with_receive_delay_ms(180);
    s_ovs.current_ticks = Arc::new(vec![
        // utc 1000 -> effective 1180
        ReplayTick { broker_id: 2, utc_ms: 1000, mt5_ms: 1000, bid: 150.0, ask: 150.02 },
        // utc 1100 -> effective 1280
        ReplayTick { broker_id: 2, utc_ms: 1100, mt5_ms: 1100, bid: 150.0, ask: 150.02 },
    ]);

    let mut merge = MergeStream::new(vec![s_dom, s_ovs]);

    // 1. Physical latency ordering:
    // Even though Tradeview tick 1 was generated earlier (utc 1000 < 1100),
    // JFX tick arrives at 1120, while Tradeview arrives at 1180.
    // JFX must be popped FIRST!
    let t1 = merge.pop_next().expect("First tick");
    assert_eq!(t1.broker_id, 5, "Domestic broker must arrive first despite later server time");
    assert_eq!(t1.utc_ms, 1100);

    let t2 = merge.pop_next().expect("Second tick");
    assert_eq!(t2.broker_id, 2, "Overseas broker arrives second");
    assert_eq!(t2.utc_ms, 1000);

    let t3 = merge.pop_next().expect("Third tick");
    assert_eq!(t3.broker_id, 5); // 1220 vs 1280
    assert_eq!(t3.utc_ms, 1200);

    let t4 = merge.pop_next().expect("Fourth tick");
    assert_eq!(t4.broker_id, 2);
    assert_eq!(t4.utc_ms, 1100);

    // 2. Seeking with receive delay:
    // Seeking to effective time 1150:
    // - JFX: target_utc = 1150 - 20 = 1130 -> cursor at tick utc 1200 (eff 1220)
    // - Tradeview: target_utc = 1150 - 180 = 970 -> cursor at tick utc 1000 (eff 1180)
    merge.seek_to_utc(1150);
    let after_seek = merge.pop_next().expect("Next tick after seek to 1150");
    assert_eq!(after_seek.broker_id, 2, "Tradeview tick with arrival 1180 >= 1150 must NOT be skipped");
    assert_eq!(after_seek.utc_ms, 1000);
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
    let trade_store = Arc::new(parking_lot::RwLock::new(tick_scope::core::models::ReplayTradeStore::default()));

    let mut driver = tick_scope::replay::driver::ReplayDriver::new(
        run_id,
        clock.clone(),
        engine,
        stream,
        tick_wake,
        trade_store.clone(),
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
    let trade_store = Arc::new(parking_lot::RwLock::new(tick_scope::core::models::ReplayTradeStore::default()));

    let driver = tick_scope::replay::driver::ReplayDriver::new(
        run_id,
        clock.clone(),
        engine.clone(),
        stream,
        tick_wake,
        trade_store,
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

#[test]
fn test_simulate_replay_seek_and_playback_candles() {
    let tick_dir = std::path::Path::new(r"D:\Drehis\tick");
    if !tick_dir.exists() {
        eprintln!("D:\\Drehis\\tick does not exist on this machine; skipping simulation test");
        return;
    }

    let config = make_test_config();
    let run_id = RunId::new_random();
    let clock = VirtualClock::new(run_id, 0);
    let engine = Arc::new(parking_lot::Mutex::new(TickEngine::new(config.clone())));

    let mut sources = Vec::new();
    for &(b_id, name) in &[(1, "OANDA"), (2, "Tradeview"), (3, "Dukascopy"), (4, "Axiory"), (5, "JFX")] {
        if let Ok(mut src) = BrokerParquetSource::new(b_id, name, "usdjpy", tick_dir) {
            let _ = src.load_partition(2026, 8);
            sources.push(src);
        }
    }
    let merge_stream = Arc::new(parking_lot::RwLock::new(MergeStream::new(sources)));
    let tick_wake = Arc::new((parking_lot::Mutex::new(false), parking_lot::Condvar::new()));

    // 1. Initial SEEK at 2026-08-20 12:00:00 UTC (1787227200000)
    let start_utc_ms = 1_787_227_200_000i64;
    println!("\n=== STEP 1: Rebuilding state at start_utc_ms: {} ===", start_utc_ms);
    tick_scope::replay::rebuilder::StateRebuilder::rebuild_at(
        start_utc_ms,
        true,
        1.0,
        1,
        run_id,
        &clock,
        &engine,
        &merge_stream,
        &tick_wake,
    );

    // Check projection immediately after rebuild
    {
        let eng = engine.lock();
        let proj = eng.make_projection_at(UtcMs(start_utc_ms), MonoNs(start_utc_ms as u64 * 1_000_000));
        println!("Projection after rebuild:");
        for bo in &proj.broker_overviews {
            println!("  Broker {}: quote={:?}, health={:?}", bo.broker_id, bo.latest_quote.is_some(), bo.health.data_freshness);
        }
        for (&period, cv) in &proj.candle_views {
            for (bid, slots) in &cv.slots_by_broker {
                let populated = slots.iter().filter(|s| s.ohlc.is_some()).count();
                println!("  Period {}ms, Broker {}: {} / {} slots populated", period, bid, populated, slots.len());
            }
        }
    }

    // 2. Play 5 seconds forward: pop ticks and feed
    let mut current_utc = start_utc_ms;
    for sec in 1..=5 {
        current_utc += 1000;
        clock.set_time(current_utc);
        let ticks = {
            let mut st = merge_stream.write();
            st.pop_up_to(current_utc, 1024)
        };
        println!("Second {}: popped {} ticks up to {}", sec, ticks.len(), current_utc);

        let mut eng = engine.lock();
        if !ticks.is_empty() {
            let mut broker_groups: std::collections::HashMap<BrokerId, Vec<ReplayTick>> = std::collections::HashMap::new();
            for t in ticks {
                broker_groups.entry(t.broker_id).or_default().push(t);
            }
            for (b_id, b_ticks) in broker_groups {
                let item = make_ingress_tick_batch(b_id, 1, &b_ticks, sec as u64 * 1000, false, run_id);
                eng.on_ingress_item(item);
            }
        }
        for &b_id in &[1, 2, 3, 4, 5] {
            eng.on_ingress_item(IngressItem::Progress {
                broker_id: b_id,
                watermark_ns: MonoNs(current_utc as u64 * 1_000_000),
            });
        }
        drop(eng);
    }

    // Check projection after 5 seconds of playback
    {
        let eng = engine.lock();
        let proj = eng.make_projection_at(UtcMs(current_utc), MonoNs(current_utc as u64 * 1_000_000));
        println!("\nProjection after 5s playback (current_utc: {}):", current_utc);
        for (&period, cv) in &proj.candle_views {
            for (bid, slots) in &cv.slots_by_broker {
                let populated = slots.iter().filter(|s| s.ohlc.is_some()).count();
                println!("  Period {}ms, Broker {}: {} / {} slots populated", period, bid, populated, slots.len());
            }
        }
    }

    // 3. JUMP +10 MINUTES (600,000ms)
    let jump_utc_ms = current_utc + 600_000;
    println!("\n=== STEP 3: JUMP +10M to {} ===", jump_utc_ms);
    tick_scope::replay::rebuilder::StateRebuilder::rebuild_at(
        jump_utc_ms,
        true,
        1.0,
        2,
        run_id,
        &clock,
        &engine,
        &merge_stream,
        &tick_wake,
    );

    // Check projection immediately after +10M jump
    {
        let eng = engine.lock();
        let proj = eng.make_projection_at(UtcMs(jump_utc_ms), MonoNs(jump_utc_ms as u64 * 1_000_000));
        println!("Projection immediately after +10M jump:");
        for bo in &proj.broker_overviews {
            println!("  Broker {}: quote={:?}, health={:?}", bo.broker_id, bo.latest_quote.is_some(), bo.health.data_freshness);
        }
        for (&period, cv) in &proj.candle_views {
            for (bid, slots) in &cv.slots_by_broker {
                let populated = slots.iter().filter(|s| s.ohlc.is_some()).count();
                println!("  Period {}ms, Broker {}: {} / {} slots populated", period, bid, populated, slots.len());
            }
        }
    }

    // 4. Play 5 seconds forward after jump
    current_utc = jump_utc_ms;
    for sec in 1..=5 {
        current_utc += 1000;
        clock.set_time(current_utc);
        let ticks = {
            let mut st = merge_stream.write();
            st.pop_up_to(current_utc, 1024)
        };
        println!("After jump Second {}: popped {} ticks up to {}", sec, ticks.len(), current_utc);

        let mut eng = engine.lock();
        if !ticks.is_empty() {
            let mut broker_groups: std::collections::HashMap<BrokerId, Vec<ReplayTick>> = std::collections::HashMap::new();
            for t in ticks {
                broker_groups.entry(t.broker_id).or_default().push(t);
            }
            for (b_id, b_ticks) in broker_groups {
                let item = make_ingress_tick_batch(b_id, 2, &b_ticks, 10000 + sec as u64 * 1000, false, run_id);
                eng.on_ingress_item(item);
            }
        }
        for &b_id in &[1, 2, 3, 4, 5] {
            eng.on_ingress_item(IngressItem::Progress {
                broker_id: b_id,
                watermark_ns: MonoNs(current_utc as u64 * 1_000_000),
            });
        }
        drop(eng);
    }

    // Check projection after 5 seconds of playback post-jump
    {
        let eng = engine.lock();
        let proj = eng.make_projection_at(UtcMs(current_utc), MonoNs(current_utc as u64 * 1_000_000));
        println!("\nProjection after 5s playback post-jump (current_utc: {}):", current_utc);
        for (&period, cv) in &proj.candle_views {
            for (bid, slots) in &cv.slots_by_broker {
                let populated = slots.iter().filter(|s| s.ohlc.is_some()).count();
                println!("  Period {}ms, Broker {}: {} / {} slots populated", period, bid, populated, slots.len());
            }
        }
    }
}

#[test]
fn test_simulate_august_3_live_issue() {
    let tick_dir = std::path::Path::new(r"D:\Drehis\tick");
    if !tick_dir.exists() {
        eprintln!("D:\\Drehis\\tick does not exist on this machine; skipping simulation test");
        return;
    }

    let config = make_test_config();
    let run_id = RunId::new_random();
    let clock = VirtualClock::new(run_id, 0);
    let engine = Arc::new(parking_lot::Mutex::new(TickEngine::new(config.clone())));

    let mut sources = Vec::new();
    for &(b_id, name) in &[(1, "OANDA"), (2, "Tradeview"), (3, "Dukascopy"), (4, "Axiory"), (5, "JFX")] {
        if let Ok(mut src) = BrokerParquetSource::new(b_id, name, "usdjpy", tick_dir) {
            let _ = src.load_partition(2026, 8);
            sources.push(src);
        }
    }
    let merge_stream = Arc::new(parking_lot::RwLock::new(MergeStream::new(sources)));
    let tick_wake = Arc::new((parking_lot::Mutex::new(false), parking_lot::Condvar::new()));

    // Target from live user session: 1785725353921 MT5 ms -> UTC
    let live_mt5_ms = 1785725353921i64;
    let start_utc_ms = mt5_to_utc_ms(live_mt5_ms);
    println!("\n=== LIVE TEST: Rebuilding state at start_utc_ms: {} (MT5: {}) ===", start_utc_ms, live_mt5_ms);
    tick_scope::replay::rebuilder::StateRebuilder::rebuild_at(
        start_utc_ms,
        true,
        1.0,
        1,
        run_id,
        &clock,
        &engine,
        &merge_stream,
        &tick_wake,
    );

    // Check engine status immediately after rebuild
    {
        let eng = engine.lock();
        let target_mono = MonoNs(start_utc_ms as u64 * 1_000_000);
        let proj = eng.make_projection_at(UtcMs(start_utc_ms), target_mono);
        println!("Projection after rebuild on Aug 3:");
        for bo in &proj.broker_overviews {
            println!(
                "  Broker {} ({}): connected={:?}, freshness={:?}, latest_quote={:?}",
                bo.broker_id,
                bo.name,
                bo.health.connection,
                bo.health.data_freshness,
                bo.latest_quote.map(|q| (q.bid, q.ask, q.rx_mono_ns))
            );
        }
        for (&period, cv) in &proj.candle_views {
            for (bid, slots) in &cv.slots_by_broker {
                let populated = slots.iter().filter(|s| s.ohlc.is_some()).count();
                println!("  Period {}ms, Broker {}: {} / {} slots populated", period, bid, populated, slots.len());
            }
        }
        for (bid, ch) in &eng.channels {
            println!(
                "  Channel {}: connected={}, watermark={}, pending_frames={}, expected_seq={}",
                bid, ch.is_connected, ch.watermark.0, ch.pending_frames.len(), ch.ledger.expected_sequence()
            );
        }
        println!("  Realtime quote history len: {}", eng.realtime_quote_history.len());
    }

    // Simulate PlaybackPump popping ticks
    let mut current_utc = start_utc_ms;
    let mut next_sequences: std::collections::HashMap<BrokerId, u64> = std::collections::HashMap::new();
    for sec in 1..=5 {
        current_utc += 1000;
        clock.set_time(current_utc);
        let current_mono = MonoNs(current_utc as u64 * 1_000_000);
        let ticks = {
            let mut st = merge_stream.write();
            st.pop_up_to(current_utc, 1024)
        };
        println!("Second {}: popped {} ticks up to {}", sec, ticks.len(), current_utc);
        for t in &ticks {
            println!("   -> popped tick: broker={}, utc_ms={}, mt5_ms={}, bid={}, ask={}", t.broker_id, t.utc_ms, t.mt5_ms, t.bid, t.ask);
        }

        let mut eng = engine.lock();
        if !ticks.is_empty() {
            let mut broker_groups: std::collections::HashMap<BrokerId, Vec<ReplayTick>> = std::collections::HashMap::new();
            for t in ticks {
                broker_groups.entry(t.broker_id).or_default().push(t);
            }
            for (b_id, b_ticks) in broker_groups {
                let seq = next_sequences.entry(b_id).or_insert_with(|| eng.channels.get(&b_id).map(|c| c.ledger.expected_sequence().max(1)).unwrap_or(1));
                let item = make_ingress_tick_batch(b_id, 1, &b_ticks, *seq, false, run_id);
                *seq += b_ticks.len() as u64;
                eng.on_ingress_item(item);
            }
        }

        let active_ids: Vec<BrokerId> = eng.channels.iter().filter(|(_, ch)| ch.is_connected).map(|(&id, _)| id).collect();
        for b_id in active_ids {
            eng.on_ingress_item(IngressItem::Progress {
                broker_id: b_id,
                watermark_ns: current_mono,
            });
        }

        for (bid, ch) in &eng.channels {
            println!(
                "  [Sec {}] Channel {}: connected={}, watermark={}, pending_frames={}",
                sec, bid, ch.is_connected, ch.watermark.0, ch.pending_frames.len()
            );
        }
        println!("  [Sec {}] Realtime quote history len: {}", sec, eng.realtime_quote_history.len());

        let proj = eng.make_projection_at(UtcMs(current_utc), current_mono);
        for (&period, cv) in &proj.candle_views {
            if period == 1000 || period == 60000 {
                for (bid, slots) in &cv.slots_by_broker {
                    let populated = slots.iter().filter(|s| s.ohlc.is_some()).count();
                    let latest_slot = slots.last().and_then(|s| s.ohlc.as_ref());
                    println!("    [Sec {}] Period {}ms, Broker {}: {} / {} slots populated, latest_slot_ohlc={:?}", sec, period, bid, populated, slots.len(), latest_slot.map(|o| o.close));
                }
            }
        }
        if sec == 5 {
            let s1_latest = proj.candle_views.get(&1000).unwrap().slots_by_broker.get(&1).unwrap().last().and_then(|s| s.ohlc.as_ref());
            assert!(s1_latest.is_some(), "S1 latest candle must be populated with incoming ticks");
        }
        drop(eng);
    }
}

