//! Multi-broker End-to-End Integration Test: T-I01.

use std::io::Write;
use std::net::TcpStream;
use std::thread;
use std::time::Duration;
use tick_scope::config::{AppConfig, BrokerConfig, TimezoneRule};
use tick_scope::core::ports::SnapshotExchangePort;
use tick_scope::core::types::*;
use tick_scope::protocol::*;
use tick_scope::runtime::coordinator::RuntimeCoordinator;
use tick_scope::tick::engine::TickEngine;

fn send_test_tick(stream: &mut TcpStream, broker_id: BrokerId, seq: Sequence, time_msc: i64, bid: f64, ask: f64) {
    let frame = Frame {
        header: Header {
            magic: MAGIC_TICK,
            protocol_version: PROTOCOL_VERSION,
            message_type: MSG_TYPE_TICK_BATCH,
            header_length: HEADER_LENGTH,
            header_flags: 0,
            broker_id,
            session_id: 100 + broker_id as u64,
            sequence_start: seq,
            tick_count: 1,
            payload_length: 72,
        },
        payload: FramePayload::TickBatch(vec![TickRecord {
            sequence: seq,
            broker_time_msc: time_msc,
            ea_elapsed_us: 10,
            bid,
            ask,
            last: 0.0,
            volume: 1,
            volume_real: 1.0,
            flags: 0,
            reserved: 0,
        }]),
    };

    let bytes = encode_frame(&frame).unwrap();
    stream.write_all(&bytes).unwrap();
    stream.flush().unwrap();
}

