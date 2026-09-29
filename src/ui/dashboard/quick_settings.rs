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

    let screen_rect = ctx.screen_rect();
    let top_offset = 32.0;
    let bottom_margin = 8.0;
    let side_margin = 8.0;

    // Dynamically constrain dialog within visible viewport so it never cuts off or overflows.
    // egui::Window decoration (title bar ~28px, frame margins ~24px, inner margins/spacing)
    // adds ~84px around the inner content.
    let frame_decorations = 84.0;
    let max_total_window_height = (screen_rect.height() - top_offset - bottom_margin).max(160.0);
    let max_window_content_height = (max_total_window_height - frame_decorations).max(80.0);
    let max_dialog_width = (screen_rect.width() - side_margin * 2.0).max(260.0);
    let default_width = 330.0_f32.min(max_dialog_width);

    let mut is_open = true;
    egui::Window::new("⚙ Quick Settings")
        .collapsible(false)
        .resizable(true)
        .anchor(egui::Align2::RIGHT_TOP, egui::vec2(-side_margin, top_offset))
        .default_width(default_width)
        .min_width(260.0_f32.min(max_dialog_width))
        .max_width(max_dialog_width)
        .max_height(max_window_content_height)
        .constrain(true)
        .open(&mut is_open)
        .show(ctx, |ui| {
            // Quick Reset Banner: if viewport is small or window is maximized, provide instant 1-click reset at top
            let is_compact_screen = screen_rect.height() < 680.0 || screen_rect.width() < 950.0;
            let mut banner_height = 0.0;
            if is_compact_screen || app.window_geometry.maximized {
                let banner_start = ui.cursor().top();
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(format!(
                            "画面: {:.0}×{:.0}{}",
                            app.window_geometry.inner_size[0],
                            app.window_geometry.inner_size[1],
                            if app.window_geometry.maximized { " (最大化)" } else { "" }
                        ))
                        .small()
                        .color(Color32::from_gray(160)),
                    );
                    let reset_btn = ui.small_button("⟲ リセット (1100×750)");
                    if reset_btn
                        .on_hover_text("ウィンドウサイズを規定値（1100×750）に戻します [Key: Ctrl+0]")
                        .clicked()
                    {
                        app.reset_window_size(ctx);
                    }
                });
                ui.separator();
                banner_height = (ui.cursor().top() - banner_start).max(0.0);
            }

            // Scrollable body so no settings get cut off on small windows or high DPI
            let scroll_max_h = (max_window_content_height - banner_height).max(60.0);

            egui::ScrollArea::vertical()
                .auto_shrink([false, true])
                .max_height(scroll_max_h)
                .show(ui, |ui| {
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
                        ui.label("Timeframe:");
                for &(ms, label) in &[
                    (10000, "S10 (10秒)"),
                    (5000, "S5 (5秒)"),
                    (1000, "S1 (1秒)"),
                    (60000, "M1 (1分)"),
                ] {
                    if ui.selectable_label(app.selected_timeframe_ms == ms, label).clicked() {
                        app.selected_timeframe_ms = ms;
                    }
                }
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

            // 4. MT5 Process Management
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("MT5 Process Lifecycle")
                        .strong()
                        .color(Color32::from_rgb(180, 220, 255)),
                );
            });

            ui.checkbox(&mut app.mt5_auto_launch, "Auto-launch target MT5s on start");
            ui.checkbox(&mut app.mt5_auto_close, "Auto-close running MT5s on exit");
            ui.checkbox(&mut app.mt5_minimized, "Launch Minimized (最小化起動)");

            ui.horizontal(|ui| {
                ui.label("通常表示する業者 (Normal Window):");
                let current_selected = app.mt5_non_minimized_broker;
                let selected_text = match current_selected {
                    Some(id) => app
                        .broker_configs
                        .iter()
                        .find(|b| b.id == id)
                        .map(|b| format!("{} (ID: {})", b.name, b.id))
                        .unwrap_or_else(|| format!("ID: {}", id)),
                    None => "なし (すべて最小化)".to_string(),
                };
                egui::ComboBox::from_id_salt("mt5_normal_broker_combo")
                    .selected_text(selected_text)
                    .show_ui(ui, |ui| {
                        if ui
                            .selectable_value(
                                &mut app.mt5_non_minimized_broker,
                                None,
                                "なし (すべて最小化)",
                            )
                            .clicked()
                        {
                            app.state_dirty = true;
                        }
                        for b in &app.broker_configs {
                            let label = format!("{} (ID: {})", b.name, b.id);
                            if ui
                                .selectable_value(
                                    &mut app.mt5_non_minimized_broker,
                                    Some(b.id),
                                    label,
                                )
                                .clicked()
                            {
                                app.state_dirty = true;
                            }
                        }
                    });
            });

            ui.collapsing("Resolved MT5 Executable Paths", |ui| {
                for b in &app.broker_configs {
                    let path_str = app
                        .terminal_manager
                        .get_exe_path(b.id)
                        .map(|p| p.display().to_string())
                        .unwrap_or_else(|| "Not found".to_string());
                    ui.label(RichText::new(&b.name).strong());
                    ui.label(RichText::new(path_str).small().monospace().color(Color32::from_gray(160)));
                }
            });

            ui.separator();

            // 5. Window & Display (ウィンドウ・画面サイズ)
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("Window & Display (画面サイズ)")
                        .strong()
                        .color(Color32::from_rgb(180, 220, 255)),
                );
            });

            ui.horizontal(|ui| {
                let size_str = format!(
                    "{:.0} × {:.0}{}",
                    app.window_geometry.inner_size[0],
                    app.window_geometry.inner_size[1],
                    if app.window_geometry.maximized { " (最大化)" } else { "" }
                );
                ui.label(format!("Current: {}", size_str));
            });

            ui.horizontal(|ui| {
                let reset_btn = ui.button(
                    RichText::new("⟲ 画面サイズをリセット (1100×750)")
                        .color(Color32::from_rgb(220, 230, 255))
                        .strong(),
                );
                if reset_btn
                    .on_hover_text("ウィンドウサイズを規定値（1100×750）に戻し、最大化と拡大率をリセットします [Key: Ctrl+0]")
                    .clicked()
                {
                    app.reset_window_size(ctx);
                }
            });

            ui.separator();

            // 6. Shortcuts Guide
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(
                        "Keys: [S] Settings, [Ctrl+0] Reset Size, [B] Brokers, [P] Pair, [1-8] Metric, [Esc] Close",
                    )
                    .color(Color32::from_gray(140))
                    .small(),
                );
            });
        });
    });
    app.show_quick_settings = is_open;
}
