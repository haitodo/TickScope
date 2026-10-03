//! Transport receiver test: tests connection, framing, monotonic timestamping, and ACK.

#[path = "support/mod.rs"]
mod support;

use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use support::*;
use tick_scope::config::{BrokerConfig, TimezoneRule};
use tick_scope::core::types::*;
use tick_scope::protocol::*;
use tick_scope::transport::tcp::TransportReceiver;

#[test]
fn test_transport_receiver_lifecycle_and_ack() {
    let clock = Arc::new(FakeClock::new(500_000_000, 1_700_000_000_000_000_000));
    let ingress = Arc::new(FakeIngressSink::new(100));

    let config = BrokerConfig {
        id: 1,
        name: "TestBroker".to_string(),
        host: "127.0.0.1".to_string(),
        port: 39101,
        symbol: "USDJPY".to_string(),
        point_size: 0.001,
        pip_size: 0.01,
        utc_offset_sec: 0,
        utc_verified: false,
        auto_utc_offset: true,
        timezone_rule: TimezoneRule::Fixed,
        terminal_path: None,
        receive_delay_ms: None,
    };


    let receiver = Arc::new(TransportReceiver::new(
        config,
        "forced".to_string(),
        clock.clone(),
        ingress.clone(),
    ));
    let rec_clone = receiver.clone();
    let thread_handle = thread::spawn(move || {
        rec_clone.run();
    });

    thread::sleep(Duration::from_millis(100));

    // Connect client
    let mut client = TcpStream::connect("127.0.0.1:39101").expect("Must connect to receiver");
    client.set_nodelay(true).unwrap();

    let frame = Frame {
        header: Header {
            magic: MAGIC_TICK,
            protocol_version: PROTOCOL_VERSION,
            message_type: MSG_TYPE_TICK_BATCH,
            header_length: HEADER_LENGTH,
            header_flags: 0,
            broker_id: 1,
            session_id: 99,
            sequence_start: 10,
            tick_count: 1,
            payload_length: 72,
        },
        payload: FramePayload::TickBatch(vec![TickRecord {
            sequence: 10,
            broker_time_msc: 1000,
            ea_elapsed_us: 100,
            bid: 155.123,
            ask: 155.125,
            last: 0.0,
            volume: 1,
            volume_real: 1.0,
            flags: 0,
            reserved: 0,
        }]),
    };

    let bytes = encode_frame(&frame).unwrap();
    client.write_all(&bytes).unwrap();
    client.flush().unwrap();

    // Read ACK
    let mut ack_buf = [0u8; 48];
    client.read_exact(&mut ack_buf).expect("Must receive 48-byte ACK");
    let magic = u32::from_le_bytes(ack_buf[0..4].try_into().unwrap());
    assert_eq!(magic, MAGIC_TICK);
    let msg_type = u16::from_le_bytes(ack_buf[6..8].try_into().unwrap());
    assert_eq!(msg_type, MSG_TYPE_BATCH_ACK);
    let seq_end = u64::from_le_bytes(ack_buf[40..48].try_into().unwrap());
    assert_eq!(seq_end, 10);

    drop(client);
    thread::sleep(Duration::from_millis(50));

    receiver.stop();
    let _ = thread_handle.join();

    let items = ingress.collected_items();
    let has_connected = items.iter().any(|it| matches!(it, IngressItem::Connected { broker_id: 1, .. }));
    let has_frame = items.iter().any(|it| matches!(it, IngressItem::Frame(_)));
    let has_end = items.iter().any(|it| matches!(it, IngressItem::End { broker_id: 1, .. }));

    assert!(has_connected, "Must contain Connected item");
    assert!(has_frame, "Must contain Frame item");
    assert!(has_end, "Must contain End item");
}