#[test]
fn test_ti01_multi_broker_end_to_end_pipeline() {
    let mut config = AppConfig::default();
    config.logger.enabled = false; // In-memory for integration test
    config.mt5.auto_deploy = false;
    config.display.repaint_hz = 100;

    // 3 Brokers
    config.brokers = vec![
        BrokerConfig {
            id: 1,
            name: "Broker1".to_string(),
            host: "127.0.0.1".to_string(),
            port: 39201,
            symbol: "USDJPY".to_string(),
            point_size: 0.001,
            pip_size: 0.01,
            utc_offset_sec: 0,
            utc_verified: true,
            auto_utc_offset: true,
            timezone_rule: TimezoneRule::Fixed,
            terminal_path: None,
            receive_delay_ms: None,
        },
        BrokerConfig {
            id: 2,
            name: "Broker2".to_string(),
            host: "127.0.0.1".to_string(),
            port: 39202,
            symbol: "USDJPY.pro".to_string(),
            point_size: 0.001,
            pip_size: 0.01,
            utc_offset_sec: 0,
            utc_verified: true,
            auto_utc_offset: true,
            timezone_rule: TimezoneRule::Fixed,
            terminal_path: None,
            receive_delay_ms: None,
        },
        BrokerConfig {
            id: 3,
            name: "Broker3".to_string(),
            host: "127.0.0.1".to_string(),
            port: 39203,
            symbol: "USDJPY#".to_string(),
            point_size: 0.001,
            pip_size: 0.01,
            utc_offset_sec: 0,
            utc_verified: true,
            auto_utc_offset: true,
            timezone_rule: TimezoneRule::Fixed,
            terminal_path: None,
            receive_delay_ms: None,
        },

    ];
    config.active_pair = (1, 2);

    let mut coordinator = RuntimeCoordinator::new(config).expect("Coordinator init failed");
    thread::sleep(Duration::from_millis(150));

    // Connect 3 simulated MT5 terminals
    let mut client1 = TcpStream::connect("127.0.0.1:39201").expect("Connect broker 1");
    let mut client2 = TcpStream::connect("127.0.0.1:39202").expect("Connect broker 2");
    let mut client3 = TcpStream::connect("127.0.0.1:39203").expect("Connect broker 3");

    client1.set_nodelay(true).unwrap();
    client2.set_nodelay(true).unwrap();
    client3.set_nodelay(true).unwrap();

    thread::sleep(Duration::from_millis(50));

    // Send ticks from all 3 brokers
    send_test_tick(&mut client1, 1, 0, 1000, 155.000, 155.002);
    send_test_tick(&mut client2, 2, 0, 1005, 154.998, 155.001);
    send_test_tick(&mut client3, 3, 0, 1010, 155.003, 155.006);

    // Wait for pipeline processing and publisher tick
    thread::sleep(Duration::from_millis(150));

    let snap = coordinator.exchange.load_latest();
    assert_eq!(snap.broker_overviews.len(), 3, "All 3 brokers in overview");

    let b1 = snap.broker_overviews.iter().find(|b| b.broker_id == 1).unwrap();
    assert!(b1.latest_quote.is_some(), "Broker 1 has quote");
    assert_eq!(b1.latest_quote.as_ref().unwrap().bid, 155.000);

    let b2 = snap.broker_overviews.iter().find(|b| b.broker_id == 2).unwrap();
    assert!(b2.latest_quote.is_some(), "Broker 2 has quote");
    assert_eq!(b2.latest_quote.as_ref().unwrap().bid, 154.998);

    let b3 = snap.broker_overviews.iter().find(|b| b.broker_id == 3).unwrap();
    assert!(b3.latest_quote.is_some(), "Broker 3 has quote");
    assert_eq!(b3.latest_quote.as_ref().unwrap().bid, 155.003);

    // Verify Active Pair (1 vs 2) diff
    let pair_comp = snap.active_pair_comparison.as_ref().expect("Pair comparison exists");
    assert_eq!(pair_comp.broker_a, 1);
    assert_eq!(pair_comp.broker_b, 2);
    let bid_diff = pair_comp.bid_diff.expect("Bid diff computed");
    assert!((bid_diff - 0.002).abs() < 1e-6); // 155.000 - 154.998 = +0.002

    // Dynamically switch active pair to (1, 3)
    coordinator.set_active_pair((1, 3));
    send_test_tick(&mut client1, 1, 1, 2000, 155.010, 155.012);
    send_test_tick(&mut client3, 3, 1, 2005, 155.015, 155.018);

    thread::sleep(Duration::from_millis(150));

    let snap2 = coordinator.exchange.load_latest();
    assert_eq!(snap2.active_pair, (1, 3));
    let pair_comp2 = snap2.active_pair_comparison.as_ref().expect("Pair comparison 1 vs 3");
    assert_eq!(pair_comp2.broker_a, 1);
    assert_eq!(pair_comp2.broker_b, 3);
    let bid_diff2 = pair_comp2.bid_diff.expect("Bid diff for 1 vs 3");
    assert!((bid_diff2 - (-0.005)).abs() < 1e-6); // 155.010 - 155.015 = -0.005

    // Graceful stop
    drop(client1);
    drop(client2);
    drop(client3);

    coordinator.stop();
}

