//! Central panel chart rendering, bottom indicator toolbar, and metric displays.

use super::quick_settings;
use super::DashboardApp;
use crate::core::models::UiSnapshot;
use crate::ui::settings::CandlePriceMode;
use crate::ui::chart::{
    draw_bid_ask_diff_chart, draw_candlestick_chart_for_brokers, draw_lead_lag_view,
    draw_mid_diff_chart, draw_mid_dispersion_view_with_visibility, draw_move_breadth_view,
    draw_quote_persistence_view_with_visibility, draw_realtime_quote_path_chart_with_visibility,
    draw_spread_diff_chart, BottomMetric, BottomMetricCategory, ChartXAxisMode,
};
use eframe::egui;
use egui::{Color32, RichText};

/// Render the central area containing the top chart (candlestick or quote path),
/// the metric toolbar selector, and the chosen bottom indicator chart.
pub fn render_charts_view(
    app: &mut DashboardApp,
    ctx: &egui::Context,
    snapshot: &UiSnapshot,
    name_a: &str,
    name_b: &str,
) {
    egui::CentralPanel::default().show(ctx, |ui| {
        let available_rect = ui.available_rect_before_wrap();
        let toolbar_height = 24.0;
        let available_chart_space = (available_rect.height() - toolbar_height - 8.0).max(100.0);
        let candle_height = available_chart_space * 0.65;
        let metric_height = available_chart_space * 0.35;

        // 1. Candlestick Chart / Realtime Path Chart
        let candle_rect = egui::Rect::from_min_size(
            available_rect.min,
            egui::Vec2::new(available_rect.width(), candle_height),
        );
        let painter = ui.painter_at(candle_rect);
        let candle_view = match app.candle_price_mode {
            CandlePriceMode::Bid => snapshot
                .candle_views
                .get(&app.selected_timeframe_ms)
                .or(snapshot.active_candles.as_ref()),
            CandlePriceMode::Mid => snapshot.mid_candle_views.get(&app.selected_timeframe_ms),
        };
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

        let visible_broker_ids = app.visible_broker_ids(&snapshot.broker_overviews);

        if app.show_candle_context {
            draw_candlestick_chart_for_brokers(
                &painter,
                candle_rect,
                candle_view,
                &visible_broker_ids,
                &snapshot.broker_overviews,
                app.candle_bar_width,
                app.candle_price_scale,
                app.candle_follow_criteria,
                app.pip_size,
                &mut app.candle_chart_anchor,
                &mut app.candle_margin_edge_latch,
                fallback_price,
                app.chart_max_quote_age_ms,
                snapshot.built_mono_ns,
                app.candle_price_mode.price_mode(),
                &app.theme,
            );
        } else {
            draw_realtime_quote_path_chart_with_visibility(
                &painter,
                candle_rect,
                &snapshot.realtime_quote_points,
                &snapshot.broker_overviews,
                Some(&visible_broker_ids),
                app.selected_pair(),
                app.top_x_axis_mode,
                snapshot.built_mono_ns,
                app.visible_seconds,
                app.visible_ticks,
                app.pip_size,
                5.0,
                0.4,
                &mut app.chart_anchor,
                &app.theme,
            );
        }

        // 2. Bottom Metric Selector Toolbar
        let toolbar_rect = egui::Rect::from_min_size(
            egui::Pos2::new(available_rect.left(), candle_rect.bottom() + 4.0),
            egui::Vec2::new(available_rect.width(), toolbar_height),
        );

        ui.allocate_new_ui(egui::UiBuilder::new().max_rect(toolbar_rect), |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;

                // Indicator Selector (ComboBox + Prev/Next buttons)
                ui.label(
                    RichText::new("Indicator:")
                        .color(Color32::from_rgb(180, 200, 220))
                        .strong(),
                );

                let prev_btn = ui.small_button("◀");
                if prev_btn
                    .on_hover_text(format!(
                        "Previous metric: {} [Shift+Tab]",
                        app.bottom_metric.prev().label()
                    ))
                    .clicked()
                {
                    app.bottom_metric = app.bottom_metric.prev();
                }

                egui::ComboBox::from_id_salt("bottom_metric_combobox")
                    .selected_text(
                        RichText::new(app.bottom_metric.label())
                            .strong()
                            .color(Color32::from_rgb(0, 220, 255)),
                    )
                    .width(155.0)
                    .show_ui(ui, |ui| {
                        for &category in &BottomMetricCategory::ALL {
                            ui.label(
                                RichText::new(category.title())
                                    .small()
                                    .strong()
                                    .color(Color32::from_rgb(180, 200, 220)),
                            );
                            for &m in category.metrics() {
                                let is_active = app.bottom_metric == m;
                                let text = RichText::new(m.label());
                                let rich = if is_active {
                                    text.strong().color(Color32::from_rgb(0, 220, 255))
                                } else {
                                    text.color(Color32::WHITE)
                                };
                                let resp = ui.selectable_label(is_active, rich);
                                if resp
                                    .on_hover_text(format!("{} [Key: {}]", m.title(), m.key_number()))
                                    .clicked()
                                {
                                    app.bottom_metric = m;
                                }
                            }
                            ui.separator();
                        }
                    });

                let next_btn = ui.small_button("▶");
                if next_btn
                    .on_hover_text(format!(
                        "Next metric: {} [Tab]",
                        app.bottom_metric.next().label()
                    ))
                    .clicked()
                {
                    app.bottom_metric = app.bottom_metric.next();
                }

                // Pair Selector (Pair metrics 1-4)
                if app.bottom_metric.is_pair_metric() {
                    ui.separator();
                    ui.label(RichText::new("Pair:").color(Color32::from_gray(160)).small());

                    ui.menu_button(
                        RichText::new(format!("[A] {}", name_a))
                            .strong()
                            .color(Color32::from_rgb(0, 220, 255)),
                        |ui| {
                            for b in &snapshot.broker_overviews {
                                if b.broker_id != app.selected_broker_b
                                    && ui.selectable_label(b.broker_id == app.selected_broker_a, &b.name).clicked()
                                {
                                    app.set_broker_a(b.broker_id);
                                    ui.close_menu();
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
                                if b.broker_id != app.selected_broker_a
                                    && ui.selectable_label(b.broker_id == app.selected_broker_b, &b.name).clicked()
                                {
                                    app.set_broker_b(b.broker_id);
                                    ui.close_menu();
                                }
                            }
                        },
                    );
                }

                // Right Zone: Bottom X-Axis Mode
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let is_b_ticks = app.bottom_x_axis_mode == ChartXAxisMode::TickCount;
                    if ui.selectable_label(is_b_ticks, "Ticks").clicked() {
                        app.set_bottom_x_axis_mode(ChartXAxisMode::TickCount);
                    }
                    let is_b_time = app.bottom_x_axis_mode == ChartXAxisMode::ReceiveTime;
                    if ui.selectable_label(is_b_time, "Time").clicked() {
                        app.set_bottom_x_axis_mode(ChartXAxisMode::ReceiveTime);
                    }
                    ui.label(RichText::new("Bottom X:").color(Color32::from_gray(160)).small());
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

        match app.bottom_metric {
            BottomMetric::MidDiff => {
                draw_mid_diff_chart(
                    &bottom_painter,
                    bottom_rect,
                    series,
                    comparison,
                    app.bottom_x_axis_mode,
                    snapshot.built_mono_ns,
                    app.visible_seconds,
                    app.visible_ticks,
                    &app.theme,
                );
            }
            BottomMetric::BidAskDiff => {
                draw_bid_ask_diff_chart(
                    &bottom_painter,
                    bottom_rect,
                    series,
                    comparison,
                    app.bottom_x_axis_mode,
                    snapshot.built_mono_ns,
                    app.visible_seconds,
                    app.visible_ticks,
                    &app.theme,
                );
            }
            BottomMetric::SpreadDiff => {
                draw_spread_diff_chart(
                    &bottom_painter,
                    bottom_rect,
                    series,
                    comparison,
                    app.bottom_x_axis_mode,
                    snapshot.built_mono_ns,
                    app.visible_seconds,
                    app.visible_ticks,
                    &app.theme,
                );
            }
            BottomMetric::LeadLag => {
                draw_lead_lag_view(
                    &bottom_painter,
                    bottom_rect,
                    snapshot.active_pair_comparison.as_ref(),
                    &snapshot.broker_overviews,
                    &app.theme,
                );
            }
            BottomMetric::MidDispersion => {
                let vis = if app.hidden_brokers.is_empty() {
                    None
                } else {
                    Some(visible_broker_ids.as_slice())
                };
                draw_mid_dispersion_view_with_visibility(
                    &bottom_painter,
                    bottom_rect,
                    &snapshot.consensus,
                    &snapshot.broker_overviews,
                    vis,
                    &app.theme,
                );
            }
            BottomMetric::MoveBreadthView => {
                draw_move_breadth_view(
                    &bottom_painter,
                    bottom_rect,
                    &snapshot.current_breadth,
                    &snapshot.active_clusters,
                    &snapshot.broker_overviews,
                    &app.theme,
                );
            }
            BottomMetric::QuotePersistence => {
                let vis = if app.hidden_brokers.is_empty() {
                    None
                } else {
                    Some(visible_broker_ids.as_slice())
                };
                draw_quote_persistence_view_with_visibility(
                    &bottom_painter,
                    bottom_rect,
                    &snapshot.broker_overviews,
                    vis,
                    &app.theme,
                );
            }
            BottomMetric::QuotePath => {
                draw_realtime_quote_path_chart_with_visibility(
                    &bottom_painter,
                    bottom_rect,
                    &snapshot.realtime_quote_points,
                    &snapshot.broker_overviews,
                    Some(&visible_broker_ids),
                    app.selected_pair(),
                    app.bottom_x_axis_mode,
                    snapshot.built_mono_ns,
                    app.visible_seconds,
                    app.visible_ticks,
                    app.pip_size,
                    5.0,
                    0.4,
                    &mut app.bottom_chart_anchor,
                    &app.theme,
                );
            }
        }

        // Quick Settings Popover
        quick_settings::render_quick_settings(app, ctx, snapshot);

        // Debug Overlay
        if app.show_debug_overlay {
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
                    if app.diagnostics.is_some() {
                        ui.label("Detailed one-second summaries are saved under data/diagnostics.");
                    }
                    ui.label("Recent Diagnostics:");
                    for d in snapshot.diagnostics.iter().rev().take(10) {
                        ui.label(format!("[{}] {}: {}", d.severity as u8, d.code, d.message));
                    }
                });
        }
    });
}
