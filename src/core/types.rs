//! Shared data types, newtypes, units, and invariants for TickScope.

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

/// 72-byte wire and core tick record representation.
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
pub enum SequenceDisposition {
    New = 0,
    DuplicateExact = 1,
    IdentityConflict = 2,
    OutOfOrderUnverified = 3,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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
