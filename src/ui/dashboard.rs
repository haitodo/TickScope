use crate::contracts::ports::SnapshotExchangePort;
use crate::contracts::types::*;
use crate::ui::chart::{
    draw_bid_ask_diff_chart, draw_candlestick_chart_multi, draw_lead_lag_view, draw_mid_diff_chart,
    draw_mid_dispersion_view, draw_move_breadth_view, draw_quote_persistence_view,
    draw_realtime_quote_path_chart, draw_spread_diff_chart, draw_state_ribbon, BottomMetric,
    ChartTheme, ChartXAxisMode,
};
use crate::ui::fonts::setup_fonts;
use crate::ui::settings::{
    save_ui_state, UiState, WindowGeometryState, MIN_WINDOW_HEIGHT, MIN_WINDOW_WIDTH,
};
use eframe::egui;
use egui::{Color32, RichText};
use std::path::PathBuf;
use std::sync::Arc;

pub struct DashboardApp {
    exchange: Arc<dyn SnapshotExchangePort>,
    selected_broker_a: BrokerId,
    selected_broker_b: BrokerId,
    selected_timeframe_ms: i64,
    show_debug_overlay: bool,
    bottom_metric: BottomMetric,
    theme: ChartTheme,
    show_candle_context: bool,
    chart_anchor: Option<f64>,
    pip_size: f64,
    visible_seconds: u64,
    visible_ticks: usize,
    x_axis_mode: ChartXAxisMode,
    pair_selection_handler: Option<Arc<dyn Fn((BrokerId, BrokerId)) + Send + Sync>>,
    fonts_configured: bool,
    show_broker_overview: bool,
    ui_state_path: Option<PathBuf>,
    window_geometry: WindowGeometryState,
    state_dirty: bool,
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
            selected_timeframe_ms: 60000,
            show_debug_overlay: false,
            bottom_metric: BottomMetric::default(),
            theme: ChartTheme::default(),
            show_candle_context: false,
            chart_anchor: None,
            pip_size: 0.01,
            visible_seconds: 60,
            visible_ticks: 1200,
            x_axis_mode: ChartXAxisMode::default(),
            pair_selection_handler: None,
            fonts_configured: false,
            show_broker_overview: false,
            ui_state_path: None,
            window_geometry: WindowGeometryState::default(),
            state_dirty: false,
        }
    }

    pub fn with_ui_state(mut self, state: &UiState) -> Self {
        self.selected_broker_a = state.active_pair.0;
        self.selected_broker_b = state.active_pair.1;
        self.show_candle_context = state.show_candle_context;
        self.selected_timeframe_ms = state.selected_timeframe_ms;
        self.x_axis_mode = state.x_axis_mode;
        self.bottom_metric = state.bottom_metric;
        self.show_broker_overview = state.show_broker_overview;
        self.window_geometry = state.window.clone();
        self
    }

    pub fn with_ui_state_path(mut self, path: PathBuf) -> Self {
        self.ui_state_path = Some(path);
        self
    }

    pub fn current_ui_state(&self) -> UiState {
        UiState {
            active_pair: (self.selected_broker_a, self.selected_broker_b),
            show_candle_context: self.show_candle_context,
            selected_timeframe_ms: self.selected_timeframe_ms,
            x_axis_mode: self.x_axis_mode,
            bottom_metric: self.bottom_metric,
            show_broker_overview: self.show_broker_overview,
            window: self.window_geometry.clone(),
        }
    }

    pub fn save_state(&mut self) {
        if let Some(path) = &self.ui_state_path {
            let state = self.current_ui_state();
            if let Err(e) = save_ui_state(path, &state) {
                log::warn!("Failed to persist UI state: {}", e);
            }
        }
        self.state_dirty = false;
    }

    pub fn mark_dirty(&mut self) {
        self.state_dirty = true;
    }

    pub fn mark_fonts_configured(&mut self) {
        self.fonts_configured = true;
    }

    pub fn with_show_broker_overview(mut self, show: bool) -> Self {
        self.show_broker_overview = show;
        self
    }

    pub fn show_broker_overview(&self) -> bool {
        self.show_broker_overview
    }

    pub fn set_show_broker_overview(&mut self, show: bool) {
        if self.show_broker_overview != show {
            self.show_broker_overview = show;
            self.state_dirty = true;
        }
    }

    pub fn with_pip_size(mut self, pip_size: f64) -> Self {
        if pip_size.is_finite() && pip_size > 0.0 {
            self.pip_size = pip_size;
        }
        self
    }

    pub fn with_visible_seconds(mut self, visible_seconds: u64) -> Self {
        self.visible_seconds = visible_seconds.max(1);
        self
    }

    pub fn with_visible_ticks(mut self, visible_ticks: usize) -> Self {
        self.visible_ticks = visible_ticks.max(1);
        self
    }

    pub fn with_pair_selection_handler(
        mut self,
        handler: Arc<dyn Fn((BrokerId, BrokerId)) + Send + Sync>,
    ) -> Self {
        self.pair_selection_handler = Some(handler);
        self
    }

    pub fn selected_pair(&self) -> (BrokerId, BrokerId) {
        (self.selected_broker_a, self.selected_broker_b)
    }

    pub fn set_selected_pair(&mut self, a: BrokerId, b: BrokerId) {
        if a == b || (a, b) == self.selected_pair() {
            return;
        }
        self.selected_broker_a = a;
        self.selected_broker_b = b;
        self.state_dirty = true;
        if let Some(handler) = &self.pair_selection_handler {
            handler((a, b));
        }
    }

    pub fn set_broker_a(&mut self, a: BrokerId) {
        if a == self.selected_broker_a {
            return;
        }
        if a == self.selected_broker_b {
            self.set_selected_pair(self.selected_broker_b, self.selected_broker_a);
        } else {
            self.set_selected_pair(a, self.selected_broker_b);
        }
    }

    pub fn set_broker_b(&mut self, b: BrokerId) {
        if b == self.selected_broker_b {
            return;
        }
        if b == self.selected_broker_a {
            self.set_selected_pair(self.selected_broker_b, self.selected_broker_a);
        } else {
            self.set_selected_pair(self.selected_broker_a, b);
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

    pub fn bottom_metric(&self) -> BottomMetric {
        self.bottom_metric
    }

    pub fn set_bottom_metric(&mut self, metric: BottomMetric) {
        if self.bottom_metric != metric {
            self.bottom_metric = metric;
            self.state_dirty = true;
        }
    }

    pub fn render_ui(&mut self, ctx: &egui::Context) {
        let prev_candle = self.show_candle_context;
        let prev_timeframe = self.selected_timeframe_ms;
        let prev_xaxis = self.x_axis_mode;
        let prev_overview = self.show_broker_overview;
        let prev_metric = self.bottom_metric;

        if !self.fonts_configured {
            setup_fonts(ctx);
            self.fonts_configured = true;
        }

        let snapshot = self.exchange.load_latest();
        let name_a = snapshot
            .broker_overviews
            .iter()
            .find(|b| b.broker_id == self.selected_broker_a)
            .map(|b| b.name.as_str())
            .unwrap_or("A");
        let name_b = snapshot
            .broker_overviews
            .iter()
            .find(|b| b.broker_id == self.selected_broker_b)
            .map(|b| b.name.as_str())
            .unwrap_or("B");

        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading(
                    RichText::new("TickScope")
                        .strong()
                        .color(Color32::from_rgb(0, 200, 255)),
                );
                ui.separator();

                // Timeframe selector
                ui.checkbox(&mut self.show_candle_context, "Candle context");
                ui.selectable_value(&mut self.selected_timeframe_ms, 60000, "M1");
                ui.selectable_value(&mut self.selected_timeframe_ms, 10000, "S10");
                ui.selectable_value(&mut self.selected_timeframe_ms, 5000, "S5");
                ui.selectable_value(&mut self.selected_timeframe_ms, 1000, "S1");

                ui.separator();

                ui.label("X:");
                egui::ComboBox::from_id_salt("chart_x_axis_mode")
                    .selected_text(self.x_axis_mode.label())
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut self.x_axis_mode,
                            ChartXAxisMode::ReceiveTime,
                            ChartXAxisMode::ReceiveTime.label(),
                        );
                        ui.selectable_value(
                            &mut self.x_axis_mode,
                            ChartXAxisMode::TickCount,
                            ChartXAxisMode::TickCount.label(),
                        );
                    });

                ui.separator();

                // Broker Overview Collapsing Toggle
                let total_brokers = snapshot.broker_overviews.len();
                let live_brokers = snapshot
                    .broker_overviews
                    .iter()
                    .filter(|b| {
                        b.health.connection == ConnectionState::Connected
                            && b.health.data_freshness == FreshnessState::Live
                    })
                    .count();

                let overview_arrow = if self.show_broker_overview { "▲" } else { "▼" };
                let overview_text = format!("{} Brokers ({}/{})", overview_arrow, live_brokers, total_brokers);
                let overview_color = if live_brokers > 0 {
                    Color32::from_rgb(0, 220, 140)
                } else {
                    Color32::from_rgb(255, 120, 120)
                };

                let toggle_btn = ui.selectable_label(
                    self.show_broker_overview,
                    RichText::new(overview_text).color(overview_color).strong(),
                );
                if toggle_btn.on_hover_text("Toggle Broker Overview table [Key: B]").clicked() {
                    self.show_broker_overview = !self.show_broker_overview;
                }

                ui.separator();

                // Observed Lead/Lag Badge with pair context
                if let Some(comp) = &snapshot.active_pair_comparison {
                    if let Some(m) = &comp.latest_match {
                        let leader_name = snapshot
                            .broker_overviews
                            .iter()
                            .find(|b| b.broker_id == m.leader)
                            .map(|b| b.name.as_str())
                            .unwrap_or("Leader");
                        let ema_text = comp
                            .ema_lead_lag_ms
                            .map(|e| format!(" (EMA: {:+.1} ms)", e))
                            .unwrap_or_default();
                        let badge = format!(
                            "Lead [{} vs {}]: {} ({:.1} ms){}",
                            name_a,
                            name_b,
                            leader_name,
                            m.raw_delta_ms.abs(),
                            ema_text
                        );
                        ui.label(
                            RichText::new(badge)
                                .strong()
                                .color(Color32::from_rgb(255, 215, 0)),
                        );
                    } else {
                        ui.label(
                            RichText::new(format!("Lead [{} vs {}]: None", name_a, name_b))
                                .color(Color32::GRAY),
                        );
                    }
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.checkbox(&mut self.show_debug_overlay, "Debug");
                });
            });
        });

        // Overview panel of ALL configured brokers (collapsible)
        if self.show_broker_overview {
            egui::TopBottomPanel::top("brokers_overview").show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.strong("Broker Overview");
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("✕ Close [B]").clicked() {
                            self.show_broker_overview = false;
                        }
                    });
                });

                egui::ScrollArea::vertical()
                    .max_height(140.0)
                    .show(ui, |ui| {
                        egui::Grid::new("broker_overview_grid")
                            .striped(true)
                            .show(ui, |ui| {
                                for header in [
                                    "Focus",
                                    "Broker",
                                    "Symbol",
                                    "Bid",
                                    "Ask",
                                    "Spread",
                                    "Quote age",
                                    "Feed",
                                    "Ticks/s",
                                ] {
                                    ui.strong(header);
                                }
                                ui.end_row();
                                for b in &snapshot.broker_overviews {
                                    let (status, status_color) = match b.health.connection {
                                        ConnectionState::Disconnected => ("DISCONNECTED", Color32::RED),
                                        ConnectionState::Connecting => ("CONNECTING", Color32::YELLOW),
                                        ConnectionState::Connected => match b.health.data_freshness {
                                            FreshnessState::Live => ("LIVE", Color32::GREEN),
                                            FreshnessState::Stale => ("STALE", Color32::YELLOW),
                                            FreshnessState::Unknown => ("WARMING", Color32::GRAY),
                                        },
                                    };
                                    let quote_is_live = b.health.connection
                                        == ConnectionState::Connected
                                        && b.health.data_freshness == FreshnessState::Live;
                                    let quote_color = if quote_is_live {
                                        Color32::WHITE
                                    } else if b.health.connection == ConnectionState::Disconnected {
                                        Color32::from_gray(90)
                                    } else {
                                        Color32::from_gray(145)
                                    };

                                    let is_a = b.broker_id == self.selected_broker_a;
                                    let is_b = b.broker_id == self.selected_broker_b;
                                    ui.horizontal(|ui| {
                                        ui.spacing_mut().item_spacing.x = 2.0;
                                        let btn_a = ui.selectable_label(
                                            is_a,
                                            RichText::new("A").strong().color(if is_a {
                                                Color32::from_rgb(0, 220, 255)
                                            } else {
                                                Color32::from_gray(100)
                                            }),
                                        );
                                        if btn_a.on_hover_text("Assign as Broker A").clicked() {
                                            self.set_broker_a(b.broker_id);
                                        }
                                        let btn_b = ui.selectable_label(
                                            is_b,
                                            RichText::new("B").strong().color(if is_b {
                                                Color32::from_rgb(255, 120, 200)
                                            } else {
                                                Color32::from_gray(100)
                                            }),
                                        );
                                        if btn_b.on_hover_text("Assign as Broker B").clicked() {
                                            self.set_broker_b(b.broker_id);
                                        }
                                    });

                                    ui.label(RichText::new(&b.name).strong().color(if quote_is_live {
                                        Color32::WHITE
                                    } else {
                                        status_color
                                    }));
                                    ui.label(RichText::new(&b.symbol).color(quote_color));
                                    if let Some(q) = &b.latest_quote {
                                        ui.label(
                                            RichText::new(format!("{:.3}", q.bid))
                                                .monospace()
                                                .color(quote_color),
                                        );
                                        ui.label(
                                            RichText::new(format!("{:.3}", q.ask))
                                                .monospace()
                                                .color(quote_color),
                                        );
                                        ui.label(
                                            RichText::new(format!("{:.3}", q.spread))
                                                .monospace()
                                                .color(quote_color),
                                        );
                                        let age_ms =
                                            snapshot.built_mono_ns.0.saturating_sub(q.rx_mono_ns.0)
                                                / 1_000_000;
                                        let age_label = if quote_is_live {
                                            format!("{} ms", age_ms)
                                        } else {
                                            format!("{} · {} ms", status, age_ms)
                                        };
                                        ui.label(RichText::new(age_label).monospace().color(
                                            if quote_is_live {
                                                Color32::from_gray(210)
                                            } else {
                                                status_color
                                            },
                                        ));
                                    } else {
                                        for _ in 0..4 {
                                            ui.label(RichText::new("—").color(quote_color));
                                        }
                                    }

                                    let status_label = ui.colored_label(status_color, status);
                                    if b.health.connection == ConnectionState::Disconnected {
                                        status_label.on_hover_text(
                                            "MT5接続待ち: 対象銘柄チャートに共通EA TickCollector を追加してください。既に動作中なら一度外して再追加してください。",
                                        );
                                    }

                                    ui.label(
                                        RichText::new(format!("{:.0}", b.tick_rate_1s))
                                            .monospace()
                                            .color(quote_color),
                                    );
                                    ui.end_row();
                                }
                            });
                    });
            });
        }

        // Keyboard Shortcuts for Bottom Metric Switching: 1-7, Tab, Shift+Tab
        egui::TopBottomPanel::bottom("state_ribbon").show(ctx, |ui| {
            draw_state_ribbon(
                ui,
                &snapshot.broker_overviews,
                &snapshot.consensus,
                &snapshot.active_clusters,
                &snapshot.current_breadth,
            );
        });
        ctx.input(|i| {
            if i.key_pressed(egui::Key::Num1) {
                self.bottom_metric = BottomMetric::MidDiff;
            } else if i.key_pressed(egui::Key::Num2) {
                self.bottom_metric = BottomMetric::BidAskDiff;
            } else if i.key_pressed(egui::Key::Num3) {
                self.bottom_metric = BottomMetric::SpreadDiff;
            } else if i.key_pressed(egui::Key::Num4) {
                self.bottom_metric = BottomMetric::LeadLag;
            } else if i.key_pressed(egui::Key::Num5) {
                self.bottom_metric = BottomMetric::MidDispersion;
            } else if i.key_pressed(egui::Key::Num6) {
                self.bottom_metric = BottomMetric::MoveBreadthView;
            } else if i.key_pressed(egui::Key::Num7) {
                self.bottom_metric = BottomMetric::QuotePersistence;
            } else if i.key_pressed(egui::Key::Tab) {
                if i.modifiers.shift {
                    self.bottom_metric = self.bottom_metric.prev();
                } else {
                    self.bottom_metric = self.bottom_metric.next();
                }
            } else if i.key_pressed(egui::Key::P) {
                let broker_ids: Vec<BrokerId> = snapshot
                    .broker_overviews
                    .iter()
                    .map(|b| b.broker_id)
                    .collect();
                self.cycle_pair(&broker_ids, !i.modifiers.shift);
            } else if i.key_pressed(egui::Key::B) {
                self.show_broker_overview = !self.show_broker_overview;
            }
        });

        // Main Charts Area
        egui::CentralPanel::default().show(ctx, |ui| {
            let available_rect = ui.available_rect_before_wrap();
            let toolbar_height = 24.0;
            let available_chart_space = (available_rect.height() - toolbar_height - 8.0).max(100.0);
            let candle_height = available_chart_space * 0.65;
            let metric_height = available_chart_space * 0.35;

            // 1. Candlestick Chart
            let candle_rect = egui::Rect::from_min_size(
                available_rect.min,
                egui::Vec2::new(available_rect.width(), candle_height),
            );
            let painter = ui.painter_at(candle_rect);
            let candle_view = snapshot
                .candle_views
                .get(&self.selected_timeframe_ms)
                .or(snapshot.active_candles.as_ref());
            let fallback_price = snapshot
                .consensus
                .as_ref()
                .and_then(|c| c.consensus_mid)
                .or_else(|| {
                    snapshot
                        .broker_overviews
                        .iter()
                        .find_map(|b| b.latest_quote.as_ref().map(|q| q.mid))
                });

            if self.show_candle_context {
                draw_candlestick_chart_multi(
                    &painter,
                    candle_rect,
                    candle_view,
                    &snapshot.broker_overviews,
                    fallback_price,
                    &self.theme,
                );
            } else {
                draw_realtime_quote_path_chart(
                    &painter,
                    candle_rect,
                    &snapshot.realtime_quote_points,
                    &snapshot.broker_overviews,
                    self.selected_pair(),
                    self.x_axis_mode,
                    snapshot.built_mono_ns,
                    self.visible_seconds,
                    self.visible_ticks,
                    self.pip_size,
                    5.0,
                    0.4,
                    &mut self.chart_anchor,
                    &self.theme,
                );
            }

            // 2. Bottom Metric Selector Toolbar
            let toolbar_rect = egui::Rect::from_min_size(
                egui::Pos2::new(available_rect.left(), candle_rect.bottom() + 4.0),
                egui::Vec2::new(available_rect.width(), toolbar_height),
            );

            ui.allocate_new_ui(egui::UiBuilder::new().max_rect(toolbar_rect), |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("Indicator:")
                            .color(Color32::from_rgb(180, 200, 220))
                            .strong(),
                    );

                    for &m in &BottomMetric::ALL {
                        let is_active = self.bottom_metric == m;
                        let text = RichText::new(m.label());
                        let rich = if is_active {
                            text.strong().color(Color32::from_rgb(0, 220, 255))
                        } else {
                            text.color(Color32::from_gray(160))
                        };
                        if ui.selectable_label(is_active, rich).clicked() {
                            self.bottom_metric = m;
                        }
                    }

                    let is_pair_metric = matches!(
                        self.bottom_metric,
                        BottomMetric::MidDiff
                            | BottomMetric::BidAskDiff
                            | BottomMetric::SpreadDiff
                            | BottomMetric::LeadLag
                    );

                    if is_pair_metric {
                        ui.separator();
                        ui.label(RichText::new("Pair:").color(Color32::from_gray(160)).small());

                        ui.menu_button(
                            RichText::new(format!("[A] {}", name_a))
                                .strong()
                                .color(Color32::from_rgb(0, 220, 255)),
                            |ui| {
                                for b in &snapshot.broker_overviews {
                                    if b.broker_id != self.selected_broker_b {
                                        if ui.selectable_label(b.broker_id == self.selected_broker_a, &b.name).clicked() {
                                            self.set_broker_a(b.broker_id);
                                            ui.close_menu();
                                        }
                                    }
                                }
                            },
                        );

                        ui.label(RichText::new("vs").color(Color32::from_gray(130)).small());

                        ui.menu_button(
                            RichText::new(format!("[B] {}", name_b))
                                .strong()
                                .color(Color32::from_rgb(255, 120, 200)),
                            |ui| {
                                for b in &snapshot.broker_overviews {
                                    if b.broker_id != self.selected_broker_a {
                                        if ui.selectable_label(b.broker_id == self.selected_broker_b, &b.name).clicked() {
                                            self.set_broker_b(b.broker_id);
                                            ui.close_menu();
                                        }
                                    }
                                }
                            },
                        );
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            RichText::new("Keys: [1-7] Metric, [P] Pair, [B] Brokers")
                                .color(Color32::from_gray(120))
                                .small(),
                        );
                    });
                });
            });

            // 3. Bottom Indicator Area
            let bottom_rect = egui::Rect::from_min_size(
                egui::Pos2::new(available_rect.left(), toolbar_rect.bottom() + 4.0),
                egui::Vec2::new(available_rect.width(), metric_height),
            );
            let bottom_painter = ui.painter_at(bottom_rect);
            let empty_series = Vec::new();
            let comparison = snapshot.active_pair_comparison.as_ref();
            let series = comparison
                .map(|c| &c.recent_diff_series)
                .unwrap_or(&empty_series);

            match self.bottom_metric {
                BottomMetric::MidDiff => {
                    draw_mid_diff_chart(
                        &bottom_painter,
                        bottom_rect,
                        series,
                        comparison,
                        self.x_axis_mode,
                        snapshot.built_mono_ns,
                        self.visible_seconds,
                        self.visible_ticks,
                        &self.theme,
                    );
                }
                BottomMetric::BidAskDiff => {
                    draw_bid_ask_diff_chart(
                        &bottom_painter,
                        bottom_rect,
                        series,
                        comparison,
                        self.x_axis_mode,
                        snapshot.built_mono_ns,
                        self.visible_seconds,
                        self.visible_ticks,
                        &self.theme,
                    );
                }
                BottomMetric::SpreadDiff => {
                    draw_spread_diff_chart(
                        &bottom_painter,
                        bottom_rect,
                        series,
                        comparison,
                        self.x_axis_mode,
                        snapshot.built_mono_ns,
                        self.visible_seconds,
                        self.visible_ticks,
                        &self.theme,
                    );
                }
                BottomMetric::LeadLag => {
                    draw_lead_lag_view(
                        &bottom_painter,
                        bottom_rect,
                        snapshot.active_pair_comparison.as_ref(),
                        &snapshot.broker_overviews,
                        &self.theme,
                    );
                }
                BottomMetric::MidDispersion => {
                    draw_mid_dispersion_view(
                        &bottom_painter,
                        bottom_rect,
                        &snapshot.consensus,
                        &snapshot.broker_overviews,
                        &self.theme,
                    );
                }
                BottomMetric::MoveBreadthView => {
                    draw_move_breadth_view(
                        &bottom_painter,
                        bottom_rect,
                        &snapshot.current_breadth,
                        &snapshot.active_clusters,
                        &snapshot.broker_overviews,
                        &self.theme,
                    );
                }
                BottomMetric::QuotePersistence => {
                    draw_quote_persistence_view(
                        &bottom_painter,
                        bottom_rect,
                        &snapshot.broker_overviews,
                        &self.theme,
                    );
                }
            }

            // Debug Overlay
            if self.show_debug_overlay {
                egui::Window::new("Debug Diagnostics")
                    .default_size([400.0, 250.0])
                    .show(ctx, |ui| {
                        ui.label(format!("Snapshot Rev: {}", snapshot.snapshot_revision));
                        ui.label(format!("Projection Rev: {}", snapshot.projection_revision));
                        ui.label(format!(
                            "Watermark ns: {}",
                            snapshot.processed_watermark_ns.0
                        ));
                        ui.label(format!("Display UTC: {}", snapshot.display_now_utc.0));
                        ui.separator();
                        ui.label("Recent Diagnostics:");
                        for d in snapshot.diagnostics.iter().rev().take(10) {
                            ui.label(format!("[{}] {}: {}", d.severity as u8, d.code, d.message));
                        }
                    });
            }
        });

        // Check if any interactive UI settings changed during this frame
        if self.show_candle_context != prev_candle
            || self.selected_timeframe_ms != prev_timeframe
            || self.x_axis_mode != prev_xaxis
            || self.show_broker_overview != prev_overview
            || self.bottom_metric != prev_metric
        {
            self.state_dirty = true;
        }

        // Track window geometry and close request
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
                    let size = [rect.width(), rect.height()];
                    if size[0] >= MIN_WINDOW_WIDTH && size[1] >= MIN_WINDOW_HEIGHT {
                        if (self.window_geometry.inner_size[0] - size[0]).abs() > 1.0
                            || (self.window_geometry.inner_size[1] - size[1]).abs() > 1.0
                        {
                            self.window_geometry.inner_size = size;
                            self.state_dirty = true;
                        }
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

        // Repaint at 60 Hz
        ctx.request_repaint();
    }
}

