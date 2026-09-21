//! Low-level byte encoding and decoding for wire protocol messages.
//! Reference: docs/blueprint/wire-format.md

use crate::contracts::types::*;
use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq, Clone)]
pub enum ProtocolError {
    #[error("Buffer truncated, need more bytes: have {have}, need {need}")]
    NeedMore { have: usize, need: usize },
    #[error("Invalid magic number: 0x{0:08X}, expected 0x5449434B")]
    InvalidMagic(u32),
    #[error("Unsupported protocol version: {0}, expected 1")]
    UnsupportedVersion(u16),
    #[error("Invalid header length: {0}, expected 40")]
    InvalidHeaderLength(u16),
    #[error("Unknown message type: {0}")]
    UnknownMessageType(u16),
    #[error("Invalid header flags for message type {msg_type}: 0x{flags:04X}")]
    InvalidHeaderFlags { msg_type: u16, flags: u16 },
    #[error("Payload length {actual} does not match expected length {expected}")]
    PayloadLengthMismatch { expected: usize, actual: usize },
    #[error("Payload length {0} exceeds maximum allowed ({MAX_PAYLOAD_LENGTH})")]
    PayloadLengthExceedsMax(u32),
    #[error("Arithmetic overflow in length calculations")]
    ArithmeticOverflow,
    #[error("Malformed payload: {0}")]
    MalformedPayload(String),
}

pub fn decode_header(buf: &[u8]) -> Result<Header, ProtocolError> {
    if buf.len() < HEADER_LENGTH as usize {
        return Err(ProtocolError::NeedMore {
            have: buf.len(),
            need: HEADER_LENGTH as usize,
        });
    }

    let magic = u32::from_le_bytes(buf[0..4].try_into().unwrap());
    if magic != MAGIC_TICK {
        return Err(ProtocolError::InvalidMagic(magic));
    }

    let protocol_version = u16::from_le_bytes(buf[4..6].try_into().unwrap());
    if protocol_version != PROTOCOL_VERSION {
        return Err(ProtocolError::UnsupportedVersion(protocol_version));
    }

    let message_type = u16::from_le_bytes(buf[6..8].try_into().unwrap());
    let header_length = u16::from_le_bytes(buf[8..10].try_into().unwrap());
    if header_length != HEADER_LENGTH {
        return Err(ProtocolError::InvalidHeaderLength(header_length));
    }

    let header_flags = u16::from_le_bytes(buf[10..12].try_into().unwrap());
    match message_type {
        MSG_TYPE_TICK_BATCH => {
            if (header_flags & !HEADER_FLAG_WARMUP) != 0 {
                return Err(ProtocolError::InvalidHeaderFlags {
                    msg_type: message_type,
                    flags: header_flags,
                });
            }
        }
        MSG_TYPE_HEARTBEAT => {
            if (header_flags & !(HB_FLAG_HAS_LAST_TICK | HB_FLAG_HAS_OFFSET_SAMPLE)) != 0 {
                return Err(ProtocolError::InvalidHeaderFlags {
                    msg_type: message_type,
                    flags: header_flags,
                });
            }
        }
        MSG_TYPE_BATCH_ACK | MSG_TYPE_STATUS => {
            if header_flags != 0 {
                return Err(ProtocolError::InvalidHeaderFlags {
                    msg_type: message_type,
                    flags: header_flags,
                });
            }
        }
        _ => return Err(ProtocolError::UnknownMessageType(message_type)),
    }

    let broker_id = u32::from_le_bytes(buf[12..16].try_into().unwrap());
    let session_id = u64::from_le_bytes(buf[16..24].try_into().unwrap());
    let sequence_start = u64::from_le_bytes(buf[24..32].try_into().unwrap());
    let tick_count = u32::from_le_bytes(buf[32..36].try_into().unwrap());
    let payload_length = u32::from_le_bytes(buf[36..40].try_into().unwrap());

    if payload_length > MAX_PAYLOAD_LENGTH {
        return Err(ProtocolError::PayloadLengthExceedsMax(payload_length));
    }

    // Validate type-specific length
    match message_type {
        MSG_TYPE_TICK_BATCH => {
            let expected_len = (tick_count as usize)
                .checked_mul(TICK_RECORD_LENGTH)
                .ok_or(ProtocolError::ArithmeticOverflow)?;
            if payload_length as usize != expected_len {
                return Err(ProtocolError::PayloadLengthMismatch {
                    expected: expected_len,
                    actual: payload_length as usize,
                });
            }
            if tick_count == 0 {
                return Err(ProtocolError::MalformedPayload(
                    "TICK_BATCH tick_count must be at least 1".to_string(),
                ));
            }
        }
        MSG_TYPE_HEARTBEAT => {
            if payload_length as usize != HEARTBEAT_PAYLOAD_LENGTH {
                return Err(ProtocolError::PayloadLengthMismatch {
                    expected: HEARTBEAT_PAYLOAD_LENGTH,
                    actual: payload_length as usize,
                });
            }
            if tick_count != 0 || sequence_start != 0 {
                return Err(ProtocolError::MalformedPayload(
                    "HEARTBEAT must have sequence_start=0 and tick_count=0".to_string(),
                ));
            }
        }
        MSG_TYPE_BATCH_ACK => {
            if payload_length as usize != BATCH_ACK_PAYLOAD_LENGTH {
                return Err(ProtocolError::PayloadLengthMismatch {
                    expected: BATCH_ACK_PAYLOAD_LENGTH,
                    actual: payload_length as usize,
                });
            }
            if tick_count != 0 || sequence_start != 0 {
                return Err(ProtocolError::MalformedPayload(
                    "BATCH_ACK must have sequence_start=0 and tick_count=0".to_string(),
                ));
            }
        }
        MSG_TYPE_STATUS => {
            if payload_length as usize != STATUS_PAYLOAD_LENGTH {
                return Err(ProtocolError::PayloadLengthMismatch {
                    expected: STATUS_PAYLOAD_LENGTH,
                    actual: payload_length as usize,
                });
            }
            if tick_count != 0 || sequence_start != 0 {
                return Err(ProtocolError::MalformedPayload(
                    "STATUS must have sequence_start=0 and tick_count=0".to_string(),
                ));
            }
        }
        _ => unreachable!(),
    }

    Ok(Header {
        magic,
        protocol_version,
        message_type,
        header_length,
        header_flags,
        broker_id,
        session_id,
        sequence_start,
        tick_count,
        payload_length,
    })
}

