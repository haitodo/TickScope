pub mod broker_overview;
pub mod charts_view;
pub mod header;
pub mod latency;
pub mod quick_settings;
pub mod ribbon;

use self::ribbon::draw_state_ribbon;
use crate::core::models::{BrokerOverview, UiSnapshot};
use crate::core::ports::{ClockPort, SnapshotExchangePort};
use crate::core::types::*;
use crate::metrics::diagnostics::{DiagnosticStage, DiagnosticsHandle};
use crate::ui::chart::{BottomMetric, ChartTheme, ChartXAxisMode, MarginEdgeLatchSide};
use crate::ui::fonts::setup_fonts;
use crate::ui::settings::{
    save_ui_state, CandleFollowCriteria, CandlePriceMode, CandlePriceScaleMode, UiState,
    WindowGeometryState, DEFAULT_CANDLE_BAR_WIDTH, DEFAULT_WINDOW_HEIGHT, DEFAULT_WINDOW_WIDTH,
    MIN_WINDOW_HEIGHT, MIN_WINDOW_WIDTH,
};
use crate::ui::style;
use eframe::egui;
use egui::RichText;


use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

pub type PairSelectionHandler = Arc<dyn Fn((BrokerId, BrokerId)) + Send + Sync>;

pub struct DashboardApp {
    pub(crate) exchange: Arc<dyn SnapshotExchangePort>,
    pub(crate) selected_broker_a: BrokerId,
    pub(crate) selected_broker_b: BrokerId,
    pub(crate) selected_timeframe_ms: i64,
    pub(crate) show_debug_overlay: bool,
    pub(crate) bottom_metric: BottomMetric,
    pub(crate) theme: ChartTheme,
    pub(crate) show_candle_context: bool,
    pub(crate) candle_bar_width: f32,
    pub(crate) candle_price_scale: CandlePriceScaleMode,
    pub(crate) candle_price_mode: CandlePriceMode,
    pub(crate) candle_follow_criteria: CandleFollowCriteria,
    pub(crate) candle_chart_anchor: Option<f64>,
    pub(crate) candle_margin_edge_latch: Option<MarginEdgeLatchSide>,
    pub(crate) chart_anchor: Option<f64>,
    pub(crate) bottom_chart_anchor: Option<f64>,
    pub(crate) pip_size: f64,
    pub(crate) visible_seconds: u64,
    pub(crate) visible_ticks: usize,
    pub(crate) chart_max_quote_age_ms: u64,
    pub(crate) top_x_axis_mode: ChartXAxisMode,
    pub(crate) bottom_x_axis_mode: ChartXAxisMode,
    pub(crate) pair_selection_handler: Option<PairSelectionHandler>,
    pub(crate) diagnostics: Option<DiagnosticsHandle>,
    pub(crate) diagnostics_clock: Option<Arc<dyn ClockPort>>,
    pub(crate) last_diagnostics_snapshot_revision: u64,
    pub(crate) fonts_configured: bool,
    pub(crate) style_configured: bool,
    pub(crate) show_broker_overview: bool,
    pub(crate) show_quick_settings: bool,
    pub(crate) hidden_brokers: Vec<BrokerId>,
    pub(crate) broker_order: Vec<BrokerId>,
    pub(crate) ui_state_path: Option<PathBuf>,
    pub(crate) window_geometry: WindowGeometryState,
    pub(crate) window_reset_in_progress: u8,
    pub(crate) state_dirty: bool,
    pub(crate) repaint_registered: bool,
    pub(crate) dpi_drag_fix_installed: bool,
    pub(crate) mt5_minimized: bool,
    pub(crate) mt5_launch_targets: Vec<BrokerId>,
    pub(crate) mt5_auto_launch: bool,
    pub(crate) mt5_auto_close: bool,
    pub(crate) mt5_non_minimized_broker: Option<BrokerId>,
    pub(crate) always_on_top: bool,
    pub(crate) show_mt5_stop_confirm_modal: bool,
    pub(crate) terminal_manager: crate::runtime::TerminalManager,
    pub(crate) discovered_terminals: Vec<crate::deploy::DiscoveredTerminal>,
    pub(crate) broker_configs: Vec<crate::config::BrokerConfig>,
    pub(crate) mt5_config: crate::config::Mt5DeployConfig,
    pub(crate) trade_store: Option<Arc<parking_lot::RwLock<crate::core::models::ReplayTradeStore>>>,
    pub(crate) show_trade_overlay: bool,
    pub(crate) show_trade_history: bool,
}

