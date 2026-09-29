//! Binary wire protocol types, headers, payload records, and constants.

use crate::core::types::{BrokerId, MonoNs, RunId, Sequence, SessionId};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

pub const MAGIC_TICK: u32 = 0x5449434B; // 'K' 'C' 'I' 'T' in LE bytes: 4B 43 49 54
pub const PROTOCOL_VERSION: u16 = 1;
pub const HEADER_LENGTH: u16 = 40;
pub const TICK_RECORD_LENGTH: usize = 72;
pub const HEARTBEAT_PAYLOAD_LENGTH: usize = 36;
pub const BATCH_ACK_PAYLOAD_LENGTH: usize = 8;
pub const STATUS_PAYLOAD_LENGTH: usize = 48;
pub const MAX_PAYLOAD_LENGTH: u32 = 1_048_576; // 1 MiB

pub const MSG_TYPE_TICK_BATCH: u16 = 1;
pub const MSG_TYPE_HEARTBEAT: u16 = 2;
pub const MSG_TYPE_BATCH_ACK: u16 = 3;
pub const MSG_TYPE_STATUS: u16 = 4;

pub const HEADER_FLAG_WARMUP: u16 = 1 << 0;
pub const HB_FLAG_HAS_LAST_TICK: u16 = 1 << 1;
pub const HB_FLAG_HAS_OFFSET_SAMPLE: u16 = 1 << 2;

pub const STATUS_FLAG_HAS_SEQUENCE_RANGE: u32 = 1 << 0;
pub const STATUS_FLAG_HAS_EXACT_COUNT: u32 = 1 << 1;

pub const PHASE_WARMING: u16 = 1;
pub const PHASE_LIVE: u16 = 2;

pub const STATUS_CODE_PHASE: u16 = 1;
pub const STATUS_CODE_TICK_BACKLOG: u16 = 2;
pub const STATUS_CODE_CURSOR_BLOCKED: u16 = 3;
pub const STATUS_CODE_TRANSPORT_FAULT: u16 = 4;
pub const STATUS_CODE_DATA_LOSS: u16 = 5;
pub const STATUS_CODE_UNCONFIRMED: u16 = 6;
pub const STATUS_CODE_RECOVERY: u16 = 7;

/// 72-byte wire record representation.
/// Float bits and reserved bytes are preserved verbatim.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct TickRecord {
    pub sequence: Sequence,
    pub broker_time_msc: i64,
    pub ea_elapsed_us: u64,
    pub bid: f64,
    pub ask: f64,
    pub last: f64,
    pub volume: u64,
    pub volume_real: f64,
    pub flags: u32,
    pub reserved: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Header {
    pub magic: u32,
    pub protocol_version: u16,
    pub message_type: u16,
    pub header_length: u16,
    pub header_flags: u16,
    pub broker_id: BrokerId,
    pub session_id: SessionId,
    pub sequence_start: Sequence,
    pub tick_count: u32,
    pub payload_length: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HeartbeatPayload {
    pub session_id: SessionId,
    pub last_sequence: Sequence,
    pub last_tick_time_msc: i64,
    pub server_utc_offset_sec: i32,
    pub heartbeat_elapsed_us: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct BatchAckPayload {
    pub sequence_end: Sequence,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatusPayload {
    pub status_code: u16,
    pub phase: u16,
    pub detail_flags: u32,
    pub sequence_first: Sequence,
    pub sequence_last: Sequence,
    pub affected_count: u64,
    pub ea_elapsed_us: u64,
    pub detail_value: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum FramePayload {
    TickBatch(Vec<TickRecord>),
    Heartbeat(HeartbeatPayload),
    BatchAck(BatchAckPayload),
    Status(StatusPayload),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Frame {
    pub header: Header,
    pub payload: FramePayload,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReceivedFrame {
    pub frame: Frame,
    pub raw_wire_bytes: Arc<Vec<u8>>,
    pub run_id: RunId,
    pub rx_mono_ns: MonoNs,
    pub rx_unix_ns: Option<i64>,
    pub connection_generation: u64,
    pub frame_index: u64,
}

impl ReceivedFrame {
    /// Return the wire size even when raw capture omitted the retained bytes.
    pub fn wire_len(&self) -> usize {
        if self.raw_wire_bytes.is_empty() {
            (self.frame.header.header_length as usize)
                .saturating_add(self.frame.header.payload_length as usize)
        } else {
            self.raw_wire_bytes.len()
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum IngressItem {
    Connected {
        broker_id: BrokerId,
        generation: u64,
        connected_at_mono: MonoNs,
    },
    Frame(ReceivedFrame),
    Progress {
        broker_id: BrokerId,
        watermark_ns: MonoNs,
    },
    End {
        broker_id: BrokerId,
        generation: u64,
        reason: String,
    },
}

impl IngressItem {
    pub fn broker_id(&self) -> BrokerId {
        match self {
            Self::Connected { broker_id, .. } => *broker_id,
            Self::Frame(rf) => rf.frame.header.broker_id,
            Self::Progress { broker_id, .. } => *broker_id,
            Self::End { broker_id, .. } => *broker_id,
        }
    }
}
