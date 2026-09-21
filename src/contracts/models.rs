//! Analysis, projection, and UI data models.
//! Reference: docs/blueprint/interfaces.md, semantics-candle.md, semantics-lead-lag.md, semantics-snapshot.md

use crate::contracts::types::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PriceMode {
    Bid,
    Ask,
    Mid,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Ohlc {
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub open_key: (UtcMs, Sequence),
    pub close_key: (UtcMs, Sequence),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SlotState {
    Empty,
    Active,
    Closed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SlotCoverage {
    Full,
    Partial,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CandleSlot {
    pub broker_id: BrokerId,
    pub segment_id: AnalysisSegmentId,
    pub period_ms: i64,
    pub start_utc_ms: UtcMs,
    pub state: SlotState,
    pub ohlc: Option<Ohlc>,
    pub tick_count: u64,
    pub revision: u64,
    pub coverage: SlotCoverage,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CandleView {
    pub period_ms: i64,
    pub slot_starts: Vec<UtcMs>,
    pub slots_by_broker: HashMap<BrokerId, Vec<CandleSlot>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MoveDirection {
    Up,
    Down,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MoveQuality {
    BidOnly,
    AskOnly,
    BothSides,
    SpreadDriven,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MoveEvent {
    pub segment_id: AnalysisSegmentId,
    pub broker_id: BrokerId,
    pub trigger_sequence: Sequence,
    pub rx_mono_ns: MonoNs,
    pub direction: MoveDirection,
    pub anchor_mid: f64,
    pub current_mid: f64,
    pub mid_delta_points: f64,
    pub bid_delta: f64,
    pub ask_delta: f64,
    pub mid_delta: f64,
    pub spread_delta: f64,
    pub quality: MoveQuality,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LeadLagMatch {
    pub match_id: u64,
    pub leader: BrokerId,
    pub follower: BrokerId,
    pub leader_event: MoveEvent,
    pub follower_event: MoveEvent,
    pub t_leader: MonoNs,
    pub t_follower: MonoNs,
    pub signed_delta_ns: i64, // t_B - t_A: positive means A leads, negative means B leads
    pub abs_delta_ns: u64,
    pub raw_delta_ms: f64,
    pub ema_delta_ms: Option<f64>,
    pub segment_id: AnalysisSegmentId,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BrokerOverview {
    pub broker_id: BrokerId,
    pub name: String,
    pub symbol: String,
    pub latest_quote: Option<Quote>,
    pub min_spread: Option<f64>,
    pub max_spread: Option<f64>,
    pub health: HealthState,
    pub tick_rate_1s: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct DiffPoint {
    pub mono_ns: MonoNs,
    pub bid_diff: f64,
    pub ask_diff: f64,
    pub mid_diff: f64,
    pub spread_diff: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PairComparison {
    pub broker_a: BrokerId,
    pub broker_b: BrokerId,
    pub as_of_mono_ns: MonoNs,
    pub bid_diff: Option<f64>, // A - B
    pub ask_diff: Option<f64>,
    pub mid_diff: Option<f64>,
    pub spread_diff: Option<f64>,
    pub recent_diff_series: Vec<DiffPoint>,
    pub latest_match: Option<LeadLagMatch>,
    pub ema_lead_lag_ms: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EngineProjection {
    pub revision: u64,
    pub watermark_ns: MonoNs,
    pub broker_overviews: Vec<BrokerOverview>,
    pub active_pair: (BrokerId, BrokerId),
    pub active_pair_comparison: Option<PairComparison>,
    pub candle_views: HashMap<i64, CandleView>,
    pub global_diagnostics: Vec<Diagnostic>,
}

pub const SCHEMA_REVISION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UiSnapshot {
    pub schema_revision: u32,
    pub snapshot_revision: u64,
    pub projection_revision: u64,
    pub run_id: RunId,
    pub built_mono_ns: MonoNs,
    pub processed_watermark_ns: MonoNs,
    pub display_now_utc: UtcMs,
    pub active_pair: (BrokerId, BrokerId),
    pub broker_overviews: Vec<BrokerOverview>,
    pub active_pair_comparison: Option<PairComparison>,
    pub active_candles: Option<CandleView>,
    pub diagnostics: Vec<Diagnostic>,
}