impl DashboardApp {
    pub fn new(
        exchange: Arc<dyn SnapshotExchangePort>,
        initial_pair: (BrokerId, BrokerId),
    ) -> Self {
        Self {
            exchange,
            selected_broker_a: initial_pair.0,
            selected_broker_b: initial_pair.1,
            selected_timeframe_ms: 1000, // Default 1-second candles (RFC §19)
            show_debug_overlay: false,
            bottom_metric: BottomMetric::QuotePath, // Default: Realtime Quote Path
            theme: ChartTheme::default(),
            show_candle_context: true,
            candle_bar_width: DEFAULT_CANDLE_BAR_WIDTH,
            candle_price_scale: CandlePriceScaleMode::Auto,
            candle_price_mode: CandlePriceMode::Bid,
            candle_follow_criteria: CandleFollowCriteria::Median,
            candle_chart_anchor: None,
            candle_margin_edge_latch: None,
            chart_anchor: None,
            bottom_chart_anchor: None,
            pip_size: 0.001,
            visible_seconds: 60,
            visible_ticks: 300,
            chart_max_quote_age_ms: 500,
            top_x_axis_mode: ChartXAxisMode::ReceiveTime,
            bottom_x_axis_mode: ChartXAxisMode::ReceiveTime,
            pair_selection_handler: None,
            diagnostics: None,
            diagnostics_clock: None,
            last_diagnostics_snapshot_revision: 0,
            fonts_configured: false,
            style_configured: false,
            show_broker_overview: false,
            show_quick_settings: false,
            hidden_brokers: Vec::new(),
            broker_order: Vec::new(),
            ui_state_path: None,
            window_geometry: WindowGeometryState::default(),
            window_reset_in_progress: 0,
            state_dirty: false,
            repaint_registered: false,
            dpi_drag_fix_installed: false,
            mt5_minimized: true,
            mt5_launch_targets: Vec::new(),
            mt5_auto_launch: false,
            mt5_auto_close: false,
            mt5_non_minimized_broker: Some(1),
            always_on_top: false,
            show_mt5_stop_confirm_modal: false,
            terminal_manager: crate::runtime::TerminalManager::new(),
            discovered_terminals: Vec::new(),
            broker_configs: Vec::new(),
            mt5_config: crate::config::Mt5DeployConfig::default(),
            trade_store: None,
            show_trade_overlay: true,
            show_trade_history: false,
        }
    }

    pub fn with_trade_store(
        mut self,
        store: Arc<parking_lot::RwLock<crate::core::models::ReplayTradeStore>>,
    ) -> Self {
        self.trade_store = Some(store);
        self
    }

    pub fn with_ui_state(mut self, state: &UiState) -> Self {
        self.show_candle_context = state.show_candle_context;
        self.selected_timeframe_ms = state.selected_timeframe_ms;
        self.candle_bar_width = state.candle_bar_width;
        self.candle_price_scale = state.candle_price_scale;
        self.candle_price_mode = state.candle_price_mode;
        self.candle_follow_criteria = state.candle_follow_criteria;
        self.top_x_axis_mode = state.top_x_axis_mode;
        self.bottom_x_axis_mode = state.bottom_x_axis_mode;
        self.show_broker_overview = state.show_broker_overview;
        self.bottom_metric = state.bottom_metric;
        self.hidden_brokers = state.hidden_brokers.clone();
        self.broker_order = state.broker_order.clone();
        self.window_geometry = state.window.clone();
        self.selected_broker_a = state.active_pair.0;
        self.selected_broker_b = state.active_pair.1;
        self.mt5_minimized = state.mt5_minimized;
        self.mt5_launch_targets = state.mt5_launch_targets.clone();
        self.mt5_auto_launch = state.mt5_auto_launch;
        self.mt5_auto_close = state.mt5_auto_close;
        self.mt5_non_minimized_broker = state.mt5_non_minimized_broker;
        self.always_on_top = state.always_on_top;
        self
    }

    pub const fn with_chart_max_quote_age_ms(mut self, age: u64) -> Self {
        self.chart_max_quote_age_ms = age;
        self
    }

    pub fn with_ui_state_path(mut self, path: PathBuf) -> Self {
        self.ui_state_path = Some(path);
        self
    }

    pub fn with_discovered_terminals(mut self, terminals: Vec<crate::deploy::DiscoveredTerminal>) -> Self {
        self.discovered_terminals = terminals;
        self
    }

    pub fn with_broker_configs(mut self, configs: Vec<crate::config::BrokerConfig>) -> Self {
        self.broker_configs = configs;
        self
    }

    pub fn with_mt5_config(mut self, config: crate::config::Mt5DeployConfig) -> Self {
        self.mt5_config = config;
        self
    }

    pub fn is_mt5_target(&self, broker_id: BrokerId) -> bool {
        self.mt5_launch_targets.contains(&broker_id)
    }

    pub fn set_mt5_target(&mut self, broker_id: BrokerId, target: bool) {
        if target {
            if !self.mt5_launch_targets.contains(&broker_id) {
                self.mt5_launch_targets.push(broker_id);
                self.mt5_launch_targets.sort_unstable();
                self.state_dirty = true;
            }
        } else if self.mt5_launch_targets.contains(&broker_id) {
            self.mt5_launch_targets.retain(|&id| id != broker_id);
            self.state_dirty = true;
        }
    }

