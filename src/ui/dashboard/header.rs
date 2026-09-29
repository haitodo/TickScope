use super::DashboardApp;
use crate::core::models::UiSnapshot;
use crate::core::types::{ConnectionState, FreshnessState};
use crate::ui::chart::ChartXAxisMode;
use crate::ui::settings::{CandleFollowCriteria, CandlePriceScaleMode};
use eframe::egui;
use egui::{Color32, RichText};

pub fn render_top_header(
    app: &mut DashboardApp,
    ctx: &egui::Context,
    snapshot: &UiSnapshot,
    name_a: &str,
    name_b: &str,
) {
    egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
        ui.horizontal(|ui| {
            // 1. Left Zone: Context Controls (操作ゾーン)
            ui.checkbox(&mut app.show_candle_context, "Candle");
            if app.show_candle_context {
                let tf_text = match app.selected_timeframe_ms {
                    60000 => "M1",
                    10000 => "S10",
                    5000 => "S5",
                    1000 => "S1",
                    _ => "S10",
                };
                egui::ComboBox::from_id_salt("header_tf")
                    .selected_text(tf_text)
                    .width(44.0)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut app.selected_timeframe_ms, 10000, "S10 (10秒)");
                        ui.selectable_value(&mut app.selected_timeframe_ms, 5000, "S5 (5秒)");
                        ui.selectable_value(&mut app.selected_timeframe_ms, 1000, "S1 (1秒)");
                        ui.selectable_value(&mut app.selected_timeframe_ms, 60000, "M1 (1分)");
                    });
            }

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

            let overview_arrow = if app.show_broker_overview { "▲" } else { "▼" };
            let hidden_count = app.hidden_brokers.len();
            let overview_text = if hidden_count > 0 {
                format!(
                    "{} ● {}/{} Live ({} Hidden)",
                    overview_arrow, live_brokers, total_brokers, hidden_count
                )
            } else {
                format!("{} ● {}/{} Live", overview_arrow, live_brokers, total_brokers)
            };
            let overview_color = if live_brokers == total_brokers && total_brokers > 0 {
                Color32::from_rgb(0, 220, 140)
            } else if live_brokers > 0 {
                Color32::from_rgb(255, 200, 60)
            } else {
                Color32::from_rgb(255, 120, 120)
            };

            let mut tooltip = "Toggle Broker Overview table [Key: B]".to_string();
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
                        .map(|e| format!("{:+.2} ms", e))
                        .unwrap_or_else(|| "N/A".to_string());
                    tooltip.push_str(&format!(
                        "\n\n⚡ Lead/Lag [{} vs {}]:\nLeader: {}\nRaw Lead: {:.2} ms\nEMA Lead: {}\n(Key: [5] Lead/Lag view)",
                        name_a, name_b, leader_name, m.raw_delta_ms.abs(), ema_text
                    ));
                }
            }

            let toggle_btn = ui.selectable_label(
                app.show_broker_overview,
                RichText::new(overview_text).color(overview_color).strong(),
            );
            if toggle_btn
                .on_hover_text(tooltip)
                .clicked()
            {
                app.show_broker_overview = !app.show_broker_overview;
            }

            ui.separator();

            // MT5 Process Quick Action Buttons
            let target_ids: Vec<crate::core::types::BrokerId> = app.mt5_launch_targets.clone();
            let running_count = target_ids
                .iter()
                .filter(|&&id| app.terminal_manager.get_status(id).is_running())
                .count();
            let stopped_count = target_ids
                .iter()
                .filter(|&&id| !app.terminal_manager.get_status(id).is_running())
                .count();

            let launch_btn = ui.add_enabled(
                stopped_count > 0,
                egui::Button::new(
                    RichText::new(format!("▶ 起動 ({})", stopped_count))
                        .color(if stopped_count > 0 {
                            Color32::from_rgb(100, 220, 255)
                        } else {
                            Color32::from_gray(120)
                        })
                        .strong(),
                ),
            );
            if launch_btn
                .on_hover_text(format!(
                    "選択中の未起動MT5（{}台）を一括起動します（最小化: {}）",
                    stopped_count,
                    if app.mt5_minimized { "オン" } else { "オフ" }
                ))
                .clicked()
            {
                let stopped_targets: Vec<crate::core::types::BrokerId> = target_ids
                    .iter()
                    .copied()
                    .filter(|&id| !app.terminal_manager.get_status(id).is_running())
                    .collect();
                let minimized = app.mt5_minimized;
                app.terminal_manager.launch_multiple(&stopped_targets, minimized);
                app.terminal_manager.poll_status(&app.broker_configs, &app.discovered_terminals, true);
            }

            let stop_btn = ui.add_enabled(
                running_count > 0,
                egui::Button::new(
                    RichText::new(format!("⏹ 終了 ({})", running_count))
                        .color(if running_count > 0 {
                            Color32::from_rgb(255, 140, 140)
                        } else {
                            Color32::from_gray(120)
                        })
                        .strong(),
                ),
            );
            if stop_btn
                .on_hover_text(format!(
                    "選択中の起動中MT5（{}台）をクリーン終了（WM_CLOSE）します",
                    running_count
                ))
                .clicked()
            {
                app.show_mt5_stop_confirm_modal = true;
            }


            // 3. Right Zone: Utility & Settings (設定・ツール)
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {

                ui.checkbox(&mut app.show_debug_overlay, "Debug");

                let settings_btn = ui.selectable_label(
                    app.show_quick_settings,
                    RichText::new("⚙ Settings").strong(),
                );
                if settings_btn
                    .on_hover_text("Open Quick Settings flyout [Key: S]\nReset window size [Key: Ctrl+0]")
                    .clicked()
                {
                    app.show_quick_settings = !app.show_quick_settings;
                }

                if app.show_candle_context {
                    let scale_label = match app.candle_price_scale {
                        CandlePriceScaleMode::Auto => "Auto".to_string(),
                        CandlePriceScaleMode::Fixed(p) => {
                            let p_str = if (p.fract()).abs() < 1e-4 {
                                format!("{:.0}p", p)
                            } else {
                                format!("{:.1}p", p)
                            };
                            match app.candle_follow_criteria {
                                CandleFollowCriteria::Median => format!("{}/Med", p_str),
                                CandleFollowCriteria::MarginEdge => format!("{}/Edge", p_str),
                            }
                        }
                    };
                    let badge_text = format!(
                        "[{} | {:.0}px | {}]",
                        app.candle_price_mode.label().to_uppercase(),
                        app.candle_bar_width,
                        scale_label
                    );
                    let badge_btn = ui.add(
                        egui::Button::new(
                            RichText::new(badge_text).color(Color32::from_rgb(180, 220, 255)),
                        )
                        .wrap_mode(egui::TextWrapMode::Extend),
                    );
                    if badge_btn
                        .on_hover_text(
                            "Current Candle price, bar width & scale.\nClick to adjust settings [Key: S]",
                        )
                        .clicked()
                    {
                        app.show_quick_settings = true;
                    }
                } else {
                    let x_label = match app.top_x_axis_mode {
                        ChartXAxisMode::ReceiveTime => "Time",
                        ChartXAxisMode::TickCount => "Ticks",
                    };
                    let badge_text = format!("[Top X: {}]", x_label);
                    let badge_btn = ui.add(
                        egui::Button::new(
                            RichText::new(badge_text).color(Color32::from_gray(180)),
                        )
                        .wrap_mode(egui::TextWrapMode::Extend),
                    );
                    if badge_btn
                        .on_hover_text(
                            "Current Top X-axis mode.\nClick to toggle mode [Key: S]",
                        )
                        .clicked()
                    {
                        app.set_top_x_axis_mode(match app.top_x_axis_mode {
                            ChartXAxisMode::ReceiveTime => ChartXAxisMode::TickCount,
                            ChartXAxisMode::TickCount => ChartXAxisMode::ReceiveTime,
                        });
                    }
                }
            });
        });
    });
}
