//! Persistent UI state management for TickScope.
//! Saves and restores UI interactive state and window geometry across application sessions.

use crate::config::BrokerConfig;
use crate::core::models::PriceMode;
use crate::core::types::BrokerId;
use crate::ui::chart::{BottomMetric, ChartXAxisMode};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use thiserror::Error;

pub const DEFAULT_WINDOW_WIDTH: f32 = 1100.0;
pub const DEFAULT_WINDOW_HEIGHT: f32 = 750.0;
pub const MIN_WINDOW_WIDTH: f32 = 800.0;
pub const MIN_WINDOW_HEIGHT: f32 = 500.0;
pub const VALID_TIMEFRAMES_MS: [i64; 4] = [1000, 5000, 10000, 60000];
pub const DEFAULT_CANDLE_BAR_WIDTH: f32 = 5.0;
pub const VALID_CANDLE_BAR_WIDTHS: [f32; 6] = [3.0, 4.0, 5.0, 6.0, 8.0, 10.0];
pub const VALID_CANDLE_FIXED_PIPS: [f64; 5] = [2.5, 5.0, 10.0, 25.0, 50.0];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CandlePriceMode {
    #[default]
    Bid,
    Mid,
}

impl CandlePriceMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Bid => "Bid",
            Self::Mid => "Mid",
        }
    }

    pub fn price_mode(self) -> PriceMode {
        match self {
            Self::Bid => PriceMode::Bid,
            Self::Mid => PriceMode::Mid,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CandlePriceScaleMode {
    #[default]
    Auto,
    Fixed(f64),
}

impl CandlePriceScaleMode {
    pub fn label(&self) -> String {
        match self {
            Self::Auto => "Auto".to_string(),
            Self::Fixed(pips) => {
                if (pips.fract()).abs() < 1e-4 {
                    format!("{:.0} pips", pips)
                } else {
                    format!("{:.1} pips", pips)
                }
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CandleFollowCriteria {
    #[default]
    Median,
    MarginEdge,
}

impl CandleFollowCriteria {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Median => "Median",
            Self::MarginEdge => "Margin Edge",
        }
    }
}

fn default_active_pair() -> (BrokerId, BrokerId) {
    (1, 2)
}

fn default_timeframe_ms() -> i64 {
    10000
}


fn default_candle_bar_width() -> f32 {
    DEFAULT_CANDLE_BAR_WIDTH
}

fn default_window_size() -> [f32; 2] {
    [DEFAULT_WINDOW_WIDTH, DEFAULT_WINDOW_HEIGHT]
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WindowGeometryState {
    #[serde(default = "default_window_size")]
    pub inner_size: [f32; 2],
    #[serde(default)]
    pub position: Option<[f32; 2]>,
    #[serde(default)]
    pub maximized: bool,
}

impl Default for WindowGeometryState {
    fn default() -> Self {
        Self {
            inner_size: default_window_size(),
            position: None,
            maximized: false,
        }
    }
}

impl WindowGeometryState {
    pub fn sanitize(&mut self) {
        if !self.inner_size[0].is_finite() || self.inner_size[0] < MIN_WINDOW_WIDTH {
            self.inner_size[0] = DEFAULT_WINDOW_WIDTH;
        }
        if !self.inner_size[1].is_finite() || self.inner_size[1] < MIN_WINDOW_HEIGHT {
            self.inner_size[1] = DEFAULT_WINDOW_HEIGHT;
        }
        if let Some(pos) = self.position {
            if !pos[0].is_finite() || !pos[1].is_finite() {
                self.position = None;
            }
        }
    }
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UiState {
    #[serde(default = "default_active_pair")]
    pub active_pair: (BrokerId, BrokerId),
    #[serde(default)]
    pub show_candle_context: bool,
    #[serde(default = "default_timeframe_ms")]
    pub selected_timeframe_ms: i64,
    #[serde(default, alias = "x_axis_mode")]
    pub top_x_axis_mode: ChartXAxisMode,
    #[serde(default)]
    pub bottom_x_axis_mode: ChartXAxisMode,
    #[serde(default)]
    pub bottom_metric: BottomMetric,
    #[serde(default)]
    pub show_broker_overview: bool,
    #[serde(default = "default_candle_bar_width")]
    pub candle_bar_width: f32,
    #[serde(default)]
    pub candle_price_scale: CandlePriceScaleMode,
    #[serde(default)]
    pub candle_price_mode: CandlePriceMode,
    #[serde(default)]
    pub candle_follow_criteria: CandleFollowCriteria,
    #[serde(default)]
    pub hidden_brokers: Vec<BrokerId>,
    #[serde(default = "default_true")]
    pub mt5_minimized: bool,
    #[serde(default)]
    pub mt5_launch_targets: Vec<BrokerId>,
    #[serde(default)]
    pub mt5_auto_launch: bool,
    #[serde(default)]
    pub mt5_auto_close: bool,
    #[serde(default)]
    pub window: WindowGeometryState,
}

impl Default for UiState {
    fn default() -> Self {
        Self {
            active_pair: default_active_pair(),
            show_candle_context: false,
            selected_timeframe_ms: default_timeframe_ms(),
            candle_bar_width: default_candle_bar_width(),
            candle_price_scale: CandlePriceScaleMode::default(),
            candle_price_mode: CandlePriceMode::default(),
            candle_follow_criteria: CandleFollowCriteria::default(),
            top_x_axis_mode: ChartXAxisMode::default(),
            bottom_x_axis_mode: ChartXAxisMode::default(),
            bottom_metric: BottomMetric::default(),
            show_broker_overview: false,
            hidden_brokers: Vec::new(),
            mt5_minimized: true,
            mt5_launch_targets: Vec::new(),
            mt5_auto_launch: false,
            mt5_auto_close: false,
            window: WindowGeometryState::default(),
        }
    }
}


impl UiState {
    pub fn is_broker_visible(&self, broker_id: BrokerId) -> bool {
        !self.hidden_brokers.contains(&broker_id)
    }

    pub fn set_broker_visible(&mut self, broker_id: BrokerId, visible: bool) {
        if visible {
            self.hidden_brokers.retain(|&id| id != broker_id);
        } else if !self.hidden_brokers.contains(&broker_id) {
            self.hidden_brokers.push(broker_id);
            self.hidden_brokers.sort_unstable();
        }
    }

    pub fn show_all_brokers(&mut self) {
        self.hidden_brokers.clear();
    }

    pub fn is_mt5_target(&self, broker_id: BrokerId) -> bool {
        self.mt5_launch_targets.contains(&broker_id)
    }

    pub fn set_mt5_target(&mut self, broker_id: BrokerId, target: bool) {
        if target {
            if !self.mt5_launch_targets.contains(&broker_id) {
                self.mt5_launch_targets.push(broker_id);
                self.mt5_launch_targets.sort_unstable();
            }
        } else {
            self.mt5_launch_targets.retain(|&id| id != broker_id);
        }
    }


    pub fn sanitize(&mut self) {
        self.window.sanitize();
        if !VALID_TIMEFRAMES_MS.contains(&self.selected_timeframe_ms) {
            self.selected_timeframe_ms = default_timeframe_ms();
        }
        if !VALID_CANDLE_BAR_WIDTHS.iter().any(|&w| (w - self.candle_bar_width).abs() < 1e-4) {
            self.candle_bar_width = default_candle_bar_width();
        }
        match self.candle_price_scale {
            CandlePriceScaleMode::Auto => {}
            CandlePriceScaleMode::Fixed(pips) => {
                if !VALID_CANDLE_FIXED_PIPS.iter().any(|&p| (p - pips).abs() < 1e-4) {
                    self.candle_price_scale = CandlePriceScaleMode::Auto;
                }
            }
        }
        if self.active_pair.0 == self.active_pair.1 {
            self.active_pair = default_active_pair();
        }
        self.hidden_brokers.sort_unstable();
        self.hidden_brokers.dedup();
    }

    /// Reconciles the loaded active broker pair with currently configured brokers.
    /// If either broker ID is not present in `brokers`, falls back to `default_pair`.
    pub fn reconcile_with_brokers(
        &mut self,
        brokers: &[BrokerConfig],
        default_pair: (BrokerId, BrokerId),
    ) {
        self.sanitize();

        // 1. Remove non-existent broker IDs from hidden_brokers
        self.hidden_brokers.retain(|&id| brokers.iter().any(|bk| bk.id == id));

        // 2. Ensure at least two brokers remain visible if at least two exist
        let visible_count = brokers.iter().filter(|bk| !self.hidden_brokers.contains(&bk.id)).count();
        if visible_count < 2 && brokers.len() >= 2 {
            self.hidden_brokers.clear();
        }

        // 3. Reconcile active pair
        let (mut a, mut b) = self.active_pair;
        let has_a = brokers.iter().any(|bk| bk.id == a && !self.hidden_brokers.contains(&bk.id));
        let has_b = brokers.iter().any(|bk| bk.id == b && !self.hidden_brokers.contains(&bk.id));

        if !has_a || !has_b || a == b {
            // Find first two visible brokers as fallback
            let visible_ids: Vec<BrokerId> = brokers
                .iter()
                .filter(|bk| !self.hidden_brokers.contains(&bk.id))
                .map(|bk| bk.id)
                .collect();
            if visible_ids.len() >= 2 {
                a = visible_ids[0];
                b = visible_ids[1];
            } else if brokers.iter().any(|bk| bk.id == default_pair.0)
                && brokers.iter().any(|bk| bk.id == default_pair.1)
                && default_pair.0 != default_pair.1
            {
                a = default_pair.0;
                b = default_pair.1;
                // Ensure default pair is visible
                self.hidden_brokers.retain(|&id| id != a && id != b);
            }
            self.active_pair = (a, b);
        }

        // 4. Reconcile MT5 launch targets
        if self.mt5_launch_targets.is_empty() {
            self.mt5_launch_targets = brokers.iter().map(|bk| bk.id).collect();
            self.mt5_launch_targets.sort_unstable();
        } else {
            self.mt5_launch_targets.retain(|&id| brokers.iter().any(|bk| bk.id == id));
        }
    }
}


/// Resolves the file path for persistent UI state.
pub fn resolve_ui_state_path(exe_dir: &Path, cwd: &Path) -> PathBuf {
    let cwd_path = cwd.join("data").join("ui_state.json");
    if cwd_path.exists() {
        return cwd_path;
    }
    let exe_path = exe_dir.join("data").join("ui_state.json");
    if exe_path.exists() {
        return exe_path;
    }
    // Default to cwd/data/ui_state.json
    cwd_path
}

/// Loads UI state from the given JSON file path.
/// Returns None if the file does not exist or fails to parse.
pub fn load_ui_state<P: AsRef<Path>>(path: P) -> Option<UiState> {
    let p = path.as_ref();
    if !p.exists() {
        return None;
    }
    let content = match fs::read_to_string(p) {
        Ok(c) => c,
        Err(e) => {
            log::warn!("Failed to read UI state file '{}': {}", p.display(), e);
            return None;
        }
    };
    match serde_json::from_str::<UiState>(&content) {
        Ok(mut state) => {
            state.sanitize();
            Some(state)
        }
        Err(e) => {
            log::warn!("Failed to parse UI state from '{}': {}", p.display(), e);
            None
        }
    }
}

#[derive(Debug, Error)]
pub enum UiStateError {
    #[error("Failed to create directory '{path}': {source}")]
    CreateDir {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("Failed to serialize UI state to JSON: {0}")]
    Serialize(#[from] serde_json::Error),
    #[error("Failed to write temporary UI state file '{path}': {source}")]
    WriteTemp {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("Failed to write UI state file directly after rename failed: {0}")]
    DirectWrite(String),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

impl From<UiStateError> for String {
    fn from(e: UiStateError) -> Self {
        e.to_string()
    }
}

/// Saves UI state to the given JSON file path using an atomic write (temp file + rename).
pub fn save_ui_state<P: AsRef<Path>>(path: P, state: &UiState) -> Result<(), UiStateError> {
    let p = path.as_ref();
    if let Some(parent) = p.parent() {
        fs::create_dir_all(parent).map_err(|e| UiStateError::CreateDir {
            path: parent.to_path_buf(),
            source: e,
        })?;
    }

    let json = serde_json::to_string_pretty(state)?;

    // Atomic write to avoid partial writes on sudden termination
    let tmp_path = p.with_extension(format!("tmp.{}", std::process::id()));
    fs::write(&tmp_path, json.as_bytes()).map_err(|e| UiStateError::WriteTemp {
        path: tmp_path.clone(),
        source: e,
    })?;

    if let Err(e) = fs::rename(&tmp_path, p) {
        // If rename fails (e.g. cross-filesystem or OS restriction), fallback to direct write
        let _ = fs::remove_file(&tmp_path);
        fs::write(p, json.as_bytes())
            .map_err(|err| UiStateError::DirectWrite(format!("{}: {}", e, err)))?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ui_state_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("data/ui_state.json");

        let original = UiState {
            active_pair: (2, 3),
            show_candle_context: true,
            selected_timeframe_ms: 10000,
            candle_bar_width: 6.0,
            candle_price_scale: CandlePriceScaleMode::Fixed(10.0),
            candle_price_mode: CandlePriceMode::Bid,
            candle_follow_criteria: CandleFollowCriteria::MarginEdge,
            top_x_axis_mode: ChartXAxisMode::TickCount,
            bottom_x_axis_mode: ChartXAxisMode::ReceiveTime,
            bottom_metric: BottomMetric::SpreadDiff,
            show_broker_overview: true,
            hidden_brokers: vec![3, 5],
            mt5_minimized: true,
            mt5_launch_targets: vec![1, 2],
            mt5_auto_launch: true,
            mt5_auto_close: false,
            window: WindowGeometryState {

                inner_size: [1280.0, 800.0],
                position: Some([100.0, 150.0]),
                maximized: false,
            },
        };

        save_ui_state(&path, &original).expect("save should succeed");
        let loaded = load_ui_state(&path).expect("load should succeed");
        assert_eq!(original, loaded);
    }

    #[test]
    fn test_ui_state_visibility_helpers() {
        let mut state = UiState::default();
        assert!(state.is_broker_visible(1));
        assert!(state.is_broker_visible(2));

        state.set_broker_visible(2, false);
        assert!(state.is_broker_visible(1));
        assert!(!state.is_broker_visible(2));
        assert_eq!(state.hidden_brokers, vec![2]);

        state.set_broker_visible(1, false);
        assert!(!state.is_broker_visible(1));
        assert!(!state.is_broker_visible(2));
        assert_eq!(state.hidden_brokers, vec![1, 2]);

        state.set_broker_visible(2, true);
        assert!(!state.is_broker_visible(1));
        assert!(state.is_broker_visible(2));
        assert_eq!(state.hidden_brokers, vec![1]);

        state.show_all_brokers();
        assert!(state.is_broker_visible(1));
        assert!(state.is_broker_visible(2));
        assert!(state.hidden_brokers.is_empty());
    }

    #[test]
    fn test_legacy_ui_state_deserialization() {
        let legacy_json = r#"{
            "active_pair": [1, 2],
            "show_candle_context": false,
            "x_axis_mode": "TickCount",
            "bottom_metric": "MidDiff"
        }"#;

        let state: UiState = serde_json::from_str(legacy_json).expect("should deserialize legacy json");
        assert_eq!(state.top_x_axis_mode, ChartXAxisMode::TickCount);
        assert_eq!(state.bottom_x_axis_mode, ChartXAxisMode::ReceiveTime);
        assert_eq!(state.bottom_metric, BottomMetric::MidDiff);
    }

    #[test]
    fn test_ui_state_sanitize_and_reconcile() {
        let mut state = UiState {
            active_pair: (99, 100),
            show_candle_context: false,
            selected_timeframe_ms: 42000, // Invalid timeframe
            candle_bar_width: 99.0,       // Invalid width -> should sanitize to default
            candle_price_scale: CandlePriceScaleMode::Fixed(99.0), // Invalid fixed pips -> should sanitize to Auto
            candle_price_mode: CandlePriceMode::Bid,
            candle_follow_criteria: CandleFollowCriteria::Median,
            top_x_axis_mode: ChartXAxisMode::ReceiveTime,
            bottom_x_axis_mode: ChartXAxisMode::ReceiveTime,
            bottom_metric: BottomMetric::MidDiff,
            show_broker_overview: false,
            hidden_brokers: Vec::new(),
            mt5_minimized: true,
            mt5_launch_targets: Vec::new(),
            mt5_auto_launch: false,
            mt5_auto_close: false,
            window: WindowGeometryState {

                inner_size: [200.0, 100.0], // Too small
                position: None,
                maximized: false,
            },
        };

        let brokers = vec![
            BrokerConfig {
                id: 1,
                name: "A".to_string(),
                ..Default::default()
            },
            BrokerConfig {
                id: 2,
                name: "B".to_string(),
                ..Default::default()
            },
        ];

        state.reconcile_with_brokers(&brokers, (1, 2));

        // active_pair should fallback to default (1, 2) since 99, 100 don't exist
        assert_eq!(state.active_pair, (1, 2));
        // timeframe should sanitize to default (10000ms / S10)
        assert_eq!(state.selected_timeframe_ms, default_timeframe_ms());
        // candle_bar_width should sanitize to default

        assert_eq!(state.candle_bar_width, DEFAULT_CANDLE_BAR_WIDTH);
        // candle_price_scale should sanitize to Auto
        assert_eq!(state.candle_price_scale, CandlePriceScaleMode::Auto);
        // window size should sanitize to min/defaults
        assert_eq!(state.window.inner_size, [DEFAULT_WINDOW_WIDTH, DEFAULT_WINDOW_HEIGHT]);

        // Reconcile with hidden brokers
        let brokers3 = vec![
            BrokerConfig { id: 1, name: "A".to_string(), ..Default::default() },
            BrokerConfig { id: 2, name: "B".to_string(), ..Default::default() },
            BrokerConfig { id: 3, name: "C".to_string(), ..Default::default() },
        ];
        state.hidden_brokers = vec![2, 99]; // 99 doesn't exist, 2 is hidden
        state.active_pair = (1, 2); // 2 is hidden, should switch to visible (1, 3)
        state.reconcile_with_brokers(&brokers3, (1, 2));
        assert_eq!(state.hidden_brokers, vec![2]);
        assert_eq!(state.active_pair, (1, 3));
    }

    #[test]
    fn test_corrupt_file_handling() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ui_state.json");
        fs::write(&path, "invalid json").unwrap();

        assert!(load_ui_state(&path).is_none());
    }

    #[test]
    fn test_candle_scale_and_width_options() {
        assert!(VALID_CANDLE_BAR_WIDTHS.contains(&10.0));
        assert!(VALID_CANDLE_FIXED_PIPS.contains(&2.5));
        assert!(VALID_CANDLE_FIXED_PIPS.contains(&25.0));
        assert!(!VALID_CANDLE_FIXED_PIPS.contains(&20.0));

        assert_eq!(CandlePriceScaleMode::Auto.label(), "Auto");
        assert_eq!(CandlePriceScaleMode::Fixed(2.5).label(), "2.5 pips");
        assert_eq!(CandlePriceScaleMode::Fixed(5.0).label(), "5 pips");
        assert_eq!(CandlePriceScaleMode::Fixed(10.0).label(), "10 pips");
        assert_eq!(CandlePriceScaleMode::Fixed(25.0).label(), "25 pips");
        assert_eq!(CandlePriceScaleMode::Fixed(50.0).label(), "50 pips");

        let mut state = UiState {
            candle_bar_width: 10.0,
            candle_price_scale: CandlePriceScaleMode::Fixed(2.5),
            ..Default::default()
        };
        state.sanitize();
        assert_eq!(state.candle_bar_width, 10.0);
        assert_eq!(state.candle_price_scale, CandlePriceScaleMode::Fixed(2.5));

        // 20.0 is no longer valid, should sanitize to Auto
        state.candle_price_scale = CandlePriceScaleMode::Fixed(20.0);
        state.sanitize();
        assert_eq!(state.candle_price_scale, CandlePriceScaleMode::Auto);
    }
}