    pub fn select_all_mt5_targets(&mut self, all_ids: &[BrokerId]) {
        self.mt5_launch_targets = all_ids.to_vec();
        self.mt5_launch_targets.sort_unstable();
        self.state_dirty = true;
    }

    pub fn current_ui_state(&self) -> UiState {
        UiState {
            show_candle_context: self.show_candle_context,
            selected_timeframe_ms: self.selected_timeframe_ms,
            candle_bar_width: self.candle_bar_width,
            candle_price_scale: self.candle_price_scale,
            candle_price_mode: self.candle_price_mode,
            candle_follow_criteria: self.candle_follow_criteria,
            top_x_axis_mode: self.top_x_axis_mode,
            bottom_x_axis_mode: self.bottom_x_axis_mode,
            show_broker_overview: self.show_broker_overview,
            bottom_metric: self.bottom_metric,
            active_pair: self.selected_pair(),
            hidden_brokers: self.hidden_brokers.clone(),
            broker_order: self.broker_order.clone(),
            mt5_minimized: self.mt5_minimized,
            mt5_launch_targets: self.mt5_launch_targets.clone(),
            mt5_auto_launch: self.mt5_auto_launch,
            mt5_auto_close: self.mt5_auto_close,
            mt5_non_minimized_broker: self.mt5_non_minimized_broker,
            always_on_top: self.always_on_top,
            window: self.window_geometry.clone(),
        }
    }

    pub const fn always_on_top(&self) -> bool {
        self.always_on_top
    }

