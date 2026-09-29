//! Protocol Codec tests: T-W01 through T-W04.

use tick_scope::core::types::*;
use tick_scope::protocol::codec::*;
use tick_scope::protocol::packet::*;

fn ack_wire(sequence_end: u64) -> Vec<u8> {
    encode_frame(&Frame {
        header: Header {
            magic: MAGIC_TICK, protocol_version: PROTOCOL_VERSION,
            message_type: MSG_TYPE_BATCH_ACK, header_length: HEADER_LENGTH,
            header_flags: 0, broker_id: 1, session_id: 1,
            sequence_start: 0, tick_count: 0,
            payload_length: BATCH_ACK_PAYLOAD_LENGTH as u32,
        },
        payload: FramePayload::BatchAck(BatchAckPayload { sequence_end }),
    }).unwrap()
}

#[test]
fn burst_decode_preserves_every_frame_across_compaction_and_partial_tail() {
    let mut decoder = StreamingDecoder::new(1_048_576, 65_536);
    let mut wire = Vec::new();
    for sequence in 0..10_000 {
        wire.extend_from_slice(&ack_wire(sequence));
    }
    let mut expected = 0;
    // Deliberately unaligned with the 48-byte frame size.
    for chunk in wire.chunks(8191) {
        decoder.push(chunk).unwrap();
        while let Some(frame) = decoder.next_frame().unwrap() {
            assert_eq!(*frame.raw_wire_bytes, ack_wire(expected));
            assert!(matches!(frame.frame.payload, FramePayload::BatchAck(a) if a.sequence_end == expected));
            expected += 1;
        }
    }
    assert_eq!(expected, 10_000);
    assert_eq!(decoder.buffer_len(), 0);
}

#[test]
fn configured_payload_limit_is_enforced_before_waiting_for_body() {
    let wire = ack_wire(1);
    let mut decoder = StreamingDecoder::new(4, 64);
    decoder.push(&wire[..HEADER_LENGTH as usize]).unwrap();
    assert_eq!(decoder.next_frame(), Err(ProtocolError::PayloadLengthExceedsMax(8)));
}

#[test]
fn resync_after_consumed_frame_preserves_next_frame() {
    let mut decoder = StreamingDecoder::new(1024, 64);
    let mut wire = ack_wire(1);
    wire.extend_from_slice(&[0xFF; 7]);
    wire.extend_from_slice(&ack_wire(2));
    decoder.push(&wire).unwrap();
    assert!(decoder.next_frame().unwrap().is_some());
    assert!(decoder.next_frame().is_err());
    assert!(decoder.try_resync());
    assert_eq!(*decoder.next_frame().unwrap().unwrap().raw_wire_bytes, ack_wire(2));
    assert_eq!(decoder.buffer_len(), 0);
}

#[test]
fn test_tw01_golden_vectors() {
    let mut decoder = StreamingDecoder::new(1_048_576, 65_536);

    let mut wire = Vec::new();
    wire.extend_from_slice(&[
        0x4B, 0x43, 0x49, 0x54, // magic
        0x01, 0x00,             // version: 1
        0x01, 0x00,             // type: TICK_BATCH
        0x28, 0x00,             // header_len: 40
        0x00, 0x00,             // flags: 0
        0x01, 0x00, 0x00, 0x00, // broker: 1
        0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // session: 1
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // seq_start: 0
        0x01, 0x00, 0x00, 0x00, // tick_count: 1
        0x48, 0x00, 0x00, 0x00, // payload_len: 72
    ]);
    wire.extend_from_slice(&[
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // seq: 0
        0xE8, 0x03, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // time_msc: 1000
        0x0A, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // ea_us: 10
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xF0, 0x3F, // bid: 1.0
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x40, // ask: 2.0
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // last: 0.0
        0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // volume: 1
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xF0, 0x3F, // volume_real: 1.0
        0x00, 0x00, 0x00, 0x00,                         // flags: 0
        0x00, 0x00, 0x00, 0x00,                         // reserved: 0
    ]);

    decoder.push(&wire).unwrap();
    let decoded = decoder.next_frame().unwrap().expect("Frame must decode");
    assert_eq!(decoded.frame.header.magic, MAGIC_TICK);
    assert_eq!(decoded.frame.header.tick_count, 1);
    if let FramePayload::TickBatch(ticks) = decoded.frame.payload {
        assert_eq!(ticks.len(), 1);
        assert_eq!(ticks[0].bid, 1.0);
        assert_eq!(ticks[0].ask, 2.0);
        assert_eq!(ticks[0].broker_time_msc, 1000);
    } else {
        panic!("Expected TickBatch payload");
    }
}

