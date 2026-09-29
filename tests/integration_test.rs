//! Multi-broker End-to-End Integration Test: T-I01.

use std::io::Write;
use std::net::TcpStream;
use std::thread;
use std::time::Duration;
use tick_scope::config::{AppConfig, BrokerConfig};
use tick_scope::core::ports::SnapshotExchangePort;
use tick_scope::core::types::*;
use tick_scope::protocol::codec::encode_frame;
use tick_scope::runtime::coordinator::RuntimeCoordinator;

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
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    loop {
        let snap = coordinator.exchange.load_latest();
        let all_caught_up = [11, 12, 13].iter().all(|&bid| {
            snap.broker_overviews
                .iter()
                .find(|b| b.broker_id == bid)
                .and_then(|b| b.latest_quote.as_ref())
                .is_some_and(|q| q.tick_id.sequence == (burst_count - 1) as u64)
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