#[test]
fn test_ti02_high_frequency_burst_injection() {
    let mut config = AppConfig::default();
    config.logger.enabled = false;
    config.mt5.auto_deploy = false;
    config.protocol.ack_mode = "off".to_string();
    config.display.repaint_hz = 60;
    // Deliberately small channel capacity to exercise channel backpressure and unparking
    config.ingress.max_frames_per_broker = 64;
    config.brokers = vec![
        BrokerConfig {
            id: 11,
            name: "BurstBroker1".to_string(),
            host: "127.0.0.1".to_string(),
            port: 39311,
            symbol: "EURUSD".to_string(),
            point_size: 0.00001,
            pip_size: 0.0001,
            utc_offset_sec: 0,
            utc_verified: true,
            auto_utc_offset: false,
            timezone_rule: TimezoneRule::Fixed,
            terminal_path: None,
            receive_delay_ms: None,
        },
        BrokerConfig {
            id: 12,
            name: "BurstBroker2".to_string(),
            host: "127.0.0.1".to_string(),
            port: 39312,
            symbol: "EURUSD".to_string(),
            point_size: 0.00001,
            pip_size: 0.0001,
            utc_offset_sec: 0,
            utc_verified: true,
            auto_utc_offset: false,
            timezone_rule: TimezoneRule::Fixed,
            terminal_path: None,
            receive_delay_ms: None,
        },
        BrokerConfig {
            id: 13,
            name: "BurstBroker3".to_string(),
            host: "127.0.0.1".to_string(),
            port: 39313,
            symbol: "EURUSD".to_string(),
            point_size: 0.00001,
            pip_size: 0.0001,
            utc_offset_sec: 0,
            utc_verified: true,
            auto_utc_offset: false,
            timezone_rule: TimezoneRule::Fixed,
            terminal_path: None,
            receive_delay_ms: None,
        },

    ];
    config.active_pair = (11, 12);

    let mut coordinator = RuntimeCoordinator::new(config).expect("Coordinator init failed");
    thread::sleep(Duration::from_millis(150));

    let burst_count = 200; // 200 ticks * 3 brokers = 600 ticks, exceeding channel capacity of 64
    let handles: Vec<_> = [39311, 39312, 39313]
        .into_iter()
        .enumerate()
        .map(|(idx, port)| {
            let broker_id = (idx as u32) + 11;
            thread::spawn(move || {
                let mut client =
                    TcpStream::connect(format!("127.0.0.1:{port}")).expect("Connect broker");
                client.set_nodelay(true).unwrap();
                for seq in 0..burst_count {
                    let base_price = 1.0800 + (broker_id as f64 * 0.001);
                    send_test_tick(
                        &mut client,
                        broker_id,
                        seq as u64,
                        1000 + seq as i64,
                        base_price + (seq as f64 * 0.00001),
                        base_price + (seq as f64 * 0.00001) + 0.0001,
                    );
                }
                client
            })
        })
        .collect();

    let clients: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();

    // Allow pipeline to drain all queued ticks and publish a snapshot
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    loop {
        let snap = coordinator.exchange.load_latest();
        let all_caught_up = [11, 12, 13].iter().all(|&bid| {
            snap.broker_overviews
                .iter()
                .find(|b| b.broker_id == bid)
                .is_some_and(|b| {
                    b.health.total_ticks_received == burst_count as u64
                        && b.latest_quote.as_ref().is_some_and(|q| q.tick_id.sequence == (burst_count - 1) as u64)
                })
        });
        if all_caught_up || std::time::Instant::now() > deadline {
            break;
        }
        thread::sleep(Duration::from_millis(20));
    }

    let final_snap = coordinator.exchange.load_latest();
    for &bid in &[11, 12, 13] {
        let b = final_snap
            .broker_overviews
            .iter()
            .find(|b| b.broker_id == bid)
            .expect("Broker overview present");
        let q = b.latest_quote.as_ref().expect("Latest quote present");
        assert_eq!(
            q.tick_id.sequence,
            (burst_count - 1) as u64,
            "Broker {bid} must have caught up to the final burst sequence without dropped ticks"
        );
        assert_eq!(
            b.health.total_ticks_received,
            burst_count as u64,
            "Broker {bid} must have processed exactly all burst ticks"
        );
    }

    drop(clients);
    coordinator.stop();
}