#[test]
fn test_transport_receiver_ack_mode_off() {
    let clock = Arc::new(FakeClock::new(500_000_000, 1_700_000_000_000_000_000));
    let ingress = Arc::new(FakeIngressSink::new(100));

    let config = BrokerConfig {
        id: 2,
        name: "TestBroker2".to_string(),
        host: "127.0.0.1".to_string(),
        port: 39102,
        symbol: "USDJPY".to_string(),
        point_size: 0.001,
        pip_size: 0.01,
        utc_offset_sec: 0,
        utc_verified: false,
        auto_utc_offset: true,
        timezone_rule: TimezoneRule::Fixed,
        terminal_path: None,
        receive_delay_ms: None,
    };

    let receiver = Arc::new(TransportReceiver::new(
        config,
        "off".to_string(),
        clock.clone(),
        ingress.clone(),
    ));
    let rec_clone = receiver.clone();
    let thread_handle = thread::spawn(move || {
        rec_clone.run();
    });

    thread::sleep(Duration::from_millis(100));

    let mut client = TcpStream::connect("127.0.0.1:39102").expect("Must connect to receiver");
    client.set_nodelay(true).unwrap();
    client.set_read_timeout(Some(Duration::from_millis(100))).unwrap();

    let frame = Frame {
        header: Header {
            magic: MAGIC_TICK,
            protocol_version: PROTOCOL_VERSION,
            message_type: MSG_TYPE_TICK_BATCH,
            header_length: HEADER_LENGTH,
            header_flags: 0,
            broker_id: 2,
            session_id: 100,
            sequence_start: 1,
            tick_count: 1,
            payload_length: 72,
        },
        payload: FramePayload::TickBatch(vec![TickRecord {
            sequence: 1,
            broker_time_msc: 1000,
            ea_elapsed_us: 100,
            bid: 155.123,
            ask: 155.125,
            last: 0.0,
            volume: 1,
            volume_real: 1.0,
            flags: 0,
            reserved: 0,
        }]),
    };

    let bytes = encode_frame(&frame).unwrap();
    client.write_all(&bytes).unwrap();
    client.flush().unwrap();

    // Verify NO ACK is received when ack_mode is "off"
    let mut ack_buf = [0u8; 48];
    let res = client.read_exact(&mut ack_buf);
    assert!(res.is_err(), "Must NOT receive ACK when ack_mode is 'off'");

    drop(client);
    thread::sleep(Duration::from_millis(50));

    receiver.stop();
    let _ = thread_handle.join();
}

#[test]
fn test_transport_receiver_multiple_reconnects_lifecycle() {
    let clock = Arc::new(FakeClock::new(500_000_000, 1_700_000_000_000_000_000));
    let ingress = Arc::new(FakeIngressSink::new(10_000));

    let config = BrokerConfig {
        id: 3,
        name: "TestBroker3".to_string(),
        host: "127.0.0.1".to_string(),
        port: 39103,
        symbol: "USDJPY".to_string(),
        point_size: 0.001,
        pip_size: 0.01,
        utc_offset_sec: 0,
        utc_verified: false,
        auto_utc_offset: true,
        timezone_rule: TimezoneRule::Fixed,
        terminal_path: None,
        receive_delay_ms: None,
    };


    let receiver = Arc::new(TransportReceiver::new(
        config,
        "off".to_string(),
        clock.clone(),
        ingress.clone(),
    ));
    let rec_clone = receiver.clone();
    let thread_handle = thread::spawn(move || {
        rec_clone.run();
    });

    thread::sleep(Duration::from_millis(100));

    // Session 1: Client connects, sends a frame, and disconnects (EOF)
    {
        let mut client1 = TcpStream::connect("127.0.0.1:39103").expect("Session 1 must connect");
        client1.set_nodelay(true).unwrap();

        let frame1 = Frame {
            header: Header {
                magic: MAGIC_TICK,
                protocol_version: PROTOCOL_VERSION,
                message_type: MSG_TYPE_TICK_BATCH,
                header_length: HEADER_LENGTH,
                header_flags: 0,
                broker_id: 3,
                session_id: 101,
                sequence_start: 1,
                tick_count: 1,
                payload_length: 72,
            },
            payload: FramePayload::TickBatch(vec![TickRecord {
                sequence: 1,
                broker_time_msc: 1000,
                ea_elapsed_us: 100,
                bid: 155.100,
                ask: 155.102,
                last: 0.0,
                volume: 1,
                volume_real: 1.0,
                flags: 0,
                reserved: 0,
            }]),
        };
        let bytes1 = encode_frame(&frame1).unwrap();
        client1.write_all(&bytes1).unwrap();
        client1.flush().unwrap();
        thread::sleep(Duration::from_millis(50));
        drop(client1);
    }

    thread::sleep(Duration::from_millis(100));

    // Session 2: Client reconnects (simulating MT5 reconnecting after disconnect)
    {
        let mut client2 = TcpStream::connect("127.0.0.1:39103").expect("Session 2 must reconnect");
        client2.set_nodelay(true).unwrap();

        let frame2 = Frame {
            header: Header {
                magic: MAGIC_TICK,
                protocol_version: PROTOCOL_VERSION,
                message_type: MSG_TYPE_TICK_BATCH,
                header_length: HEADER_LENGTH,
                header_flags: 0,
                broker_id: 3,
                session_id: 101,
                sequence_start: 2,
                tick_count: 1,
                payload_length: 72,
            },
            payload: FramePayload::TickBatch(vec![TickRecord {
                sequence: 2,
                broker_time_msc: 1200,
                ea_elapsed_us: 200,
                bid: 155.105,
                ask: 155.107,
                last: 0.0,
                volume: 1,
                volume_real: 1.0,
                flags: 0,
                reserved: 0,
            }]),
        };
        let bytes2 = encode_frame(&frame2).unwrap();
        client2.write_all(&bytes2).unwrap();
        client2.flush().unwrap();
        thread::sleep(Duration::from_millis(50));
        drop(client2);
    }

    thread::sleep(Duration::from_millis(50));
    receiver.stop();
    let _ = thread_handle.join();

    let items = ingress.collected_items();
    let connected_count = items.iter().filter(|it| matches!(it, IngressItem::Connected { broker_id: 3, .. })).count();
    let end_count = items.iter().filter(|it| matches!(it, IngressItem::End { broker_id: 3, .. })).count();
    let frame_count = items.iter().filter(|it| matches!(it, IngressItem::Frame(_))).count();

    assert_eq!(connected_count, 2, "Must accept both connections");
    assert_eq!(end_count, 2, "Must record clean End for both connections");
    assert_eq!(frame_count, 2, "Must receive frames from both sessions");
}

