use crate::contracts::ports::SnapshotExchangePort;
use crate::contracts::types::*;
use crate::ui::chart::{
    draw_bid_ask_diff_chart, draw_candlestick_chart_multi, draw_lead_lag_view, draw_mid_diff_chart,
    draw_mid_dispersion_view, draw_move_breadth_view, draw_quote_persistence_view,
    draw_realtime_quote_path_chart, draw_spread_diff_chart, draw_state_ribbon, BottomMetric,
    ChartTheme, ChartXAxisMode,
};
use eframe::egui;
use egui::{Color32, RichText};
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
        if let Some(handler) = &self.pair_selection_handler {
            handler((a, b));
        }
    }

    pub fn bottom_metric(&self) -> BottomMetric {
        self.bottom_metric
    }

    pub fn set_bottom_metric(&mut self, metric: BottomMetric) {
        self.bottom_metric = metric;
    }

    pub fn render_ui(&mut self, ctx: &egui::Context) {
        let snapshot = self.exchange.load_latest();

        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading(
                    RichText::new("TickScope")
                        .strong()
                        .color(Color32::from_rgb(0, 200, 255)),
                );
                ui.separator();

                // Multi-broker Pair Selector
                ui.label("Focus Pair:");
                let previous_pair = self.selected_pair();
                let broker_ids: Vec<BrokerId> = snapshot
                    .broker_overviews
                    .iter()
                    .map(|b| b.broker_id)
                    .collect();

                egui::ComboBox::from_id_salt("broker_a_select")
                    .selected_text(
                        snapshot
                            .broker_overviews
                            .iter()
                            .find(|b| b.broker_id == self.selected_broker_a)
                            .map(|b| b.name.as_str())
                            .unwrap_or("Broker A"),
                    )
                    .show_ui(ui, |ui| {
                        for &bid in &broker_ids {
                            if bid != self.selected_broker_b {
                                let name = snapshot
                                    .broker_overviews
                                    .iter()
                                    .find(|b| b.broker_id == bid)
                                    .map(|b| b.name.as_str())
                                    .unwrap_or("Broker");
                                ui.selectable_value(&mut self.selected_broker_a, bid, name);
                            }
                        }
                    });

                ui.label("vs");

                egui::ComboBox::from_id_salt("broker_b_select")
                    .selected_text(
                        snapshot
                            .broker_overviews
                            .iter()
                            .find(|b| b.broker_id == self.selected_broker_b)
                            .map(|b| b.name.as_str())
                            .unwrap_or("Broker B"),
                    )
                    .show_ui(ui, |ui| {
                        for &bid in &broker_ids {
                            if bid != self.selected_broker_a {
                                let name = snapshot
                                    .broker_overviews
                                    .iter()
                                    .find(|b| b.broker_id == bid)
                                    .map(|b| b.name.as_str())
                                    .unwrap_or("Broker");
                                ui.selectable_value(&mut self.selected_broker_b, bid, name);
                            }
                        }
                    });

                if self.selected_pair() != previous_pair {
                    if let Some(handler) = &self.pair_selection_handler {
                        handler(self.selected_pair());
                    }
                }

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

                // Observed Lead/Lag Badge
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
                            "First observed on this PC: {} ({:.1} ms){}",
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
                        ui.label(RichText::new("Observed Lead: None").color(Color32::GRAY));
                    }
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.checkbox(&mut self.show_debug_overlay, "Debug");
                });
            });
        });

        // Overview panel of ALL configured brokers
        egui::TopBottomPanel::top("brokers_overview").show(ctx, |ui| {
            ui.strong("Broker Overview");
            egui::ScrollArea::vertical()
                .max_height(180.0)
                .show(ui, |ui| {
                    egui::Grid::new("broker_overview_grid")
                        .striped(true)
                        .show(ui, |ui| {
                            for header in [
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
                                ui.colored_label(status_color, status);
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

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            RichText::new("Keys: [1-7] or [Tab] to switch")
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

        // Repaint at 60 Hz
        ctx.request_repaint();
    }
}

impl eframe::App for DashboardApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.render_ui(ctx);
    }
}
