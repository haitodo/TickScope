use super::DashboardApp;
use crate::core::models::UiSnapshot;
use crate::ui::chart::ChartXAxisMode;
use crate::ui::settings::{
    CandleFollowCriteria, CandlePriceMode, CandlePriceScaleMode, VALID_CANDLE_BAR_WIDTHS,
    VALID_CANDLE_FIXED_PIPS,
};
use eframe::egui;
use egui::{Color32, RichText};

pub fn render_quick_settings(
    app: &mut DashboardApp,
    ctx: &egui::Context,
    snapshot: &UiSnapshot,
) {
    if !app.show_quick_settings {
        return;
    }

    let mut is_open = true;
    egui::Window::new("⚙ Quick Settings")
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::RIGHT_TOP, egui::vec2(-8.0, 32.0))
        .default_width(320.0)
        .open(&mut is_open)
        .show(ctx, |ui| {
            ui.spacing_mut().item_spacing.y = 6.0;

            // 1. Candlestick Settings
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("Candlestick Display")
                        .strong()
                        .color(Color32::from_rgb(180, 220, 255)),
                );
            });

            ui.horizontal(|ui| {
                ui.label("Price:");
                if ui
                    .selectable_label(
                        app.candle_price_mode == CandlePriceMode::Bid,
                        "Bid",
                    )
                    .clicked()
                {
                    app.set_candle_price_mode(CandlePriceMode::Bid);
                }
                if ui
                    .selectable_label(
                        app.candle_price_mode == CandlePriceMode::Mid,
                        "Mid",
                    )
                    .clicked()
                {
                    app.set_candle_price_mode(CandlePriceMode::Mid);
                }
                if app.candle_price_mode == CandlePriceMode::Mid {
                    ui.label(
                        RichText::new("Reference price; not directly executable")
                            .small()
                            .color(Color32::from_gray(150)),
                    );
                }
            });

            ui.horizontal(|ui| {
                ui.label("Bar Width:");
                for &w in &VALID_CANDLE_BAR_WIDTHS {
                    let is_sel = (app.candle_bar_width - w).abs() < 1e-4;
                    let label = format!("{:.0}px", w);
                    if ui.selectable_label(is_sel, label).clicked() {
                        app.set_candle_bar_width(w);
                    }
                }
            });

            ui.horizontal(|ui| {
                ui.label("Price Scale:");
                let is_auto = app.candle_price_scale == CandlePriceScaleMode::Auto;
                if ui.selectable_label(is_auto, "Auto").clicked() {
                    app.set_candle_price_scale(CandlePriceScaleMode::Auto);
                }
                for &pips in &VALID_CANDLE_FIXED_PIPS {
                    let is_sel =
                        app.candle_price_scale == CandlePriceScaleMode::Fixed(pips);
                    let label = if (pips.fract()).abs() < 1e-4 {
                        format!("{:.0}p", pips)
                    } else {
                        format!("{:.1}p", pips)
                    };
                    if ui.selectable_label(is_sel, label).clicked() {
                        app.set_candle_price_scale(CandlePriceScaleMode::Fixed(pips));
                    }
                }
            });

            if let CandlePriceScaleMode::Fixed(_) = app.candle_price_scale {
                ui.horizontal(|ui| {
                    ui.label("Follow Mode:");
                    let is_med =
                        app.candle_follow_criteria == CandleFollowCriteria::Median;
                    if ui.selectable_label(is_med, "Median (中央値)").clicked() {
                        app.set_candle_follow_criteria(CandleFollowCriteria::Median);
                    }
                    let is_edge =
                        app.candle_follow_criteria == CandleFollowCriteria::MarginEdge;
                    if ui.selectable_label(is_edge, "Margin Edge (余白端)").clicked() {
                        app.set_candle_follow_criteria(
                            CandleFollowCriteria::MarginEdge,
                        );
                    }
                });
            }

            ui.separator();

            // 2. Visible Brokers
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("Visible Brokers (表示業者)")
                        .strong()
                        .color(Color32::from_rgb(180, 220, 255)),
                );
                if !app.hidden_brokers.is_empty()
                    && ui
                        .small_button("Show All")
                        .on_hover_text("Show all hidden brokers")
                        .clicked()
                {
                    app.show_all_brokers();
                }
            });

            ui.horizontal_wrapped(|ui| {
                for b in &snapshot.broker_overviews {
                    let mut vis = app.is_broker_visible(b.broker_id);
                    if ui.checkbox(&mut vis, &b.name).clicked() {
                        app.set_broker_visible(b.broker_id, vis, &snapshot.broker_overviews);
                    }
                }
            });

            ui.separator();

            // 3. Chart Axes & Timeline
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("Chart Axes & Timeline")
                        .strong()
                        .color(Color32::from_rgb(180, 220, 255)),
                );
            });

            ui.horizontal(|ui| {
                ui.label("Top X-Axis:");
                let is_time = app.top_x_axis_mode == ChartXAxisMode::ReceiveTime;
                if ui.selectable_label(is_time, "Receive Time").clicked() {
                    app.set_top_x_axis_mode(ChartXAxisMode::ReceiveTime);
                }
                let is_ticks = app.top_x_axis_mode == ChartXAxisMode::TickCount;
                if ui.selectable_label(is_ticks, "Tick Count").clicked() {
                    app.set_top_x_axis_mode(ChartXAxisMode::TickCount);
                }
            });

            ui.horizontal(|ui| {
                ui.label("Bottom X-Axis:");
                let is_time = app.bottom_x_axis_mode == ChartXAxisMode::ReceiveTime;
                if ui.selectable_label(is_time, "Receive Time").clicked() {
                    app.set_bottom_x_axis_mode(ChartXAxisMode::ReceiveTime);
                }
                let is_ticks = app.bottom_x_axis_mode == ChartXAxisMode::TickCount;
                if ui.selectable_label(is_ticks, "Tick Count").clicked() {
                    app.set_bottom_x_axis_mode(ChartXAxisMode::TickCount);
                }
            });

            ui.separator();

            // 3. Shortcuts Guide
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(
                        "Keys: [S] Settings, [B] Brokers, [P] Pair, [1-8] Metric, [Esc] Close",
                    )
                    .color(Color32::from_gray(140))
                    .small(),
                );
            });
        });
    app.show_quick_settings = is_open;
}