impl Drop for DashboardApp {
    fn drop(&mut self) {
        self.save_state();
    }
}

impl eframe::App for DashboardApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.render_ui(ctx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contracts::models::UiSnapshot;
    use crate::state::snapshot::SnapshotExchange;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn test_set_broker_a_and_b_with_swapping() {
        let exchange = Arc::new(SnapshotExchange::new(Arc::new(UiSnapshot::default())));
        let mut app = DashboardApp::new(exchange, (1, 2));

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
        let exchange = Arc::new(SnapshotExchange::new(Arc::new(UiSnapshot::default())));
        let mut app = DashboardApp::new(exchange, (1, 2));
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
        let exchange = Arc::new(SnapshotExchange::new(Arc::new(UiSnapshot::default())));
        let mut app = DashboardApp::new(exchange, (1, 2)).with_pair_selection_handler(Arc::new(
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
        let exchange = Arc::new(SnapshotExchange::new(Arc::new(UiSnapshot::default())));
        let mut app = DashboardApp::new(exchange, (1, 2));

        // Default should be collapsed (false)
        assert!(!app.show_broker_overview());

        app.set_show_broker_overview(true);
        assert!(app.show_broker_overview());

        app.set_show_broker_overview(false);
        assert!(!app.show_broker_overview());
    }
}