    pub fn set_always_on_top(&mut self, ctx: &egui::Context, enabled: bool) {
        if self.always_on_top != enabled {
            self.always_on_top = enabled;
            self.state_dirty = true;
            let level = if enabled {
                egui::WindowLevel::AlwaysOnTop
            } else {
                egui::WindowLevel::Normal
            };
            ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(level));
            self.save_state();
        }
    }

    pub fn toggle_always_on_top(&mut self, ctx: &egui::Context) {
        self.set_always_on_top(ctx, !self.always_on_top);
    }


    pub fn save_state(&mut self) {
        if let Some(path) = &self.ui_state_path {
            let state = self.current_ui_state();
            if let Err(e) = save_ui_state(path, &state) {
                log::warn!("Failed to persist UI state: {e}");
            } else {
                self.state_dirty = false;
            }
        }
    }

    /// Resets the application window size to default (1100x750),
    /// unmaximizes the window, resets zoom factor to 1.0, and saves UI state.
    pub fn reset_window_size(&mut self, ctx: &egui::Context) {
        log::info!(
            "Resetting window size to default ({DEFAULT_WINDOW_WIDTH}x{DEFAULT_WINDOW_HEIGHT})"
        );
        self.window_geometry = WindowGeometryState::default();
        self.window_reset_in_progress = 10;
        self.state_dirty = true;

        ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(false));
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(
            DEFAULT_WINDOW_WIDTH,
            DEFAULT_WINDOW_HEIGHT,
        )));
        ctx.set_zoom_factor(1.0);
        self.save_state();
    }

    pub const fn window_geometry(&self) -> &WindowGeometryState {
        &self.window_geometry
    }

    pub const fn set_window_geometry(&mut self, geometry: WindowGeometryState) {
        self.window_geometry = geometry;
        self.state_dirty = true;
    }

    pub const fn is_window_resetting(&self) -> bool {
        self.window_reset_in_progress > 0
    }

    pub const fn mark_dirty(&mut self) {
        self.state_dirty = true;
    }

    pub const fn mark_fonts_configured(&mut self) {
        self.fonts_configured = true;
    }

    pub const fn with_show_broker_overview(mut self, show: bool) -> Self {
        self.show_broker_overview = show;
        self
    }

    pub const fn show_broker_overview(&self) -> bool {
        self.show_broker_overview
    }

    pub const fn set_show_broker_overview(&mut self, show: bool) {
        if self.show_broker_overview != show {
            self.show_broker_overview = show;
            self.state_dirty = true;
        }
    }

    pub const fn show_quick_settings(&self) -> bool {
        self.show_quick_settings
    }

    pub const fn set_show_quick_settings(&mut self, show: bool) {
        self.show_quick_settings = show;
    }

    pub fn hidden_brokers(&self) -> &[BrokerId] {
        &self.hidden_brokers
    }

    pub fn is_broker_visible(&self, broker_id: BrokerId) -> bool {
        !self.hidden_brokers.contains(&broker_id)
    }

    pub fn set_broker_visible(
        &mut self,
        broker_id: BrokerId,
        visible: bool,
        available_brokers: &[BrokerOverview],
    ) {
        let all_ids: Vec<BrokerId> = available_brokers.iter().map(|b| b.broker_id).collect();
        if visible {
            if self.hidden_brokers.contains(&broker_id) {
                self.hidden_brokers.retain(|&id| id != broker_id);
                self.state_dirty = true;
            }
        } else {
            // Guard: Keep at least 2 brokers visible if 2 or more exist in available_brokers
            let current_visible_count = all_ids
                .iter()
                .filter(|&id| !self.hidden_brokers.contains(id))
                .count();
            if current_visible_count <= 2 && all_ids.len() >= 2 {
                return;
            }
            if current_visible_count <= 1 {
                return;
            }
            if !self.hidden_brokers.contains(&broker_id) {
                self.hidden_brokers.push(broker_id);
                self.hidden_brokers.sort_unstable();
                self.state_dirty = true;

                // If currently selected broker A or B is hidden, switch to another visible broker
                let remaining_visible: Vec<BrokerId> = all_ids
                    .iter()
                    .copied()
                    .filter(|id| !self.hidden_brokers.contains(id))
                    .collect();

                if self.selected_broker_a == broker_id {
                    if let Some(&new_a) = remaining_visible.iter().find(|&&id| id != self.selected_broker_b) {
                        self.set_broker_a(new_a);
                    }
                } else if self.selected_broker_b == broker_id {
                    if let Some(&new_b) = remaining_visible.iter().find(|&&id| id != self.selected_broker_a) {
                        self.set_broker_b(new_b);
                    }
                }
            }
        }
    }

    pub fn show_all_brokers(&mut self) {
        if !self.hidden_brokers.is_empty() {
            self.hidden_brokers.clear();
            self.state_dirty = true;
        }
    }

    pub fn broker_ids_in_order(&mut self, overviews: &[BrokerOverview]) -> Vec<BrokerId> {
        for overview in overviews {
            if !self.broker_order.contains(&overview.broker_id) {
                self.broker_order.push(overview.broker_id);
                self.state_dirty = true;
            }
        }

        let mut ordered_ids = Vec::with_capacity(overviews.len());
        for &broker_id in &self.broker_order {
            if overviews.iter().any(|overview| overview.broker_id == broker_id)
                && !ordered_ids.contains(&broker_id)
            {
                ordered_ids.push(broker_id);
            }
        }
        ordered_ids
    }

    pub fn visible_broker_ids(&mut self, overviews: &[BrokerOverview]) -> Vec<BrokerId> {
        self.broker_ids_in_order(overviews)
            .into_iter()
            .filter(|&id| self.is_broker_visible(id))
            .collect()
    }

    pub fn move_broker_order(&mut self, broker_id: BrokerId, target_id: BrokerId, after: bool) {
        if broker_id == target_id {
            return;
        }

        let previous_order = self.broker_order.clone();
        let Some(source_index) = self.broker_order.iter().position(|&id| id == broker_id) else {
            return;
        };
        let source_id = self.broker_order.remove(source_index);
        let Some(target_index) = self.broker_order.iter().position(|&id| id == target_id) else {
            self.broker_order.insert(source_index, source_id);
            return;
        };
        let insertion_index = target_index + usize::from(after);
        self.broker_order.insert(insertion_index, source_id);

        if self.broker_order != previous_order {
            self.state_dirty = true;
        }
    }

    pub fn reset_broker_order(&mut self, overviews: &[BrokerOverview]) {
        let default_order: Vec<BrokerId> = overviews.iter().map(|b| b.broker_id).collect();
        if self.broker_order != default_order {
            self.broker_order = default_order;
            self.state_dirty = true;
        }
    }

    pub const fn with_pip_size(mut self, pip_size: f64) -> Self {
        self.pip_size = pip_size;
        self
    }

    pub const fn with_visible_seconds(mut self, visible_seconds: u64) -> Self {
        self.visible_seconds = visible_seconds;
        self
    }

    pub const fn with_visible_ticks(mut self, visible_ticks: usize) -> Self {
        self.visible_ticks = visible_ticks;
        self
    }

    pub fn with_pair_selection_handler(
        mut self,
        handler: Arc<dyn Fn((BrokerId, BrokerId)) + Send + Sync>,
    ) -> Self {
        self.pair_selection_handler = Some(handler);
        self
    }

    pub fn with_diagnostics(
        mut self,
        diagnostics: DiagnosticsHandle,
        clock: Arc<dyn ClockPort>,
    ) -> Self {
        self.diagnostics = Some(diagnostics);
        self.diagnostics_clock = Some(clock);
        self
    }

    pub const fn selected_pair(&self) -> (BrokerId, BrokerId) {
        (self.selected_broker_a, self.selected_broker_b)
    }

    pub fn set_selected_pair(&mut self, a: BrokerId, b: BrokerId) {
        if self.selected_broker_a != a || self.selected_broker_b != b {
            self.selected_broker_a = a;
            self.selected_broker_b = b;
            self.state_dirty = true;
            if let Some(handler) = &self.pair_selection_handler {
                handler((a, b));
            }
        }
    }

    pub fn set_broker_a(&mut self, a: BrokerId) {
        let (current_a, current_b) = self.selected_pair();
        if a == current_b {
            self.set_selected_pair(current_b, current_a);
        } else if a != current_a {
            self.set_selected_pair(a, current_b);
        }
    }

    pub fn set_broker_b(&mut self, b: BrokerId) {
        let (current_a, current_b) = self.selected_pair();
        if b == current_a {
            self.set_selected_pair(current_b, current_a);
        } else if b != current_b {
            self.set_selected_pair(current_a, b);
        }
    }

    pub fn cycle_pair(&mut self, broker_ids: &[BrokerId], forward: bool) {
        if broker_ids.len() < 2 {
            return;
        }
        let mut pairs = Vec::new();
        for &a in broker_ids {
            for &b in broker_ids {
                if a != b {
                    pairs.push((a, b));
                }
            }
        }
        if pairs.is_empty() {
            return;
        }
        let current = self.selected_pair();
        let current_idx = pairs.iter().position(|&p| p == current).unwrap_or(0);
        let next_idx = if forward {
            (current_idx + 1) % pairs.len()
        } else {
            (current_idx + pairs.len() - 1) % pairs.len()
        };
        let (next_a, next_b) = pairs[next_idx];
        self.set_selected_pair(next_a, next_b);
    }

    pub const fn bottom_metric(&self) -> BottomMetric {
        self.bottom_metric
    }

    pub fn set_bottom_metric(&mut self, metric: BottomMetric) {
        if self.bottom_metric != metric {
            self.bottom_metric = metric;
            self.state_dirty = true;
        }
    }

    pub const fn candle_bar_width(&self) -> f32 {
        self.candle_bar_width
    }

    pub fn set_candle_bar_width(&mut self, width: f32) {
        if (self.candle_bar_width - width).abs() > 1e-4 {
            self.candle_bar_width = width;
            self.state_dirty = true;
        }
    }

    pub const fn candle_price_scale(&self) -> CandlePriceScaleMode {
        self.candle_price_scale
    }

    pub fn set_candle_price_scale(&mut self, mode: CandlePriceScaleMode) {
        if self.candle_price_scale != mode {
            self.candle_price_scale = mode;
            self.candle_chart_anchor = None;
            self.candle_margin_edge_latch = None;
            self.state_dirty = true;
        }
    }

    pub const fn candle_price_mode(&self) -> CandlePriceMode {
        self.candle_price_mode
    }

    pub fn set_candle_price_mode(&mut self, mode: CandlePriceMode) {
        if self.candle_price_mode != mode {
            self.candle_price_mode = mode;
            self.candle_chart_anchor = None;
            self.candle_margin_edge_latch = None;
            self.state_dirty = true;
        }
    }

    pub const fn candle_follow_criteria(&self) -> CandleFollowCriteria {
        self.candle_follow_criteria
    }

    pub fn set_candle_follow_criteria(&mut self, criteria: CandleFollowCriteria) {
        if self.candle_follow_criteria != criteria {
            self.candle_follow_criteria = criteria;
            self.candle_margin_edge_latch = None;
            self.state_dirty = true;
        }
    }

    pub const fn top_x_axis_mode(&self) -> ChartXAxisMode {
        self.top_x_axis_mode
    }

    pub fn set_top_x_axis_mode(&mut self, mode: ChartXAxisMode) {
        if self.top_x_axis_mode != mode {
            self.top_x_axis_mode = mode;
            self.state_dirty = true;
        }
    }

    pub const fn bottom_x_axis_mode(&self) -> ChartXAxisMode {
        self.bottom_x_axis_mode
    }

    pub fn set_bottom_x_axis_mode(&mut self, mode: ChartXAxisMode) {
        if self.bottom_x_axis_mode != mode {
            self.bottom_x_axis_mode = mode;
            self.state_dirty = true;
        }
    }

    pub const fn x_axis_mode(&self) -> ChartXAxisMode {
        self.top_x_axis_mode
    }

    pub fn set_x_axis_mode(&mut self, mode: ChartXAxisMode) {
        self.set_top_x_axis_mode(mode);
    }

    pub fn render_ui(&mut self, ctx: &egui::Context) {
        let ui_render_start = self.diagnostics.as_ref().map(|_| Instant::now());
        let prev_candle = self.show_candle_context;
        let prev_timeframe = self.selected_timeframe_ms;
        let prev_candle_bar_width = self.candle_bar_width;
        let prev_candle_scale = self.candle_price_scale;
        let prev_candle_follow = self.candle_follow_criteria;
        let prev_top_xaxis = self.top_x_axis_mode;
        let prev_bottom_xaxis = self.bottom_x_axis_mode;
        let prev_overview = self.show_broker_overview;
        let prev_metric = self.bottom_metric;
        let prev_mt5_minimized = self.mt5_minimized;
        let prev_mt5_auto_launch = self.mt5_auto_launch;
        let prev_mt5_auto_close = self.mt5_auto_close;
        let prev_mt5_non_minimized_broker = self.mt5_non_minimized_broker;
        let prev_always_on_top = self.always_on_top;

        // Poll MT5 process statuses periodically (throttled to 1s internally)
        self.terminal_manager
            .poll_status(&self.broker_configs, &self.discovered_terminals, false);

        if !self.fonts_configured {

            setup_fonts(ctx);
            self.fonts_configured = true;
        }
        if !self.style_configured {
            style::configure(ctx);
            self.style_configured = true;
        }
        if !self.repaint_registered {
            let ctx_clone = ctx.clone();
            self.exchange.register_repaint_signal(Arc::new(move || {
                ctx_clone.request_repaint();
            }));
            self.repaint_registered = true;
        }

        let snapshot = self.exchange.load_latest();
        if let (Some(diagnostics), Some(clock)) = (&self.diagnostics, &self.diagnostics_clock) {
            if snapshot.snapshot_revision != self.last_diagnostics_snapshot_revision {
                diagnostics.record_ns(
                    DiagnosticStage::SnapshotToUi,
                    clock
                        .sample()
                        .mono_ns
                        .saturating_sub(snapshot.built_mono_ns)
                        .0,
                );
                self.last_diagnostics_snapshot_revision = snapshot.snapshot_revision;
            }
        }
        let name_a = snapshot
            .broker_overviews
            .iter()
            .find(|b| b.broker_id == self.selected_broker_a)
            .map_or("A", |b| b.name.as_str());
        let name_b = snapshot
            .broker_overviews
            .iter()
            .find(|b| b.broker_id == self.selected_broker_b)
            .map_or("B", |b| b.name.as_str());

        // 1. Top Panel: Header context controls and HUD
        header::render_top_header(self, ctx, &snapshot, name_a, name_b);

        // 2. Broker Overview panel of ALL configured brokers (collapsible)
        broker_overview::render_broker_overview(self, ctx, &snapshot);

        // 3. State Ribbon at Bottom
        egui::TopBottomPanel::bottom("state_ribbon").show(ctx, |ui| {
            draw_state_ribbon(
                ui,
                &snapshot.broker_overviews,
                &snapshot.consensus,
                &snapshot.active_clusters,
                &snapshot.current_breadth,
            );
        });

        // 4. Keyboard Shortcuts
        self.handle_hotkeys(ctx, &snapshot);

        // 5. Main Charts Area
        charts_view::render_charts_view(self, ctx, &snapshot, name_a, name_b);

        // Check if any interactive UI settings changed during this frame
        if self.show_candle_context != prev_candle
            || self.selected_timeframe_ms != prev_timeframe
            || (self.candle_bar_width - prev_candle_bar_width).abs() > 1e-4
            || self.candle_price_scale != prev_candle_scale
            || self.candle_follow_criteria != prev_candle_follow
            || self.top_x_axis_mode != prev_top_xaxis
            || self.bottom_x_axis_mode != prev_bottom_xaxis
            || self.show_broker_overview != prev_overview
            || self.bottom_metric != prev_metric
            || self.mt5_minimized != prev_mt5_minimized
            || self.mt5_auto_launch != prev_mt5_auto_launch
            || self.mt5_auto_close != prev_mt5_auto_close
            || self.mt5_non_minimized_broker != prev_mt5_non_minimized_broker
            || self.always_on_top != prev_always_on_top
        {
            if self.candle_price_scale != prev_candle_scale {
                self.candle_chart_anchor = None;
                self.candle_margin_edge_latch = None;
            }
            if self.candle_follow_criteria != prev_candle_follow {
                self.candle_margin_edge_latch = None;
            }
            if self.always_on_top != prev_always_on_top {
                let level = if self.always_on_top {
                    egui::WindowLevel::AlwaysOnTop
                } else {
                    egui::WindowLevel::Normal
                };
                ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(level));
            }
            self.state_dirty = true;
        }

        // 6. MT5 Termination Confirmation Modal
        if self.show_mt5_stop_confirm_modal {
            let mut close_modal = false;
            let running_targets: Vec<BrokerId> = self
                .mt5_launch_targets
                .iter()
                .copied()
                .filter(|&id| self.terminal_manager.get_status(id).is_running())
                .collect();
            let running_names: Vec<String> = running_targets
                .iter()
                .map(|&id| {
                    self.broker_configs
                        .iter()
                        .find(|b| b.id == id).map_or_else(|| format!("Broker {id}"), |b| b.name.clone())
                })
                .collect();

            egui::Window::new("MT5終了の確認")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
                .show(ctx, |ui| {
                    ui.spacing_mut().item_spacing.y = 8.0;
                    ui.label(RichText::new("起動中のMT5端末を終了しますか？").strong());
                    if running_names.is_empty() {
                        ui.label("現在起動中の対象MT5はありません。");
                    } else {
                        ui.label(format!("対象: {}", running_names.join(", ")));
                    }
                    ui.label(
                        RichText::new("各端末にWM_CLOSE（正常終了）を送信し、チャート設定やEA状態を安全に保存して終了します。")
                            .small()
                            .color(crate::ui::style::TEXT_SUBDUED),
                    );
                    ui.horizontal(|ui| {
                        if ui
                            .button(
                                RichText::new("⏹ 終了する")
                                    .color(crate::ui::style::STATUS_ALERT)
                                    .strong(),
                            )
                            .clicked()
                        {
                            self.terminal_manager
                                .stop_multiple(&running_targets, std::time::Duration::from_secs(5));
                            close_modal = true;
                        }
                        if ui.button("キャンセル").clicked() {
                            close_modal = true;
                        }
                    });
                });

            if close_modal {
                self.show_mt5_stop_confirm_modal = false;
            }
        }

        // 7. Track window geometry and auto-persist state
        self.track_window_geometry(ctx);


        if let (Some(diagnostics), Some(start)) = (&self.diagnostics, ui_render_start) {
            diagnostics.record_duration(DiagnosticStage::UiRenderWork, start.elapsed());
        }

        // When a new snapshot is published, the exchange triggers request_repaint()
        // immediately. For fallback (clock ticks, data freshness transitions),
        // request repaint after an idle timeout.
        ctx.request_repaint_after(std::time::Duration::from_millis(200));
    }

    fn handle_hotkeys(&mut self, ctx: &egui::Context, snapshot: &UiSnapshot) {
        let mut reset_size_requested = false;
        let mut toggle_always_on_top = false;
        ctx.input(|i| {
            if (i.modifiers.command || i.modifiers.ctrl) && i.key_pressed(egui::Key::Num0) {
                reset_size_requested = true;
            } else if i.key_pressed(egui::Key::Num2) {
                self.bottom_metric = BottomMetric::MidDiff;
            } else if i.key_pressed(egui::Key::Num3) {
                self.bottom_metric = BottomMetric::BidAskDiff;
            } else if i.key_pressed(egui::Key::Num4) {
                self.bottom_metric = BottomMetric::SpreadDiff;
            } else if i.key_pressed(egui::Key::Num5) {
                self.bottom_metric = BottomMetric::LeadLag;
            } else if i.key_pressed(egui::Key::Num6) {
                self.bottom_metric = BottomMetric::MidDispersion;
            } else if i.key_pressed(egui::Key::Num7) {
                self.bottom_metric = BottomMetric::MoveBreadthView;
            } else if i.key_pressed(egui::Key::Num8) {
                self.bottom_metric = BottomMetric::QuotePersistence;
            } else if i.key_pressed(egui::Key::Num1) {
                self.bottom_metric = BottomMetric::QuotePath;
            } else if i.key_pressed(egui::Key::Tab) {
                if i.modifiers.shift {
                    self.bottom_metric = self.bottom_metric.prev();
                } else {
                    self.bottom_metric = self.bottom_metric.next();
                }
            } else if i.key_pressed(egui::Key::V) || ((i.modifiers.command || i.modifiers.ctrl) && i.key_pressed(egui::Key::P)) {
                self.show_trade_overlay = !self.show_trade_overlay;
            } else if i.key_pressed(egui::Key::P) {
                let broker_ids: Vec<BrokerId> = self.visible_broker_ids(&snapshot.broker_overviews);
                self.cycle_pair(&broker_ids, !i.modifiers.shift);
            } else if i.key_pressed(egui::Key::B) {
                self.show_broker_overview = !self.show_broker_overview;
            } else if i.key_pressed(egui::Key::S) || i.key_pressed(egui::Key::Comma) {
                self.show_quick_settings = !self.show_quick_settings;
            } else if i.key_pressed(egui::Key::T) {
                toggle_always_on_top = true;
            } else if i.key_pressed(egui::Key::Escape) {
                if self.show_quick_settings {
                    self.show_quick_settings = false;
                } else if self.show_broker_overview {
                    self.show_broker_overview = false;
                }
            }
        });

        if toggle_always_on_top {
            self.toggle_always_on_top(ctx);
        }

        if reset_size_requested {
            self.reset_window_size(ctx);
        }
    }

    fn track_window_geometry(&mut self, ctx: &egui::Context) {
        if self.window_reset_in_progress > 0 {
            self.window_reset_in_progress -= 1;
            if self.state_dirty || ctx.input(|i| i.viewport().close_requested()) {
                self.save_state();
            }
            return;
        }

        let current_ppp = ctx.pixels_per_point();

        // No manual DPI compensation is needed here.  winit handles
        // WM_DPICHANGED on Windows (Per-Monitor DPI V2) and applies the
        // OS-suggested window rect, which preserves the window's visual
        // (inch) size across monitors — matching Explorer and other native
        // apps.  The logical size stays constant; the physical pixel count
        // scales with newDpi / oldDpi automatically.

        ctx.input(|i| {
            let vp = i.viewport();
            if let Some(maximized) = vp.maximized {
                if self.window_geometry.maximized != maximized {
                    self.window_geometry.maximized = maximized;
                    self.state_dirty = true;
                }
            }
            if !self.window_geometry.maximized {
                if let Some(rect) = vp.inner_rect {
                    let logical_size = [rect.width(), rect.height()];
                    let physical_size = [rect.width() * current_ppp, rect.height() * current_ppp];
                    if logical_size[0] >= MIN_WINDOW_WIDTH && logical_size[1] >= MIN_WINDOW_HEIGHT
                        && ((self.window_geometry.inner_size[0] - logical_size[0]).abs() > 1.0
                            || (self.window_geometry.inner_size[1] - logical_size[1]).abs() > 1.0)
                    {
                        self.window_geometry.inner_size = logical_size;
                        self.window_geometry.physical_inner_size = Some(physical_size);
                        self.state_dirty = true;
                    }
                }
                if let Some(rect) = vp.outer_rect {
                    let pos = [rect.min.x, rect.min.y];
                    if self.window_geometry.position != Some(pos) {
                        self.window_geometry.position = Some(pos);
                        self.state_dirty = true;
                    }
                }
            }
        });

        if self.state_dirty || ctx.input(|i| i.viewport().close_requested()) {
            self.save_state();
        }
    }
}