pub fn encode_header(hdr: &Header, buf: &mut [u8]) {
    buf[0..4].copy_from_slice(&hdr.magic.to_le_bytes());
    buf[4..6].copy_from_slice(&hdr.protocol_version.to_le_bytes());
    buf[6..8].copy_from_slice(&hdr.message_type.to_le_bytes());
    buf[8..10].copy_from_slice(&hdr.header_length.to_le_bytes());
    buf[10..12].copy_from_slice(&hdr.header_flags.to_le_bytes());
    buf[12..16].copy_from_slice(&hdr.broker_id.to_le_bytes());
    buf[16..24].copy_from_slice(&hdr.session_id.to_le_bytes());
    buf[24..32].copy_from_slice(&hdr.sequence_start.to_le_bytes());
    buf[32..36].copy_from_slice(&hdr.tick_count.to_le_bytes());
    buf[36..40].copy_from_slice(&hdr.payload_length.to_le_bytes());
}

pub fn decode_tick_record(buf: &[u8]) -> (TickRecord, Option<String>) {
    let sequence = u64::from_le_bytes(buf[0..8].try_into().unwrap());
    let broker_time_msc = i64::from_le_bytes(buf[8..16].try_into().unwrap());
    let ea_elapsed_us = u64::from_le_bytes(buf[16..24].try_into().unwrap());
    let bid = f64::from_le_bytes(buf[24..32].try_into().unwrap());
    let ask = f64::from_le_bytes(buf[32..40].try_into().unwrap());
    let last = f64::from_le_bytes(buf[40..48].try_into().unwrap());
    let volume = u64::from_le_bytes(buf[48..56].try_into().unwrap());
    let volume_real = f64::from_le_bytes(buf[56..64].try_into().unwrap());
    let flags = u32::from_le_bytes(buf[64..68].try_into().unwrap());
    let reserved = u32::from_le_bytes(buf[68..72].try_into().unwrap());

    let warn = if reserved != 0 {
        Some(format!(
            "Tick sequence {} has non-zero reserved field: 0x{:08X}",
            sequence, reserved
        ))
    } else {
        None
    };

    (
        TickRecord {
            sequence,
            broker_time_msc,
            ea_elapsed_us,
            bid,
            ask,
            last,
            volume,
            volume_real,
            flags,
            reserved,
        },
        warn,
    )
}