#[test]
fn test_fast_path_quote_immediate_exposure_without_merge_wait() {
    use tick_scope::tick::engine::TickEngine;

    let mut config = AppConfig::default();
    config.brokers = vec![
        BrokerConfig {
            id: 21,
            name: "FastBroker".to_string(),
            host: "127.0.0.1".to_string(),
            port: 39301,
            symbol: "USDJPY".to_string(),
            point_size: 0.001,
            pip_size: 0.01,
            utc_offset_sec: 0,
            timezone_rule: TimezoneRule::Utc,
            utc_verified: true,
            auto_utc_offset: true,
            terminal_path: None,
            receive_delay_ms: None,
        },
        BrokerConfig {
            id: 22,
            name: "SlowBroker".to_string(),
            host: "127.0.0.1".to_string(),
            port: 39302,
            symbol: "USDJPY".to_string(),
            point_size: 0.001,
            pip_size: 0.01,
            utc_offset_sec: 0,
            timezone_rule: TimezoneRule::Utc,
            utc_verified: true,
            auto_utc_offset: true,
            terminal_path: None,
            receive_delay_ms: None,
        },
    ];

    let mut engine = TickEngine::new(config);

    // Connect both brokers
    engine.on_ingress_item(IngressItem::Connected {
        broker_id: 21,
        generation: 1,
        connected_at_mono: MonoNs(100),
    });
    engine.on_ingress_item(IngressItem::Connected {
        broker_id: 22,
        generation: 1,
        connected_at_mono: MonoNs(100),
    });

    // SlowBroker (22) has received NO frames and watermark is 0.
    // Therefore, calculate_global_watermark() is 0.

    // FastBroker (21) receives a live tick at rx_mono_ns = 50_000_000 (50ms).
    let tick_record = TickRecord {
        sequence: 42,
        broker_time_msc: 1700000000000,
        ea_elapsed_us: 1000,
        bid: 150.123,
        ask: 150.125,
        last: 0.0,
        volume: 1,
        volume_real: 1.0,
        flags: 0,
        reserved: 0,
    };
    let rf = ReceivedFrame {
        frame: Frame {
            header: Header {
                magic: MAGIC_TICK,
                protocol_version: PROTOCOL_VERSION,
                message_type: MSG_TYPE_TICK_BATCH,
                header_length: HEADER_LENGTH,
                header_flags: 0,
                broker_id: 21,
                session_id: 999,
                sequence_start: 42,
                tick_count: 1,
                payload_length: 72,
            },
            payload: FramePayload::TickBatch(vec![tick_record]),
        },
        raw_wire_bytes: std::sync::Arc::new(Vec::new()),
        run_id: RunId::new_random(),
        rx_mono_ns: MonoNs(50_000_000),
        rx_unix_ns: Some(1700000000000_000_000),
        connection_generation: 1,
        frame_index: 1,
    };

    engine.on_ingress_item(IngressItem::Frame(rf));

    // Verify Fast Path: FastBroker quote is immediately visible in projection
    // despite SlowBroker holding the global merge watermark at 0!
    let proj = engine.make_projection_at(UtcMs(1700000000000), MonoNs(50_000_000));
    let overview_21 = proj.broker_overviews.iter().find(|b| b.broker_id == 21).expect("Broker 21 overview present");
    let q = overview_21.latest_quote.as_ref().expect("Latest quote in overview must be immediately available via Fast Path");
    assert_eq!(q.tick_id.sequence, 42);
    assert_eq!(q.bid, 150.123);
    assert_eq!(q.ask, 150.125);
    assert_eq!(q.rx_mono_ns, MonoNs(50_000_000));
}