#[test]
fn test_transport_receiver_activity_timeout() {
    let clock = Arc::new(FakeClock::new(500_000_000, 1_700_000_000_000_000_000));
    let ingress = Arc::new(FakeIngressSink::new(100));

    let config = BrokerConfig {
        id: 4,
        name: "TestBroker4".to_string(),
        host: "127.0.0.1".to_string(),
        port: 39104,
        symbol: "USDJPY".to_string(),
        point_size: 0.001,
        pip_size: 0.01,
        utc_offset_sec: 0,
        utc_verified: false,
        auto_utc_offset: true,
        timezone_rule: TimezoneRule::Fixed,
        terminal_path: None,
        receive_delay_ms: None,
    };

    let receiver = Arc::new(
        TransportReceiver::new(config, "off".to_string(), clock.clone(), ingress.clone())
            .with_activity_timeout(Duration::from_millis(150)),
    );
    let rec_clone = receiver.clone();
    let thread_handle = thread::spawn(move || {
        rec_clone.run();
    });

    thread::sleep(Duration::from_millis(50));

    let mut client = TcpStream::connect("127.0.0.1:39104").expect("Client must connect");
    client.set_nodelay(true).unwrap();

    // Sleep longer than activity timeout (150ms) without sending any frames
    thread::sleep(Duration::from_millis(300));

    // Verify the server closed the socket due to activity timeout
    let mut buf = [0u8; 16];
    let read_res = client.read(&mut buf);
    assert!(
        matches!(read_res, Ok(0) | Err(_)),
        "Server must close socket after activity timeout"
    );

    receiver.stop();
    let _ = thread_handle.join();

    let items = ingress.collected_items();
    let end_item = items
        .iter()
        .find(|it| matches!(it, IngressItem::End { broker_id: 4, .. }));
    assert!(end_item.is_some(), "Must record End item on activity timeout");
    if let Some(IngressItem::End { reason, .. }) = end_item {
        assert!(
            reason.contains("activity timeout"),
            "End reason must indicate activity timeout, got: {reason}"
        );
    }
}