pub fn encode_tick_record(t: &TickRecord, buf: &mut [u8]) {
    buf[0..8].copy_from_slice(&t.sequence.to_le_bytes());
    buf[8..16].copy_from_slice(&t.broker_time_msc.to_le_bytes());
    buf[16..24].copy_from_slice(&t.ea_elapsed_us.to_le_bytes());
    buf[24..32].copy_from_slice(&t.bid.to_le_bytes());
    buf[32..40].copy_from_slice(&t.ask.to_le_bytes());
    buf[40..48].copy_from_slice(&t.last.to_le_bytes());
    buf[48..56].copy_from_slice(&t.volume.to_le_bytes());
    buf[56..64].copy_from_slice(&t.volume_real.to_le_bytes());
    buf[64..68].copy_from_slice(&t.flags.to_le_bytes());
    buf[68..72].copy_from_slice(&t.reserved.to_le_bytes());
}

pub fn decode_heartbeat(buf: &[u8]) -> HeartbeatPayload {
    let session_id = u64::from_le_bytes(buf[0..8].try_into().unwrap());
    let last_sequence = u64::from_le_bytes(buf[8..16].try_into().unwrap());
    let last_tick_time_msc = i64::from_le_bytes(buf[16..24].try_into().unwrap());
    let server_utc_offset_sec = i32::from_le_bytes(buf[24..28].try_into().unwrap());
    let heartbeat_elapsed_us = u64::from_le_bytes(buf[28..36].try_into().unwrap());

    HeartbeatPayload {
        session_id,
        last_sequence,
        last_tick_time_msc,
        server_utc_offset_sec,
        heartbeat_elapsed_us,
    }
}

pub fn encode_heartbeat(hb: &HeartbeatPayload, buf: &mut [u8]) {
    buf[0..8].copy_from_slice(&hb.session_id.to_le_bytes());
    buf[8..16].copy_from_slice(&hb.last_sequence.to_le_bytes());
    buf[16..24].copy_from_slice(&hb.last_tick_time_msc.to_le_bytes());
    buf[24..28].copy_from_slice(&hb.server_utc_offset_sec.to_le_bytes());
    buf[28..36].copy_from_slice(&hb.heartbeat_elapsed_us.to_le_bytes());
}

pub fn decode_batch_ack(buf: &[u8]) -> BatchAckPayload {
    let sequence_end = u64::from_le_bytes(buf[0..8].try_into().unwrap());
    BatchAckPayload { sequence_end }
}

pub fn encode_batch_ack(ack: &BatchAckPayload, buf: &mut [u8]) {
    buf[0..8].copy_from_slice(&ack.sequence_end.to_le_bytes());
}

pub fn decode_status(buf: &[u8]) -> Result<StatusPayload, ProtocolError> {
    let status_code = u16::from_le_bytes(buf[0..2].try_into().unwrap());
    let phase = u16::from_le_bytes(buf[2..4].try_into().unwrap());
    let detail_flags = u32::from_le_bytes(buf[4..8].try_into().unwrap());
    let sequence_first = u64::from_le_bytes(buf[8..16].try_into().unwrap());
    let sequence_last = u64::from_le_bytes(buf[16..24].try_into().unwrap());
    let affected_count = u64::from_le_bytes(buf[24..32].try_into().unwrap());
    let ea_elapsed_us = u64::from_le_bytes(buf[32..40].try_into().unwrap());
    let detail_value = i64::from_le_bytes(buf[40..48].try_into().unwrap());

    if (detail_flags & !(STATUS_FLAG_HAS_SEQUENCE_RANGE | STATUS_FLAG_HAS_EXACT_COUNT)) != 0 {
        return Err(ProtocolError::MalformedPayload(format!(
            "Invalid status detail flags: 0x{:08X}",
            detail_flags
        )));
    }

    if (detail_flags & STATUS_FLAG_HAS_SEQUENCE_RANGE) != 0 && sequence_first > sequence_last {
        return Err(ProtocolError::MalformedPayload(format!(
            "Status sequence_first ({}) > sequence_last ({})",
            sequence_first, sequence_last
        )));
    }

    Ok(StatusPayload {
        status_code,
        phase,
        detail_flags,
        sequence_first,
        sequence_last,
        affected_count,
        ea_elapsed_us,
        detail_value,
    })
}

pub fn encode_status(status: &StatusPayload, buf: &mut [u8]) {
    buf[0..2].copy_from_slice(&status.status_code.to_le_bytes());
    buf[2..4].copy_from_slice(&status.phase.to_le_bytes());
    buf[4..8].copy_from_slice(&status.detail_flags.to_le_bytes());
    buf[8..16].copy_from_slice(&status.sequence_first.to_le_bytes());
    buf[16..24].copy_from_slice(&status.sequence_last.to_le_bytes());
    buf[24..32].copy_from_slice(&status.affected_count.to_le_bytes());
    buf[32..40].copy_from_slice(&status.ea_elapsed_us.to_le_bytes());
    buf[40..48].copy_from_slice(&status.detail_value.to_le_bytes());
}
