use super::DashboardApp;
use crate::core::models::UiSnapshot;
use crate::core::types::{ConnectionState, FreshnessState};
use crate::ui::style;
use eframe::egui;
use egui::{Color32, RichText};

pub fn render_broker_overview(
    app: &mut DashboardApp,
    ctx: &egui::Context,
    snapshot: &UiSnapshot,
) {
    if !app.show_broker_overview {
        return;
    }

    egui::TopBottomPanel::top("brokers_overview").show(ctx, |ui| {
        ui.horizontal(|ui| {
            ui.strong("Broker Overview");
            if !app.hidden_brokers.is_empty()
                && ui
                    .small_button("Show All")
                    .on_hover_text("Show all hidden brokers on charts")
                    .clicked()
            {
                app.show_all_brokers();
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.small_button("✕ Close [B]").clicked() {
                    app.show_broker_overview = false;
                }
            });
        });

        egui::ScrollArea::vertical()
            .max_height(140.0)
            .show(ui, |ui| {
                egui::Grid::new("broker_overview_grid")
                    .striped(true)
                    .spacing(egui::vec2(16.0, 8.0))
                    .min_row_height(26.0)
                    .show(ui, |ui| {
                        for header in [
                            "Vis",
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
                            ui.label(RichText::new(header).small().color(style::MUTED));
                        }
                        ui.end_row();
                        for b in &snapshot.broker_overviews {
                            let is_vis = app.is_broker_visible(b.broker_id);
                            let (status, status_color) = match b.health.connection {
                                ConnectionState::Disconnected => ("DISCONNECTED", style::ERROR),
                                ConnectionState::Connecting => ("CONNECTING", style::WARNING),
                                ConnectionState::Connected => match b.health.data_freshness {
                                    FreshnessState::Live => ("LIVE", style::LIVE),
                                    FreshnessState::Stale => ("STALE", style::WARNING),
                                    FreshnessState::Unknown => ("WARMING", style::MUTED),
                                },
                            };
                            let quote_is_live = b.health.connection
                                == ConnectionState::Connected
                                && b.health.data_freshness == FreshnessState::Live;
                            let quote_color = if !is_vis {
                                Color32::from_gray(90)
                            } else if quote_is_live {
                                Color32::WHITE
                            } else if b.health.connection == ConnectionState::Disconnected {
                                Color32::from_gray(90)
                            } else {
                                Color32::from_gray(145)
                            };

                            // 1. Visibility Checkbox
                            let mut vis_checked = is_vis;
                            let vis_btn = ui.checkbox(&mut vis_checked, "");
                            if vis_btn
                                .on_hover_text(if is_vis {
                                    "Visible on charts. Click to hide."
                                } else {
                                    "Hidden from charts. Click to show."
                                })
                                .clicked()
                            {
                                app.set_broker_visible(
                                    b.broker_id,
                                    vis_checked,
                                    &snapshot.broker_overviews,
                                );
                            }

                            // 2. Focus (A / B) Buttons
                            let is_a = b.broker_id == app.selected_broker_a;
                            let is_b = b.broker_id == app.selected_broker_b;
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 2.0;
                                let btn_a = ui.selectable_label(
                                    is_a,
                                    RichText::new("A").strong().color(if is_a {
                                        Color32::from_rgb(0, 220, 255)
                                    } else if is_vis {
                                        Color32::from_gray(100)
                                    } else {
                                        Color32::from_gray(60)
                                    }),
                                );
                                if btn_a.on_hover_text("Assign as Broker A").clicked() {
                                    app.set_broker_a(b.broker_id);
                                }
                                let btn_b = ui.selectable_label(
                                    is_b,
                                    RichText::new("B").strong().color(if is_b {
                                        Color32::from_rgb(255, 120, 200)
                                    } else if is_vis {
                                        Color32::from_gray(100)
                                    } else {
                                        Color32::from_gray(60)
                                    }),
                                );
                                if btn_b.on_hover_text("Assign as Broker B").clicked() {
                                    app.set_broker_b(b.broker_id);
                                }
                            });

                            let name_color = if !is_vis {
                                Color32::from_gray(120)
                            } else if quote_is_live {
                                Color32::WHITE
                            } else {
                                status_color
                            };
                            ui.label(RichText::new(&b.name).strong().color(name_color));
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
                                let age_ms = snapshot
                                    .built_mono_ns
                                    .0
                                    .saturating_sub(q.rx_mono_ns.0)
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