#[test]
fn test_transport_router_connection_takeover() {
    use std::collections::HashMap;
    use tick_scope::transport::router::TransportRouter;

    let clock = Arc::new(FakeClock::new(500_000_000, 1_700_000_000_000_000_000));
    let ingress = Arc::new(FakeIngressSink::new(100));

    let broker_cfg = BrokerConfig {
        id: 5,
        name: "TestBroker5".to_string(),
        host: "127.0.0.1".to_string(),
        port: 0,
        symbol: "USDJPY".to_string(),
        point_size: 0.001,
        pip_size: 0.01,
        utc_offset_sec: 0,
        utc_verified: false,
        auto_utc_offset: true,
        timezone_rule: TimezoneRule::Fixed,
        terminal_path: None,
        receive_delay_ms: None,
    };

    let receiver = Arc::new(TransportReceiver::new(
        broker_cfg,
        "off".to_string(),
        clock.clone(),
        ingress.clone(),
    ));

    let mut receivers = HashMap::new();
    receivers.insert(5, receiver);

    let router = Arc::new(TransportRouter::bind_loopback(receivers, 10).unwrap());
    let router_port = router.local_port();
    let router_clone = router.clone();
    let router_handle = thread::spawn(move || {
        router_clone.run();
    });

    thread::sleep(Duration::from_millis(50));

    // Client 1 connects and sends route handshake for broker 5
    let mut client1 = TcpStream::connect(("127.0.0.1", router_port)).expect("Client 1 must connect");
    client1.set_nodelay(true).unwrap();
    let mut hello = [0u8; 8];
    hello[0..4].copy_from_slice(b"TSCP");
    hello[4..8].copy_from_slice(&5u32.to_le_bytes());
    client1.write_all(&hello).unwrap();
    client1.flush().unwrap();

    thread::sleep(Duration::from_millis(50));

    // Send a frame from client 1
    let frame1 = Frame {
        header: Header {
            magic: MAGIC_TICK,
            protocol_version: PROTOCOL_VERSION,
            message_type: MSG_TYPE_TICK_BATCH,
            header_length: HEADER_LENGTH,
            header_flags: 0,
            broker_id: 5,
            session_id: 201,
            sequence_start: 1,
            tick_count: 1,
            payload_length: 72,
        },
        payload: FramePayload::TickBatch(vec![TickRecord {
            sequence: 1,
            broker_time_msc: 1000,
            ea_elapsed_us: 100,
            bid: 155.0,
            ask: 155.02,
            last: 0.0,
            volume: 1,
            volume_real: 1.0,
            flags: 0,
            reserved: 0,
        }]),
    };
    client1.write_all(&encode_frame(&frame1).unwrap()).unwrap();
    client1.flush().unwrap();

    thread::sleep(Duration::from_millis(50));

    // Client 2 connects for the same broker 5 (CONNECTION TAKEOVER)
    let mut client2 = TcpStream::connect(("127.0.0.1", router_port)).expect("Client 2 must connect");
    client2.set_nodelay(true).unwrap();
    client2.write_all(&hello).unwrap();
    client2.flush().unwrap();

    thread::sleep(Duration::from_millis(100));

    // Verify Client 1 was superseded: reading from client1 should encounter EOF or closed socket
    let mut buf = [0u8; 16];
    let res1 = client1.read(&mut buf);
    assert!(
        matches!(res1, Ok(0) | Err(_)),
        "Client 1 must be closed by takeover, got: {:?}",
        res1
    );

    // Send a frame from client 2
    let frame2 = Frame {
        header: Header {
            magic: MAGIC_TICK,
            protocol_version: PROTOCOL_VERSION,
            message_type: MSG_TYPE_TICK_BATCH,
            header_length: HEADER_LENGTH,
            header_flags: 0,
            broker_id: 5,
            session_id: 202,
            sequence_start: 2,
            tick_count: 1,
            payload_length: 72,
        },
        payload: FramePayload::TickBatch(vec![TickRecord {
            sequence: 2,
            broker_time_msc: 1200,
            ea_elapsed_us: 200,
            bid: 155.05,
            ask: 155.07,
            last: 0.0,
            volume: 1,
            volume_real: 1.0,
            flags: 0,
            reserved: 0,
        }]),
    };
    client2.write_all(&encode_frame(&frame2).unwrap()).unwrap();
    client2.flush().unwrap();

    thread::sleep(Duration::from_millis(50));
    drop(client2);

    router.stop();
    let _ = router_handle.join();

    let items = ingress.collected_items();
    let connections: Vec<_> = items
        .iter()
        .filter_map(|it| match it {
            IngressItem::Connected { broker_id, generation, .. } if *broker_id == 5 => Some(*generation),
            _ => None,
        })
        .collect();
    assert_eq!(connections, vec![1, 2], "Must record both generation 1 and generation 2");

    let frame_count = items
        .iter()
        .filter(|it| matches!(it, IngressItem::Frame(rf) if rf.frame.header.broker_id == 5))
        .count();
    assert_eq!(frame_count, 2, "Must receive frames from both connections");
}



