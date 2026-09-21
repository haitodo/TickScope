use crate::contracts::ports::SnapshotExchangePort;
use crate::contracts::types::*;
use crate::ui::chart::{draw_candlestick_chart, draw_difference_chart, ChartTheme};
use eframe::egui;
use egui::{Color32, RichText};
use std::sync::Arc;

pub struct DashboardApp {
    exchange: Arc<dyn SnapshotExchangePort>,
    selected_broker_a: BrokerId,
    selected_broker_b: BrokerId,
    selected_timeframe_ms: i64,
    show_debug_overlay: bool,
    theme: ChartTheme,
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
            theme: ChartTheme::default(),
        }
    }

    pub fn selected_pair(&self) -> (BrokerId, BrokerId) {
        (self.selected_broker_a, self.selected_broker_b)
    }

    pub fn set_selected_pair(&mut self, a: BrokerId, b: BrokerId) {
        self.selected_broker_a = a;
        self.selected_broker_b = b;
    }

    pub fn render_ui(&mut self, ctx: &egui::Context) {
        let snapshot = self.exchange.load_latest();


        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading(RichText::new("TickCompare").strong().color(Color32::from_rgb(0, 200, 255)));
                ui.separator();

                // Multi-broker Pair Selector
                ui.label("Compare:");
                let broker_ids: Vec<BrokerId> = snapshot.broker_overviews.iter().map(|b| b.broker_id).collect();

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

                ui.separator();

                // Timeframe selector
                ui.selectable_value(&mut self.selected_timeframe_ms, 60000, "M1");
                ui.selectable_value(&mut self.selected_timeframe_ms, 10000, "S10");
                ui.selectable_value(&mut self.selected_timeframe_ms, 5000, "S5");
                ui.selectable_value(&mut self.selected_timeframe_ms, 1000, "S1");

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
                            "Observed Lead: {} {:+.1} ms{}",
                            leader_name, m.raw_delta_ms, ema_text
                        );
                        ui.label(RichText::new(badge).strong().color(Color32::from_rgb(255, 215, 0)));
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
            ui.horizontal_wrapped(|ui| {
                for b in &snapshot.broker_overviews {
                    ui.group(|ui| {
                        ui.horizontal(|ui| {
                            ui.strong(&b.name);
                            ui.label(format!("({})", b.symbol));

                            let (status_text, status_color) = match b.health.connection {
                                ConnectionState::Connected => ("Connected", Color32::GREEN),
                                ConnectionState::Connecting => ("Connecting", Color32::YELLOW),
                                ConnectionState::Disconnected => ("Disconnected", Color32::RED),
                            };
                            ui.colored_label(status_color, status_text);

                            if let Some(q) = &b.latest_quote {
                                ui.label(format!("Bid: {:.3} | Ask: {:.3}", q.bid, q.ask));
                                ui.label(format!("Spread: {:.3}", q.spread));
                            } else {
                                ui.label("No Quote");
                            }

                            ui.label(format!("{:.0} t/s", b.tick_rate_1s));

                            let tz_str = if b.active_utc_offset_sec == 0 {
                                "UTC".to_string()
                            } else {
                                let hours = b.active_utc_offset_sec as f64 / 3600.0;
                                if hours.fract().abs() < 1e-3 {
                                    format!("GMT{:+}", hours as i32)
                                } else {
                                    format!("GMT{:+.1}", hours)
                                }
                            };
                            let mode_tag = if b.is_auto_offset { "Auto" } else { "Manual" };
                            ui.colored_label(Color32::from_rgb(100, 180, 255), format!("[{} ({})]", tz_str, mode_tag));
                        });
                    });
                }
            });
        });

        // Main Charts Area
        egui::CentralPanel::default().show(ctx, |ui| {
            let available_rect = ui.available_rect_before_wrap();
            let chart_height = available_rect.height() * 0.65;
            let diff_height = available_rect.height() * 0.33;

            // 1. Candlestick Chart
            let candle_rect = egui::Rect::from_min_size(
                available_rect.min,
                egui::Vec2::new(available_rect.width(), chart_height),
            );
            let painter = ui.painter_at(candle_rect);
            let candle_view = snapshot
                .candle_views
                .get(&self.selected_timeframe_ms)
                .or(snapshot.active_candles.as_ref());
            let fallback_price = snapshot
                .broker_overviews
                .iter()
                .find(|b| b.broker_id == self.selected_broker_a || b.broker_id == self.selected_broker_b)
                .and_then(|b| b.latest_quote.as_ref())
                .map(|q| q.mid);

            draw_candlestick_chart(
                &painter,
                candle_rect,
                candle_view,
                self.selected_broker_a,
                self.selected_broker_b,
                fallback_price,
                &self.theme,
            );

            // 2. Price Diff Waveform Chart
            let diff_rect = egui::Rect::from_min_size(
                egui::Pos2::new(available_rect.left(), candle_rect.bottom() + 4.0),
                egui::Vec2::new(available_rect.width(), diff_height),
            );
            let diff_painter = ui.painter_at(diff_rect);
            let empty_series = Vec::new();
            let series = snapshot
                .active_pair_comparison
                .as_ref()
                .map(|c| &c.recent_diff_series)
                .unwrap_or(&empty_series);

            draw_difference_chart(&diff_painter, diff_rect, series, &self.theme);

            // Debug Overlay
            if self.show_debug_overlay {
                egui::Window::new("Debug Diagnostics")
                    .default_size([400.0, 250.0])
                    .show(ctx, |ui| {
                        ui.label(format!("Snapshot Rev: {}", snapshot.snapshot_revision));
                        ui.label(format!("Projection Rev: {}", snapshot.projection_revision));
                        ui.label(format!("Watermark ns: {}", snapshot.processed_watermark_ns.0));
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

