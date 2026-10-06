use super::DashboardApp;
use crate::core::models::UiSnapshot;
use crate::core::types::{ConnectionState, FreshnessState};
use crate::ui::chart::ChartXAxisMode;
use crate::ui::settings::{
    CandleFollowCriteria, CandlePriceMode, CandlePriceScaleMode, VALID_CANDLE_FIXED_PIPS,
};
use crate::ui::shared::{broker_name, format_pips};
use eframe::egui;
use egui::RichText;
use std::fmt::Write as _;

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
                    "{overview_arrow} ● {live_brokers}/{total_brokers} Live ({hidden_count} Hidden)"
                )
            } else {
                format!("{overview_arrow} ● {live_brokers}/{total_brokers} Live")
            };
            let overview_color = if live_brokers == total_brokers && total_brokers > 0 {
                crate::ui::style::STATUS_OK
            } else if live_brokers > 0 {
                crate::ui::style::STATUS_WARN
            } else {
                crate::ui::style::STATUS_ALERT
            };

            let mut tooltip = "Toggle Broker Overview table [Key: B]".to_string();
            if let Some(comp) = &snapshot.active_pair_comparison {
                if let Some(m) = &comp.latest_match {
                    let leader_name = broker_name(&snapshot.broker_overviews, m.leader, "Leader");
                    let ema_text = comp
                        .ema_lead_lag_ms.map_or_else(|| "N/A".to_string(), |e| format!("{e:+.2} ms"));
                    let _ = write!(tooltip,
                        "\n\n⚡ Lead/Lag [{} vs {}]:\nLeader: {}\nRaw Lead: {:.2} ms\nEMA Lead: {}\n(Key: [5] Lead/Lag view)",
                        name_a, name_b, leader_name, m.raw_delta_ms.abs(), ema_text
                    );
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
                    RichText::new(format!("▶ 起動 ({stopped_count})"))
                        .color(if stopped_count > 0 {
                            crate::ui::style::INFO
                        } else {
                            crate::ui::style::TEXT_DISABLED
                        })
                        .strong(),
                ),
            );
            let normal_broker_name = app.mt5_non_minimized_broker.and_then(|id| {
                app.broker_configs.iter().find(|b| b.id == id).map(|b| b.name.as_str())
            });
            let hover_text = match normal_broker_name {
                Some(name) => format!(
                    "選択中の未起動MT5（{}台）を一括起動します（「{}」は通常表示、他は最小化: {}）",
                    stopped_count,
                    name,
                    if app.mt5_minimized { "オン" } else { "オフ" }
                ),
                None => format!(
                    "選択中の未起動MT5（{}台）を一括起動します（最小化: {}）",
                    stopped_count,
                    if app.mt5_minimized { "オン" } else { "オフ" }
                ),
            };

            if launch_btn
                .on_hover_text(hover_text)
                .clicked()
            {
                let stopped_targets: Vec<crate::core::types::BrokerId> = target_ids
                    .iter()
                    .copied()
                    .filter(|&id| !app.terminal_manager.get_status(id).is_running())
                    .collect();
                let minimized = app.mt5_minimized;
                let normal_id = app.mt5_non_minimized_broker;
                app.terminal_manager.launch_multiple_with_normal(&stopped_targets, normal_id, minimized);
                app.terminal_manager.poll_status(&app.broker_configs, &app.discovered_terminals, true);
            }

            let stop_btn = ui.add_enabled(
                running_count > 0,
                egui::Button::new(
                    RichText::new(format!("⏹ 終了 ({running_count})"))
                        .color(if running_count > 0 {
                            crate::ui::style::STATUS_ALERT_SOFT
                        } else {
                            crate::ui::style::TEXT_DISABLED
                        })
                        .strong(),
                ),
            );
            if stop_btn
                .on_hover_text(format!(
                    "選択中の起動中MT5（{running_count}台）をクリーン終了（WM_CLOSE）します"
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

                let pin_text = if app.always_on_top {
                    RichText::new("📌 Pin").color(crate::ui::style::HIGHLIGHT).strong()
                } else {
                    RichText::new("📌 Pin").color(crate::ui::style::TEXT_FAINT)
                };
                let pin_btn = ui.selectable_label(app.always_on_top, pin_text);
                let pin_hover = if app.always_on_top {
                    "最前面固定: オン\n他のウィンドウの前面に常に表示します [Key: T]"
                } else {
                    "最前面固定: オフ\nクリックして常に手前に表示 [Key: T]"
                };
                if pin_btn.on_hover_text(pin_hover).clicked() {
                    app.toggle_always_on_top(ctx);
                }

                if app.show_candle_context {
                    ui.horizontal(|ui| {
                        let bid_btn = ui.selectable_label(
                            app.candle_price_mode == CandlePriceMode::Bid,
                            "Bid",
                        );
                        if bid_btn
                            .on_hover_text("Use Bid prices for candlesticks")
                            .clicked()
                        {
                            app.set_candle_price_mode(CandlePriceMode::Bid);
                        }

                        let mid_btn = ui.selectable_label(
                            app.candle_price_mode == CandlePriceMode::Mid,
                            "Mid",
                        );
                        if mid_btn
                            .on_hover_text("Use Mid prices for candlesticks (reference price)")
                            .clicked()
                        {
                            app.set_candle_price_mode(CandlePriceMode::Mid);
                        }
                    });

                    let scale_label = match app.candle_price_scale {
                        CandlePriceScaleMode::Auto => "Auto".to_string(),
                        CandlePriceScaleMode::Fixed(p) => {
                            let p_str = format_pips(p, "p");
                            match app.candle_follow_criteria {
                                CandleFollowCriteria::Median => format!("{p_str}/Med"),
                                CandleFollowCriteria::MarginEdge => format!("{p_str}/Edge"),
                            }
                        }
                    };
                    egui::ComboBox::from_id_salt("header_price_scale")
                        .selected_text(
                            RichText::new(format!(
                                "[{:.0}px | {}]",
                                app.candle_bar_width, scale_label
                            ))
                            .color(crate::ui::style::SECTION_TITLE),
                        )
                        .show_ui(ui, |ui| {
                            if ui
                                .selectable_label(
                                    app.candle_price_scale == CandlePriceScaleMode::Auto,
                                    "Auto",
                                )
                                .clicked()
                            {
                                app.set_candle_price_scale(CandlePriceScaleMode::Auto);
                                ui.close_menu();
                            }

                            for &pips in &VALID_CANDLE_FIXED_PIPS {
                                let mode = CandlePriceScaleMode::Fixed(pips);
                                let label = format_pips(pips, "p");
                                if ui
                                    .selectable_label(app.candle_price_scale == mode, label)
                                    .clicked()
                                {
                                    app.set_candle_price_scale(mode);
                                    ui.close_menu();
                                }
                            }
                        });
                } else {
                    let x_label = match app.top_x_axis_mode {
                        ChartXAxisMode::ReceiveTime => "Time",
                        ChartXAxisMode::TickCount => "Ticks",
                    };
                    let badge_text = format!("[Top X: {x_label}]");
                    let badge_btn = ui.add(
                        egui::Button::new(
                            RichText::new(badge_text).color(crate::ui::style::TEXT_SUBDUED),
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