#[test]
fn test_warmup_ticks_populate_realtime_quote_history_across_past_window() {
    let mut config = AppConfig::default();
    config.brokers = vec![BrokerConfig {
        id: 10,
        name: "WarmupBroker".to_string(),
        host: "127.0.0.1".to_string(),
        port: 19110,
        symbol: "USDJPY".to_string(),
        point_size: 0.001,
        pip_size: 0.01,
        utc_offset_sec: 0,
        utc_verified: true,
        auto_utc_offset: false,
        timezone_rule: TimezoneRule::Fixed,
        terminal_path: None,
        receive_delay_ms: None,
    }];

    let mut engine = TickEngine::new(config);
    engine.on_ingress_item(IngressItem::Connected {
        broker_id: 10,
        generation: 1,
        connected_at_mono: MonoNs(100_000_000_000), // 100s mono
    });

    // Send a batch of warmup ticks spanning 30 seconds into the past
    // Current PC time: 1,700,000,030,000 ms (unix)
    let current_unix_ns = 1_700_000_030_000_000_000i64;
    let current_mono_ns = MonoNs(100_000_000_000); // 100s

    let mut warmup_ticks = Vec::new();
    for sec in 0..30 {
        warmup_ticks.push(TickRecord {
            sequence: 1 + sec as u64,
            broker_time_msc: 1_700_000_000_000 + sec * 1000, // 30s ago to now
            ea_elapsed_us: 10,
            bid: 150.0 + sec as f64 * 0.01,
            ask: 150.02 + sec as f64 * 0.01,
            last: 0.0,
            volume: 1,
            volume_real: 1.0,
            flags: 0,
            reserved: 0,
        });
    }

    let rf = ReceivedFrame {
        frame: Frame {
            header: Header {
                magic: MAGIC_TICK,
                protocol_version: PROTOCOL_VERSION,
                message_type: MSG_TYPE_TICK_BATCH,
                header_length: HEADER_LENGTH,
                header_flags: HEADER_FLAG_WARMUP, // WARMUP!
                broker_id: 10,
                session_id: 888,
                sequence_start: 1,
                tick_count: warmup_ticks.len() as u32,
                payload_length: (warmup_ticks.len() * TICK_RECORD_LENGTH) as u32,
            },
            payload: FramePayload::TickBatch(warmup_ticks),
        },
        raw_wire_bytes: std::sync::Arc::new(Vec::new()),
        run_id: RunId::new_random(),
        rx_mono_ns: current_mono_ns,
        rx_unix_ns: Some(current_unix_ns),
        connection_generation: 1,
        frame_index: 1,
    };

    engine.on_ingress_item(IngressItem::Frame(rf));

    let proj = engine.make_projection_at(UtcMs(1_700_000_030_000), current_mono_ns);

    // 1. Verify realtime_quote_points are populated
    assert_eq!(
        proj.realtime_quote_points.len(),
        30,
        "All 30 warmup ticks must be recorded into realtime quote points"
    );

    // 2. Verify timestamps span across the past (~30 seconds window)
    let first = proj.realtime_quote_points.first().unwrap();
    let last = proj.realtime_quote_points.last().unwrap();
    let span_sec = (last.mono_ns.0.saturating_sub(first.mono_ns.0)) as f64 / 1_000_000_000.0;
    assert!(
        (span_sec - 29.0).abs() < 1.5,
        "Warmup points must span ~29 seconds into the past, got {:.2}s",
        span_sec
    );

    // 3. Verify broker mid is populated in points
    assert!(first.broker_mids.contains_key(&10));
    assert!(last.broker_mids.contains_key(&10));

    // 4. Verify broker health freshness was NOT falsely marked Live by warmup ticks
    let overview = proj.broker_overviews.iter().find(|b| b.broker_id == 10).unwrap();
    assert_ne!(
        overview.health.data_freshness,
        FreshnessState::Live,
        "Warmup ticks must not mark data freshness as Live"
    );
}

