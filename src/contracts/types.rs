//! Shared data types, newtypes, units, and invariants for TickCompare.
//! Reference: docs/blueprint/interfaces.md and docs/blueprint/invariants.md

use serde::{Deserialize, Serialize};
use std::fmt;
use std::sync::Arc;

pub type BrokerId = u32;
pub type SessionId = u64;
pub type Sequence = u64;
pub type AnalysisSegmentId = u64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TickId {
    pub broker_id: BrokerId,
    pub session_id: SessionId,
    pub sequence: Sequence,
}

impl fmt::Display for TickId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Tick({}:{}:{})", self.broker_id, self.session_id, self.sequence)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RunId(pub [u8; 16]);

impl RunId {
    pub fn new_random() -> Self {
        use std::time::{SystemTime, UNIX_EPOCH};
        let d = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        let nanos = d.as_nanos();
        let mut bytes = [0u8; 16];
        bytes[0..8].copy_from_slice(&(nanos as u64).to_le_bytes());
        bytes[8..16].copy_from_slice(&((nanos >> 64) as u64).to_le_bytes());
        Self(bytes)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize)]
pub struct MonoNs(pub u64);

impl MonoNs {
    pub const ZERO: Self = Self(0);

    pub fn saturating_sub(self, other: Self) -> Self {
        Self(self.0.saturating_sub(other.0))
    }

    pub fn as_millis(self) -> f64 {
        self.0 as f64 / 1_000_000.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize)]
pub struct UtcMs(pub i64);

impl UtcMs {
    pub const ZERO: Self = Self(0);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize)]
pub struct BrokerMs(pub i64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize)]
pub struct EaUs(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClockReading {
    pub run_id: RunId,
    pub mono_ns: MonoNs,
    pub unix_ns: Option<i64>,
}

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SequenceDisposition {
    New = 0,
    DuplicateExact = 1,
    IdentityConflict = 2,
    OutOfOrderUnverified = 3,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ObservedTick {
    pub tick_id: TickId,
    pub record: TickRecord,
    pub rx_mono_ns: MonoNs,
    pub rx_unix_ns: Option<i64>,
    pub connection_generation: u64,
    pub frame_index: u64,
    pub is_warmup: bool,
    pub segment_id: AnalysisSegmentId,
    pub disposition: SequenceDisposition,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NormalizedTick {
    pub observed: ObservedTick,
    pub utc_ms: UtcMs,
    pub normalization_epoch: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SymbolMeta {
    pub broker_id: BrokerId,
    pub symbol: String,
    pub digits: u32,
    pub point_size: f64,
    pub pip_size: f64,
}

impl SymbolMeta {
    pub fn points_to_price(&self, points: f64) -> f64 {
        points * self.point_size
    }

    pub fn price_to_points(&self, price: f64) -> f64 {
        if self.point_size > 0.0 {
            price / self.point_size
        } else {
            0.0
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Quote {
    pub tick_id: TickId,
    pub bid: f64,
    pub ask: f64,
    pub mid: f64,
    pub spread: f64,
    pub rx_mono_ns: MonoNs,
    pub utc_ms: Option<UtcMs>,
    pub is_warmup: bool,
    pub is_valid: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConnectionState {
    Disconnected,
    Connecting,
    Connected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FreshnessState {
    Unknown,
    Live,
    Stale,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HeartbeatState {
    Unknown,
    Ok,
    Timeout,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PhaseState {
    Unknown,
    Warming,
    Live,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct OverloadFlags {
    pub receiver: bool,
    pub engine: bool,
    pub logger: bool,
    pub analysis: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum IntegrityState {
    Complete,
    PrefixUnobserved,
    Gap,
    Unconfirmed,
    DataLoss,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NormalizationState {
    Unverified,
    Verified,
    Discontinuity,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HealthState {
    pub broker_id: BrokerId,
    pub connection: ConnectionState,
    pub data_freshness: FreshnessState,
    pub heartbeat: HeartbeatState,
    pub phase: PhaseState,
    pub overload: OverloadFlags,
    pub integrity: IntegrityState,
    pub normalization: NormalizationState,
    pub last_live_tick_rx_mono: Option<MonoNs>,
    pub last_heartbeat_rx_mono: Option<MonoNs>,
    pub total_ticks_received: u64,
    pub total_frames_received: u64,
}

impl Default for HealthState {
    fn default() -> Self {
        Self {
            broker_id: 0,
            connection: ConnectionState::Disconnected,
            data_freshness: FreshnessState::Unknown,
            heartbeat: HeartbeatState::Unknown,
            phase: PhaseState::Unknown,
            overload: OverloadFlags::default(),
            integrity: IntegrityState::Complete,
            normalization: NormalizationState::Unverified,
            last_live_tick_rx_mono: None,
            last_heartbeat_rx_mono: None,
            total_ticks_received: 0,
            total_frames_received: 0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DiagnosticSeverity {
    Error = 1,
    Warn = 2,
    Info = 3,
    Debug = 4,
    Trace = 5,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Diagnostic {
    pub code: String,
    pub severity: DiagnosticSeverity,
    pub broker_id: BrokerId,
    pub session_id: Option<SessionId>,
    pub mono_ns: MonoNs,
    pub sequence_range: Option<(Sequence, Sequence)>,
    pub known_count: Option<u64>,
    pub detail_value: i64,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LogRawFrame {
    pub broker_id: BrokerId,
    pub connection_generation: u64,
    pub frame_index: u64,
    pub rx_mono_ns: MonoNs,
    pub rx_unix_ns: Option<i64>,
    pub config_epoch: u64,
    pub analysis_segment: AnalysisSegmentId,
    pub raw_wire_bytes: Arc<Vec<u8>>,
    pub dispositions: Vec<SequenceDisposition>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LogMetadata {
    pub config_epoch: u64,
    pub observed_mono_ns: MonoNs,
    pub toml_text: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum LogRecord {
    RawFrame(LogRawFrame),
    Metadata(LogMetadata),
    Diagnostic(Diagnostic),
}