impl Drop for DashboardApp {
    fn drop(&mut self) {
        self.save_state();
    }
}

impl eframe::App for DashboardApp {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        #[cfg(windows)]
        if !self.dpi_drag_fix_installed {
            self.dpi_drag_fix_installed = true;
            use raw_window_handle::{HasWindowHandle, RawWindowHandle};
            if let Ok(handle) = frame.window_handle() {
                if let RawWindowHandle::Win32(win32) = handle.as_ref() {
                    crate::ui::dpi::install_dpi_drag_fix(win32.hwnd.get());
                }
            }
        }
        self.render_ui(ctx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::test_support::headless_app;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn test_set_broker_a_and_b_with_swapping() {
        let mut app = headless_app();

        assert_eq!(app.selected_pair(), (1, 2));

        // Change A to 3
        app.set_broker_a(3);
        assert_eq!(app.selected_pair(), (3, 2));

        // Setting A to 2 (current B) should swap them
        app.set_broker_a(2);
        assert_eq!(app.selected_pair(), (2, 3));

        // Setting B to 2 (current A) should swap them
        app.set_broker_b(2);
        assert_eq!(app.selected_pair(), (3, 2));
    }

    #[test]
    fn test_cycle_pair_forward_and_backward() {
        let mut app = headless_app();
        let brokers = vec![1, 2, 3];

        // Forward cycling
        app.cycle_pair(&brokers, true);
        assert_eq!(app.selected_pair(), (1, 3));
        app.cycle_pair(&brokers, true);
        assert_eq!(app.selected_pair(), (2, 1));
        app.cycle_pair(&brokers, true);
        assert_eq!(app.selected_pair(), (2, 3));

        // Backward cycling
        app.cycle_pair(&brokers, false);
        assert_eq!(app.selected_pair(), (2, 1));
    }

    #[test]
    fn test_pair_selection_handler_called() {
        let call_count = Arc::new(AtomicUsize::new(0));
        let count_clone = Arc::clone(&call_count);
        let mut app = headless_app().with_pair_selection_handler(Arc::new(
            move |_| {
                count_clone.fetch_add(1, Ordering::SeqCst);
            },
        ));

        app.set_broker_a(3);
        assert_eq!(call_count.load(Ordering::SeqCst), 1);

        app.set_broker_b(1);
        assert_eq!(call_count.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn test_show_broker_overview_toggle() {
        let mut app = headless_app();

        // Default should be collapsed (false)
        assert!(!app.show_broker_overview());

        app.set_show_broker_overview(true);
        assert!(app.show_broker_overview());

        app.set_show_broker_overview(false);
        assert!(!app.show_broker_overview());
    }

    #[test]
    fn test_show_quick_settings_toggle() {
        let mut app = headless_app();

        // Default should be closed (false)
        assert!(!app.show_quick_settings());

        app.set_show_quick_settings(true);
        assert!(app.show_quick_settings());

        app.set_show_quick_settings(false);
        assert!(!app.show_quick_settings());
    }

    #[test]
    fn test_quick_settings_keyboard_shortcuts() {
        let mut app = headless_app();

        let ctx = egui::Context::default();

        // 1. Press S -> should open quick settings
        let mut input_s = egui::RawInput::default();
        input_s.events.push(egui::Event::Key {
            key: egui::Key::S,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        });
        let _ = ctx.run(input_s, |ctx| {
            app.render_ui(ctx);
        });
        assert!(app.show_quick_settings());

        // 2. Press Escape -> should close quick settings
        let mut input_esc = egui::RawInput::default();
        input_esc.events.push(egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        });
        let _ = ctx.run(input_esc, |ctx| {
            app.render_ui(ctx);
        });
        assert!(!app.show_quick_settings());

        // 3. Press Comma -> should open quick settings
        let mut input_comma = egui::RawInput::default();
        input_comma.events.push(egui::Event::Key {
            key: egui::Key::Comma,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        });
        let _ = ctx.run(input_comma, |ctx| {
            app.render_ui(ctx);
        });
        assert!(app.show_quick_settings());
    }
}