#[test]
fn test_multi_broker_warmup_ticks_interleaved_chronologically() {
    let mut config = AppConfig::default();
    config.brokers = vec![
        BrokerConfig {
            id: 1,
            name: "BrokerA".to_string(),
            host: "127.0.0.1".to_string(),
            port: 19101,
            symbol: "USDJPY".to_string(),
            point_size: 0.001,
            pip_size: 0.01,
            utc_offset_sec: 0,
            utc_verified: true,
            auto_utc_offset: false,
            timezone_rule: TimezoneRule::Fixed,
            terminal_path: None,
            receive_delay_ms: None,
        },
        BrokerConfig {
            id: 2,
            name: "BrokerB".to_string(),
            host: "127.0.0.1".to_string(),
            port: 19102,
            symbol: "USDJPY".to_string(),
            point_size: 0.001,
            pip_size: 0.01,
            utc_offset_sec: 0,
            utc_verified: true,
            auto_utc_offset: false,
            timezone_rule: TimezoneRule::Fixed,
            terminal_path: None,
            receive_delay_ms: None,
        },
    ];

    let mut engine = TickEngine::new(config);
    engine.on_ingress_item(IngressItem::Connected {
        broker_id: 1,
        generation: 1,
        connected_at_mono: MonoNs(100_000_000_000),
    });
    engine.on_ingress_item(IngressItem::Connected {
        broker_id: 2,
        generation: 1,
        connected_at_mono: MonoNs(101_000_000_000),
    });

    let current_unix_ns = 1_700_000_030_000_000_000i64;
    let current_mono_ns = MonoNs(100_000_000_000);

    // 1. Broker A sends warmup ticks (seconds 0 to 20)
    let ticks_a: Vec<TickRecord> = (0..=20)
        .map(|sec| TickRecord {
            sequence: 1 + sec as u64,
            broker_time_msc: 1_700_000_000_000 + sec * 1000,
            ea_elapsed_us: 10,
            bid: 150.10,
            ask: 150.12,
            last: 0.0,
            volume: 1,
            volume_real: 1.0,
            flags: 0,
            reserved: 0,
        })
        .collect();

    engine.on_ingress_item(IngressItem::Frame(ReceivedFrame {
        frame: Frame {
            header: Header {
                magic: MAGIC_TICK,
                protocol_version: PROTOCOL_VERSION,
                message_type: MSG_TYPE_TICK_BATCH,
                header_length: HEADER_LENGTH,
                header_flags: HEADER_FLAG_WARMUP,
                broker_id: 1,
                session_id: 101,
                sequence_start: 1,
                tick_count: ticks_a.len() as u32,
                payload_length: (ticks_a.len() * TICK_RECORD_LENGTH) as u32,
            },
            payload: FramePayload::TickBatch(ticks_a),
        },
        raw_wire_bytes: std::sync::Arc::new(Vec::new()),
        run_id: RunId::new_random(),
        rx_mono_ns: current_mono_ns,
        rx_unix_ns: Some(current_unix_ns),
        connection_generation: 1,
        frame_index: 1,
    }));

    // 2. Broker B connects a second later and sends warmup ticks (seconds 5 to 25)
    let ticks_b: Vec<TickRecord> = (5..=25)
        .map(|sec| TickRecord {
            sequence: 1 + (sec - 5) as u64,
            broker_time_msc: 1_700_000_000_000 + sec * 1000,
            ea_elapsed_us: 10,
            bid: 150.20,
            ask: 150.22,
            last: 0.0,
            volume: 1,
            volume_real: 1.0,
            flags: 0,
            reserved: 0,
        })
        .collect();

    engine.on_ingress_item(IngressItem::Frame(ReceivedFrame {
        frame: Frame {
            header: Header {
                magic: MAGIC_TICK,
                protocol_version: PROTOCOL_VERSION,
                message_type: MSG_TYPE_TICK_BATCH,
                header_length: HEADER_LENGTH,
                header_flags: HEADER_FLAG_WARMUP,
                broker_id: 2,
                session_id: 102,
                sequence_start: 1,
                tick_count: ticks_b.len() as u32,
                payload_length: (ticks_b.len() * TICK_RECORD_LENGTH) as u32,
            },
            payload: FramePayload::TickBatch(ticks_b),
        },
        raw_wire_bytes: std::sync::Arc::new(Vec::new()),
        run_id: RunId::new_random(),
        rx_mono_ns: MonoNs(current_mono_ns.0 + 1_000_000_000), // 1s later
        rx_unix_ns: Some(current_unix_ns + 1_000_000_000),
        connection_generation: 1,
        frame_index: 1,
    }));

    let proj = engine.make_projection_at(UtcMs(1_700_000_031_000), MonoNs(current_mono_ns.0 + 1_000_000_000));

    // Verify all points remain strictly sorted by mono_ns
    for window in proj.realtime_quote_points.windows(2) {
        assert!(
            window[0].mono_ns <= window[1].mono_ns,
            "Realtime quote points must remain strictly sorted: {:?} vs {:?}",
            window[0].mono_ns,
            window[1].mono_ns
        );
    }

    // Verify that points around second 10 contain both Broker 1 and Broker 2
    let mid_points: Vec<_> = proj
        .realtime_quote_points
        .iter()
        .filter(|p| p.broker_mids.contains_key(&1) && p.broker_mids.contains_key(&2))
        .collect();
    assert!(
        !mid_points.is_empty(),
        "Overlapping warmup window must contain mid prices for both Broker 1 and Broker 2"
    );
}




