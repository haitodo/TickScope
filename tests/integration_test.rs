//! Multi-broker End-to-End Integration Test: T-I01.

use std::io::Write;
use std::net::TcpStream;
use std::thread;
use std::time::Duration;
use tick_compare::contracts::config::{AppConfig, BrokerConfig};
use tick_compare::contracts::ports::SnapshotExchangePort;
use tick_compare::contracts::types::*;
use tick_compare::protocol::codec::encode_frame;
use tick_compare::runtime::coordinator::RuntimeCoordinator;

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
    config.display.repaint_hz = 100;

    // 3 Brokers
    config.brokers = vec![
        BrokerConfig {
            id: 1,
            name: "Broker1".to_string(),
            host: "127.0.0.1".to_string(),
            port: 39201,
            symbol: "USDJPY".to_string(),
            digits: 3,
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
            digits: 3,
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
            digits: 3,
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
