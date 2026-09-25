//! Configuration schema and validation for TickScope.
//! Reference: docs/blueprint/interfaces.md and docs/blueprint/decisions.md

use crate::contracts::types::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BrokerConfig {
    pub id: BrokerId,
    pub name: String,
    pub host: String,
    /// Legacy listener port used only when MT5 auto-deployment is disabled.
    /// With auto-deployment enabled, TickScope binds an ephemeral loopback port.
    #[serde(default = "default_broker_port")]
    pub port: u16,
    pub symbol: String,
    pub digits: u32,
    pub point_size: f64,
    pub pip_size: f64,
    pub utc_offset_sec: i32,
    #[serde(default)]
    pub utc_verified: bool,
    #[serde(default = "default_auto_utc_offset")]
    pub auto_utc_offset: bool,
}

fn default_broker_port() -> u16 {
    39001
}

fn default_auto_utc_offset() -> bool {
    true
}

impl Default for BrokerConfig {
    fn default() -> Self {
        Self {
            id: 1,
            name: "Broker1".to_string(),
            host: "127.0.0.1".to_string(),
            port: 39001,
            symbol: "USDJPY".to_string(),
            digits: 3,
            point_size: 0.001,
            pip_size: 0.01,
            utc_offset_sec: 0,
            utc_verified: false,
            auto_utc_offset: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProtocolConfig {
    #[serde(default = "default_max_payload")]
    pub max_payload_length: u32,
    #[serde(default = "default_ack_mode")]
    pub ack_mode: String,
    #[serde(default = "default_debug_resync_limit")]
    pub debug_resync_limit: u32,
}

fn default_max_payload() -> u32 {
    1_048_576
}
fn default_ack_mode() -> String {
    "required".to_string()
}
fn default_debug_resync_limit() -> u32 {
    65_536
}

impl Default for ProtocolConfig {
    fn default() -> Self {
        Self {
            max_payload_length: default_max_payload(),
            ack_mode: default_ack_mode(),
            debug_resync_limit: default_debug_resync_limit(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IngressConfig {
    #[serde(default = "default_max_frames")]
    pub max_frames_per_broker: usize,
    #[serde(default = "default_max_bytes")]
    pub max_bytes_per_broker: usize,
    #[serde(default = "default_progress_interval")]
    pub progress_interval_ms: u64,
}

fn default_max_frames() -> usize {
    256
}
fn default_max_bytes() -> usize {
    8 * 1024 * 1024
}
fn default_progress_interval() -> u64 {
    1
}

impl Default for IngressConfig {
    fn default() -> Self {
        Self {
            max_frames_per_broker: default_max_frames(),
            max_bytes_per_broker: default_max_bytes(),
            progress_interval_ms: default_progress_interval(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoggerConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_log_dir")]
    pub log_dir: String,
    #[serde(default = "default_queue_records")]
    pub max_queue_records: usize,
    #[serde(default = "default_queue_bytes")]
    pub max_queue_bytes: usize,
    #[serde(default = "default_flush_interval")]
    pub flush_interval_ms: u64,
    #[serde(default = "default_drain_timeout")]
    pub shutdown_drain_timeout_ms: u64,
}

fn default_true() -> bool {
    true
}
fn default_log_dir() -> String {
    "data/logs".to_string()
}
fn default_queue_records() -> usize {
    1024
}
fn default_queue_bytes() -> usize {
    32 * 1024 * 1024
}
fn default_flush_interval() -> u64 {
    1000
}
fn default_drain_timeout() -> u64 {
    5000
}

impl Default for LoggerConfig {
    fn default() -> Self {
        Self {
            enabled: default_true(),
            log_dir: default_log_dir(),
            max_queue_records: default_queue_records(),
            max_queue_bytes: default_queue_bytes(),
            flush_interval_ms: default_flush_interval(),
            shutdown_drain_timeout_ms: default_drain_timeout(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MatcherConfig {
    #[serde(default = "default_trigger_move")]
    pub trigger_move_points: f64,
    #[serde(default = "default_matching_window")]
    pub matching_window_ms: u64,
    #[serde(default = "default_event_cooldown")]
    pub event_cooldown_ms: u64,
    #[serde(default = "default_ema_alpha")]
    pub ema_alpha: f64,
    #[serde(default = "default_pending_capacity")]
    pub pending_event_capacity: usize,
    /// Maximum receive-time skew permitted for a displayed pairwise price
    /// difference. Lead/lag matching has its own window above.
    #[serde(default = "default_max_quote_skew")]
    pub max_quote_skew_ms: u64,
}

fn default_trigger_move() -> f64 {
    2.0
}
fn default_matching_window() -> u64 {
    100
}
fn default_event_cooldown() -> u64 {
    20
}
fn default_ema_alpha() -> f64 {
    0.1
}
fn default_pending_capacity() -> usize {
    8192
}
fn default_max_quote_skew() -> u64 {
    100
}

impl Default for MatcherConfig {
    fn default() -> Self {
        Self {
            trigger_move_points: default_trigger_move(),
            matching_window_ms: default_matching_window(),
            event_cooldown_ms: default_event_cooldown(),
            ema_alpha: default_ema_alpha(),
            pending_event_capacity: default_pending_capacity(),
            max_quote_skew_ms: default_max_quote_skew(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HealthConfig {
    #[serde(default = "default_stale_after")]
    pub stale_after_ms: u64,
    #[serde(default = "default_heartbeat_interval")]
    pub heartbeat_interval_ms: u64,
    #[serde(default = "default_heartbeat_timeout")]
    pub heartbeat_timeout_ms: u64,
}

fn default_stale_after() -> u64 {
    1000
}
fn default_heartbeat_interval() -> u64 {
    250
}
fn default_heartbeat_timeout() -> u64 {
    1500
}

impl Default for HealthConfig {
    fn default() -> Self {
        Self {
            stale_after_ms: default_stale_after(),
            heartbeat_interval_ms: default_heartbeat_interval(),
            heartbeat_timeout_ms: default_heartbeat_timeout(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DisplayConfig {
    #[serde(default = "default_repaint_hz")]
    pub repaint_hz: u32,
    #[serde(default = "default_timeframe_ms")]
    pub timeframe_ms: i64,
    #[serde(default = "default_visible_seconds")]
    pub visible_seconds: u64,
    #[serde(default = "default_visible_ticks")]
    pub visible_ticks: usize,
    #[serde(default = "default_false")]
    pub always_on_top: bool,
}

fn default_repaint_hz() -> u32 {
    60
}
fn default_timeframe_ms() -> i64 {
    60000
}
fn default_visible_seconds() -> u64 {
    60
}
fn default_visible_ticks() -> usize {
    1200
}
fn default_false() -> bool {
    false
}

impl Default for DisplayConfig {
    fn default() -> Self {
        Self {
            repaint_hz: default_repaint_hz(),
            timeframe_ms: default_timeframe_ms(),
            visible_seconds: default_visible_seconds(),
            visible_ticks: default_visible_ticks(),
            always_on_top: default_false(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SlotRetention {
    pub period_ms: i64,
    pub slots: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryConfig {
    #[serde(default = "default_slots")]
    pub retentions: Vec<SlotRetention>,
    #[serde(default = "default_ring_records")]
    pub max_tick_ring_records: usize,
    #[serde(default = "default_ledger_cap")]
    pub ledger_capacity: usize,
}

fn default_slots() -> Vec<SlotRetention> {
    vec![
        SlotRetention { period_ms: 1000, slots: 60 },
        SlotRetention { period_ms: 5000, slots: 24 },
        SlotRetention { period_ms: 10000, slots: 12 },
        SlotRetention { period_ms: 60000, slots: 10 },
    ]
}
fn default_ring_records() -> usize {
    120_000
}
fn default_ledger_cap() -> usize {
    16_384
}

impl Default for HistoryConfig {
    fn default() -> Self {
        Self {
            retentions: default_slots(),
            max_tick_ring_records: default_ring_records(),
            ledger_capacity: default_ledger_cap(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mt5DeployConfig {
    #[serde(default = "default_true")]
    pub auto_deploy: bool,
    #[serde(default)]
    pub custom_data_dirs: Vec<String>,
}

impl Default for Mt5DeployConfig {
    fn default() -> Self {
        Self {
            auto_deploy: default_true(),
            custom_data_dirs: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppConfig {
    pub brokers: Vec<BrokerConfig>,
    #[serde(default = "default_active_pair")]
    pub active_pair: (BrokerId, BrokerId),
    #[serde(default)]
    pub protocol: ProtocolConfig,
    #[serde(default)]
    pub ingress: IngressConfig,
    #[serde(default)]
    pub logger: LoggerConfig,
    #[serde(default)]
    pub matcher: MatcherConfig,
    #[serde(default)]
    pub health: HealthConfig,
    #[serde(default)]
    pub display: DisplayConfig,
    #[serde(default)]
    pub history: HistoryConfig,
    #[serde(default)]
    pub mt5: Mt5DeployConfig,
}

fn default_active_pair() -> (BrokerId, BrokerId) {
    (1, 2)
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            brokers: vec![
                BrokerConfig {
                    id: 1,
                    name: "BrokerA".to_string(),
                    host: "127.0.0.1".to_string(),
                    port: 39001,
                    symbol: "USDJPY".to_string(),
                    digits: 3,
                    point_size: 0.001,
                    pip_size: 0.01,
                    utc_offset_sec: 0,
                    utc_verified: false,
                    auto_utc_offset: true,
                },
                BrokerConfig {
                    id: 2,
                    name: "BrokerB".to_string(),
                    host: "127.0.0.1".to_string(),
                    port: 39002,
                    symbol: "USDJPY.pro".to_string(),
                    digits: 3,
                    point_size: 0.001,
                    pip_size: 0.01,
                    utc_offset_sec: 0,
                    utc_verified: false,
                    auto_utc_offset: true,
                },
            ],
            active_pair: (1, 2),
            protocol: ProtocolConfig::default(),
            ingress: IngressConfig::default(),
            logger: LoggerConfig::default(),
            matcher: MatcherConfig::default(),
            health: HealthConfig::default(),
            display: DisplayConfig::default(),
            history: HistoryConfig::default(),
            mt5: Mt5DeployConfig::default(),
        }
    }
}

impl AppConfig {
    pub fn validate(&self) -> Result<(), String> {
        if self.brokers.len() < 2 {
            return Err("At least 2 brokers must be configured".to_string());
        }

        let mut seen_ids = std::collections::HashSet::new();
        let mut seen_ports = std::collections::HashSet::new();
        for b in &self.brokers {
            if b.name.trim().is_empty() {
                return Err(format!("Broker {} name must not be empty", b.id));
            }
            if b.id == 0 {
                return Err("Broker id 0 is reserved and invalid".to_string());
            }
            if !seen_ids.insert(b.id) {
                return Err(format!("Duplicate broker id: {}", b.id));
            }
            if !self.mt5.auto_deploy && !seen_ports.insert(b.port) {
                return Err(format!("Duplicate broker port: {}", b.port));
            }
            if b.digits == 0 || b.digits > 8 {
                return Err(format!("Broker {} digits invalid: {}", b.id, b.digits));
            }
            if b.point_size <= 0.0 {
                return Err(format!("Broker {} point_size must be positive", b.id));
            }
            if b.pip_size <= 0.0 {
                return Err(format!("Broker {} pip_size must be positive", b.id));
            }
        }

        let (a, b) = self.active_pair;
        if a == b {
            return Err("Active pair cannot have the same broker for both sides".to_string());
        }
        if !seen_ids.contains(&a) {
            return Err(format!("Active pair broker A ({}) not in brokers list", a));
        }
        if !seen_ids.contains(&b) {
            return Err(format!("Active pair broker B ({}) not in brokers list", b));
        }

        if self.matcher.trigger_move_points <= 0.0 {
            return Err("trigger_move_points must be positive".to_string());
        }
        if self.matcher.matching_window_ms == 0 {
            return Err("matching_window_ms must be positive".to_string());
        }
        if self.matcher.ema_alpha <= 0.0 || self.matcher.ema_alpha > 1.0 {
            return Err("ema_alpha must be in (0.0, 1.0]".to_string());
        }
        if self.matcher.pending_event_capacity == 0 {
            return Err("pending_event_capacity must be positive".to_string());
        }
        if self.matcher.max_quote_skew_ms == 0 {
            return Err("max_quote_skew_ms must be positive".to_string());
        }
        if self.history.ledger_capacity == 0 {
            return Err("history.ledger_capacity must be positive".to_string());
        }
        if self.ingress.max_frames_per_broker == 0 || self.ingress.max_bytes_per_broker == 0 {
            return Err("ingress frame and byte capacities must be positive".to_string());
        }
        if self.logger.enabled
            && (self.logger.max_queue_records == 0 || self.logger.max_queue_bytes == 0)
        {
            return Err("logger record and byte capacities must be positive".to_string());
        }
        if self.protocol.max_payload_length == 0 || self.protocol.debug_resync_limit == 0 {
            return Err("protocol payload and resync limits must be positive".to_string());
        }
        let largest_raw_log_record = (self.protocol.max_payload_length as usize)
            .saturating_add(HEADER_LENGTH as usize)
            .saturating_add(128);
        if self.logger.enabled && self.logger.max_queue_bytes < largest_raw_log_record {
            return Err(format!(
                "logger.max_queue_bytes ({}) is smaller than one maximum raw frame ({largest_raw_log_record})",
                self.logger.max_queue_bytes,
            ));
        }
        let largest_raw_frame = (self.protocol.max_payload_length as usize)
            .saturating_add(HEADER_LENGTH as usize);
        if self.ingress.max_bytes_per_broker < largest_raw_frame {
            return Err(format!(
                "ingress.max_bytes_per_broker ({}) is smaller than one maximum raw frame ({largest_raw_frame})",
                self.ingress.max_bytes_per_broker,
            ));
        }
        if self.ingress.progress_interval_ms == 0 {
            return Err("ingress.progress_interval_ms must be positive".to_string());
        }
        if self.health.heartbeat_timeout_ms == 0 {
            return Err("health.heartbeat_timeout_ms must be positive".to_string());
        }
        if self.protocol.ack_mode != "required" && self.protocol.ack_mode != "off" {
            return Err("protocol.ack_mode must be 'required' or 'off'".to_string());
        }
        if self.history.retentions.iter().any(|retention| retention.period_ms <= 0 || retention.slots == 0) {
            return Err("history retentions require positive period_ms and slots".to_string());
        }
        if self.display.visible_seconds == 0 {
            return Err("display.visible_seconds must be positive".to_string());
        }
        if self.display.visible_ticks == 0 {
            return Err("display.visible_ticks must be positive".to_string());
        }

        Ok(())
    }
}
