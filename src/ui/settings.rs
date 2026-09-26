//! Persistent UI state management for TickScope.
//! Saves and restores UI interactive state and window geometry across application sessions.

use crate::contracts::config::BrokerConfig;
use crate::contracts::types::BrokerId;
use crate::ui::chart::{BottomMetric, ChartXAxisMode};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

pub const DEFAULT_WINDOW_WIDTH: f32 = 1100.0;
pub const DEFAULT_WINDOW_HEIGHT: f32 = 750.0;
pub const MIN_WINDOW_WIDTH: f32 = 800.0;
pub const MIN_WINDOW_HEIGHT: f32 = 500.0;
pub const VALID_TIMEFRAMES_MS: [i64; 4] = [1000, 5000, 10000, 60000];

fn default_active_pair() -> (BrokerId, BrokerId) {
    (1, 2)
}

fn default_timeframe_ms() -> i64 {
    60000
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UiState {
    #[serde(default = "default_active_pair")]
    pub active_pair: (BrokerId, BrokerId),
    #[serde(default)]
    pub show_candle_context: bool,
    #[serde(default = "default_timeframe_ms")]
    pub selected_timeframe_ms: i64,
    #[serde(default)]
    pub x_axis_mode: ChartXAxisMode,
    #[serde(default)]
    pub bottom_metric: BottomMetric,
    #[serde(default)]
    pub show_broker_overview: bool,
    #[serde(default)]
    pub window: WindowGeometryState,
}

impl Default for UiState {
    fn default() -> Self {
        Self {
            active_pair: default_active_pair(),
            show_candle_context: false,
            selected_timeframe_ms: default_timeframe_ms(),
            x_axis_mode: ChartXAxisMode::default(),
            bottom_metric: BottomMetric::default(),
            show_broker_overview: false,
            window: WindowGeometryState::default(),
        }
    }
}

impl UiState {
    pub fn sanitize(&mut self) {
        self.window.sanitize();
        if !VALID_TIMEFRAMES_MS.contains(&self.selected_timeframe_ms) {
            self.selected_timeframe_ms = default_timeframe_ms();
        }
        if self.active_pair.0 == self.active_pair.1 {
            self.active_pair = default_active_pair();
        }
    }

    /// Reconciles the loaded active broker pair with currently configured brokers.
    /// If either broker ID is not present in `brokers`, falls back to `default_pair`.
    pub fn reconcile_with_brokers(
        &mut self,
        brokers: &[BrokerConfig],
        default_pair: (BrokerId, BrokerId),
    ) {
        self.sanitize();
        let (a, b) = self.active_pair;
        let has_a = brokers.iter().any(|bk| bk.id == a);
        let has_b = brokers.iter().any(|bk| bk.id == b);
        if !has_a || !has_b || a == b {
            self.active_pair = default_pair;
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
            eprintln!("Warning: Failed to read UI state file '{}': {}", p.display(), e);
            return None;
        }
    };
    match serde_json::from_str::<UiState>(&content) {
        Ok(mut state) => {
            state.sanitize();
            Some(state)
        }
        Err(e) => {
            eprintln!("Warning: Failed to parse UI state from '{}': {}", p.display(), e);
            None
        }
    }
}

/// Saves UI state to the given JSON file path using an atomic write (temp file + rename).
pub fn save_ui_state<P: AsRef<Path>>(path: P, state: &UiState) -> Result<(), String> {
    let p = path.as_ref();
    if let Some(parent) = p.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create directory '{}': {}", parent.display(), e))?;
    }

    let json = serde_json::to_string_pretty(state)
        .map_err(|e| format!("Failed to serialize UI state to JSON: {}", e))?;

    // Atomic write to avoid partial writes on sudden termination
    let tmp_path = p.with_extension(format!("tmp.{}", std::process::id()));
    fs::write(&tmp_path, json.as_bytes())
        .map_err(|e| format!("Failed to write temporary UI state file: {}", e))?;

    if let Err(e) = fs::rename(&tmp_path, p) {
        // If rename fails (e.g. cross-filesystem or OS restriction), fallback to direct write
        let _ = fs::remove_file(&tmp_path);
        fs::write(p, json.as_bytes())
            .map_err(|err| format!("Failed to write UI state file directly after rename failed ({}): {}", e, err))?;
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
            x_axis_mode: ChartXAxisMode::TickCount,
            bottom_metric: BottomMetric::SpreadDiff,
            show_broker_overview: true,
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
    fn test_ui_state_sanitize_and_reconcile() {
        let mut state = UiState {
            active_pair: (99, 100),
            show_candle_context: false,
            selected_timeframe_ms: 42000, // Invalid timeframe
            x_axis_mode: ChartXAxisMode::ReceiveTime,
            bottom_metric: BottomMetric::MidDiff,
            show_broker_overview: false,
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
        // timeframe should sanitize to 60000
        assert_eq!(state.selected_timeframe_ms, 60000);
        // window size should sanitize to min/defaults
        assert_eq!(state.window.inner_size, [DEFAULT_WINDOW_WIDTH, DEFAULT_WINDOW_HEIGHT]);
    }

    #[test]
    fn test_corrupt_file_handling() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ui_state.json");
        fs::write(&path, "invalid json").unwrap();

        assert!(load_ui_state(&path).is_none());
    }
}