#[test]
fn test_tw02_split_reads_and_coalesced_frames() {
    let frame1 = Frame {
        header: Header {
            magic: MAGIC_TICK,
            protocol_version: PROTOCOL_VERSION,
            message_type: MSG_TYPE_HEARTBEAT,
            header_length: HEADER_LENGTH,
            header_flags: HB_FLAG_HAS_LAST_TICK,
            broker_id: 1,
            session_id: 42,
            sequence_start: 0,
            tick_count: 0,
            payload_length: HEARTBEAT_PAYLOAD_LENGTH as u32,
        },
        payload: FramePayload::Heartbeat(HeartbeatPayload {
            session_id: 42,
            last_sequence: 100,
            last_tick_time_msc: 2000,
            server_utc_offset_sec: 10800,
            heartbeat_elapsed_us: 500_000,
        }),
    };

    let frame2 = Frame {
        header: Header {
            magic: MAGIC_TICK,
            protocol_version: PROTOCOL_VERSION,
            message_type: MSG_TYPE_BATCH_ACK,
            header_length: HEADER_LENGTH,
            header_flags: 0,
            broker_id: 1,
            session_id: 42,
            sequence_start: 0,
            tick_count: 0,
            payload_length: BATCH_ACK_PAYLOAD_LENGTH as u32,
        },
        payload: FramePayload::BatchAck(BatchAckPayload { sequence_end: 100 }),
    };

    let bytes1 = encode_frame(&frame1).unwrap();
    let bytes2 = encode_frame(&frame2).unwrap();

    let mut combined = Vec::new();
    combined.extend_from_slice(&bytes1);
    combined.extend_from_slice(&bytes2);

    // Feed 1 byte at a time
    let mut decoder = StreamingDecoder::new(1_048_576, 65_536);
    let mut decoded_frames = Vec::new();

    for &b in &combined {
        decoder.push(&[b]).unwrap();
        while let Some(f) = decoder.next_frame().unwrap() {
            decoded_frames.push(f);
        }
    }

    assert_eq!(decoded_frames.len(), 2);
    assert_eq!(decoded_frames[0].frame.header.message_type, MSG_TYPE_HEARTBEAT);
    assert_eq!(decoded_frames[1].frame.header.message_type, MSG_TYPE_BATCH_ACK);
}

#[test]
fn test_tw03_malformed_errors() {
    let mut decoder = StreamingDecoder::new(1_048_576, 65_536);
    let mut bad_magic = vec![0u8; 40];
    bad_magic[0..4].copy_from_slice(&0xDEADBEEFu32.to_le_bytes());

    decoder.push(&bad_magic).unwrap();
    let err = decoder.next_frame().unwrap_err();
    assert!(matches!(err, ProtocolError::InvalidMagic(0xDEADBEEF)));
}

#[test]
fn test_tw04_reserved_warn_and_nan_bits() {
    let nan_val = f64::from_bits(0x7FF0_0000_0000_0001); // signaling NaN with payload 1
    assert!(nan_val.is_nan());

    let frame = Frame {
        header: Header {
            magic: MAGIC_TICK,
            protocol_version: PROTOCOL_VERSION,
            message_type: MSG_TYPE_TICK_BATCH,
            header_length: HEADER_LENGTH,
            header_flags: 0,
            broker_id: 2,
            session_id: 10,
            sequence_start: 5,
            tick_count: 1,
            payload_length: 72,
        },
        payload: FramePayload::TickBatch(vec![TickRecord {
            sequence: 5,
            broker_time_msc: 3000,
            ea_elapsed_us: 4000,
            bid: nan_val,
            ask: 158.234,
            last: 0.0,
            volume: 1,
            volume_real: 1.0,
            flags: 2,
            reserved: 0xCAFEBABE, // Non-zero reserved
        }]),
    };

    let encoded = encode_frame(&frame).unwrap();
    let mut decoder = StreamingDecoder::new(1_048_576, 65_536);
    decoder.push(&encoded).unwrap();

    let decoded = decoder.next_frame().unwrap().expect("Must decode with warn");
    assert!(!decoded.warnings.is_empty(), "Must have warning for reserved != 0");
    assert!(decoded.warnings[0].contains("non-zero reserved"));

    if let FramePayload::TickBatch(ticks) = decoded.frame.payload {
        assert_eq!(ticks[0].bid.to_bits(), nan_val.to_bits(), "Exact NaN bits must be preserved");
        assert_eq!(ticks[0].reserved, 0xCAFEBABE);
    } else {
        panic!("Expected TickBatch");
    }
}
