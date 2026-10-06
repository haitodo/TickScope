use super::DashboardApp;
use crate::core::models::{BrokerOverview, UiSnapshot};
use crate::core::types::{BrokerId, ConnectionState, FreshnessState};
use crate::runtime::TerminalProcessStatus;
use crate::ui::style;
use eframe::egui;
use egui::{Color32, RichText};

const ROW_HEIGHT: f32 = 30.0;
const GAP: f32 = 8.0;
const HEADERS: [&str; 11] = [
    "順序",
    "表示",
    "比較",
    "Broker",
    "Bid",
    "Ask",
    "Spread",
    "Quote age",
    "Feed",
    "対象",
    "MT5",
];

// Only the broker name receives extra space. Live values never size columns.
fn column_widths(available: f32) -> [f32; 11] {
    let mut widths = [
        72.0, 30.0, 48.0, 120.0, 80.0, 80.0, 64.0, 76.0, 92.0, 32.0, 92.0,
    ];
    let minimum = widths.iter().sum::<f32>() + GAP * 10.0;
    widths[3] += (available - minimum).max(0.0);
    widths
}

const CELL_PADDING_X: f32 = 4.0;

fn cell<R>(ui: &mut egui::Ui, width: f32, contents: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, ROW_HEIGHT), egui::Sense::hover());
    let content_rect = egui::Rect::from_min_max(
        rect.min + egui::vec2(CELL_PADDING_X, 0.0),
        rect.max - egui::vec2(CELL_PADDING_X, 0.0),
    );
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(content_rect)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    child.set_clip_rect(ui.clip_rect().intersect(rect.expand2(egui::vec2(2.0, 0.0))));
    child.spacing_mut().item_spacing.x = 3.0;
    child.spacing_mut().button_padding = egui::vec2(4.0, 3.0);
    contents(&mut child)
}

fn value(ui: &mut egui::Ui, text: String, color: Color32) {
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        ui.add(egui::Label::new(RichText::new(&text).monospace().color(color)).truncate())
            .on_hover_text(text);
    });
}

const fn feed_status(b: &BrokerOverview) -> (&'static str, Color32, &'static str) {
    match b.health.connection {
        ConnectionState::Disconnected => (
            "Offline",
            style::ERROR,
            "未接続。MT5の対象チャートでTickCollectorを確認してください。",
        ),
        ConnectionState::Connecting => ("Connecting", style::WARNING, "接続中"),
        ConnectionState::Connected => match b.health.data_freshness {
            FreshnessState::Live => ("Live", style::LIVE, "接続中・最新の価格を受信"),
            FreshnessState::Stale => ("Stale", style::WARNING, "接続中・価格の更新が遅れています"),
            FreshnessState::Unknown => ("Warming", style::MUTED, "接続中・価格の受信待ち"),
        },
    }
}

fn quote_age(age_ms: u64) -> String {
    if age_ms < 1000 {
        format!("{age_ms} ms")
    } else if age_ms < 60_000 {
        format!("{:.1} s", age_ms as f64 / 1000.0)
    } else if age_ms < 3_600_000 {
        format!("{} min", age_ms / 60_000)
    } else if age_ms < 86_400_000 {
        format!("{} h", age_ms / 3_600_000)
    } else {
        format!("{} d", age_ms / 86_400_000)
    }
}

pub fn render_broker_overview(app: &mut DashboardApp, ctx: &egui::Context, snapshot: &UiSnapshot) {
    if !app.show_broker_overview {
        return;
    }

    egui::TopBottomPanel::top("brokers_overview").show(ctx, |ui| {
        ui.horizontal(|ui| {
            ui.strong("Brokers");
            ui.label(
                RichText::new("上から順に、ローソク足の左 → 右")
                    .small()
                    .color(style::MUTED),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .small_button("閉じる")
                    .on_hover_text("閉じる [B / Esc]")
                    .clicked()
                {
                    app.set_show_broker_overview(false);
                }
                ui.menu_button("操作", |ui| {
                    if ui.button("順序を初期設定に戻す").clicked() {
                        app.reset_broker_order(&snapshot.broker_overviews);
                        ui.close_menu();
                    }
                    if ui.button("すべての業者を表示").clicked() {
                        app.show_all_brokers();
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("すべてをMT5一括操作の対象にする").clicked() {
                        let ids: Vec<_> = snapshot
                            .broker_overviews
                            .iter()
                            .map(|b| b.broker_id)
                            .collect();
                        app.select_all_mt5_targets(&ids);
                        ui.close_menu();
                    }
                    if ui.button("MT5一括操作の対象をすべて解除").clicked() {
                        app.mt5_launch_targets.clear();
                        app.state_dirty = true;
                        ui.close_menu();
                    }
                });
            });
        });

        let ids = app.broker_ids_in_order(&snapshot.broker_overviews);
        let widths = column_widths(ui.available_width() - 16.0);
        let table_width = widths.iter().sum::<f32>() + GAP * 10.0;
        let mut movement = None;

        let row_count = ids.len().max(1);
        let content_height = ROW_HEIGHT + (row_count as f32) * (ROW_HEIGHT + 2.0) + 12.0;
        let viewport_h = ctx.screen_rect().height();
        let max_allowed = (viewport_h * 0.60).clamp(190.0, 550.0);
        let scroll_max_height = content_height.min(max_allowed).max(120.0);

        egui::ScrollArea::both()
            .id_salt("broker_overview_scroll")
            .max_height(scroll_max_height)
            .min_scrolled_height(scroll_max_height)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing = egui::vec2(GAP, 2.0);
                ui.horizontal(|ui| {
                    for (index, header) in HEADERS.iter().enumerate() {
                        cell(ui, widths[index], |ui| {
                            ui.label(RichText::new(*header).small().color(style::MUTED))
                                .on_hover_text(match index {
                                    0 => "ハンドルをドラッグ、または↑↓で移動",
                                    7 => "最後の価格受信からの経過時間",
                                    9 => "MT5一括起動・終了の対象",
                                    10 => "端末状態と起動・停止メニュー",
                                    _ => *header,
                                });
                        });
                    }
                });
                for (index, &id) in ids.iter().enumerate() {
                    let Some((color_index, b)) = snapshot
                        .broker_overviews
                        .iter()
                        .enumerate()
                        .find(|(_, b)| b.broker_id == id)
                    else {
                        continue;
                    };
                    let row_rect = egui::Rect::from_min_size(
                        ui.cursor().min,
                        egui::vec2(table_width, ROW_HEIGHT),
                    );
                    let is_hovered = ctx
                        .pointer_hover_pos()
                        .is_some_and(|p| row_rect.contains(p));
                    let bg_color = if is_hovered {
                        crate::ui::style::row_hover()
                    } else if index % 2 == 0 {
                        crate::ui::style::row_stripe()
                    } else {
                        Color32::TRANSPARENT
                    };
                    if bg_color != Color32::TRANSPARENT {
                        ui.painter().rect_filled(row_rect, 3.0, bg_color);
                    }
                    let visible = app.is_broker_visible(id);
                    let (status, status_color, status_help) = feed_status(b);
                    let live = b.health.connection == ConnectionState::Connected
                        && b.health.data_freshness == FreshnessState::Live;
                    let quote_color = if visible && live {
                        Color32::WHITE
                    } else {
                        style::MUTED
                    };
                    ui.push_id(id, |ui| {
                        ui.horizontal(|ui| {
                            cell(ui, widths[0], |ui| {
                                ui.dnd_drag_source(
                                    egui::Id::new(("broker_order_drag", id)),
                                    id,
                                    |ui| {
                                        // Draw the handle so it does not depend on symbol font coverage.
                                        let (rect, response) = ui.allocate_exact_size(
                                            egui::vec2(16.0, 22.0),
                                            egui::Sense::hover(),
                                        );
                                        for x in [-3.0, 3.0] {
                                            for y in [-5.0, 0.0, 5.0] {
                                                ui.painter().circle_filled(
                                                    rect.center() + egui::vec2(x, y),
                                                    1.2,
                                                    style::MUTED,
                                                );
                                            }
                                        }
                                        response
                                    },
                                )
                                .response
                                .on_hover_text("行へドラッグして並べ替え");
                                if ui
                                    .add_enabled(index > 0, egui::Button::new("↑").small())
                                    .on_hover_text("上へ移動")
                                    .clicked()
                                {
                                    movement = Some((id, ids[index - 1], false));
                                }
                                if ui
                                    .add_enabled(
                                        index + 1 < ids.len(),
                                        egui::Button::new("↓").small(),
                                    )
                                    .on_hover_text("下へ移動")
                                    .clicked()
                                {
                                    movement = Some((id, ids[index + 1], true));
                                }
                            });
                            cell(ui, widths[1], |ui| {
                                let mut checked = visible;
                                if ui
                                    .checkbox(&mut checked, "")
                                    .on_hover_text("チャートに表示")
                                    .changed()
                                {
                                    app.set_broker_visible(id, checked, &snapshot.broker_overviews);
                                }
                            });
                            cell(ui, widths[2], |ui| {
                                if ui
                                    .selectable_label(app.selected_broker_a == id, "A")
                                    .on_hover_text("比較対象A")
                                    .clicked()
                                {
                                    app.set_broker_a(id);
                                }
                                if ui
                                    .selectable_label(app.selected_broker_b == id, "B")
                                    .on_hover_text("比較対象B")
                                    .clicked()
                                {
                                    app.set_broker_b(id);
                                }
                            });
                            cell(ui, widths[3], |ui| {
                                let color = crate::ui::chart::broker_color_for_name(
                                    &app.theme,
                                    Some(&b.name),
                                    color_index,
                                );
                                let (rect, _) = ui.allocate_exact_size(
                                    egui::vec2(4.0, 14.0),
                                    egui::Sense::hover(),
                                );
                                ui.painter().rect_filled(
                                    rect,
                                    2.0,
                                    if visible { color } else { style::MUTED },
                                );
                                ui.add(
                                    egui::Label::new(RichText::new(&b.name).strong().color(
                                        if visible {
                                            Color32::WHITE
                                        } else {
                                            style::MUTED
                                        },
                                    ))
                                    .truncate(),
                                )
                                .on_hover_text(format!(
                                    "{} [{}]\nSymbol: {}\nUTC: {:+.1}h ({})\nTicks/s: {:.0}",
                                    b.name,
                                    id,
                                    b.symbol,
                                    f64::from(b.active_utc_offset_sec) / 3600.0,
                                    if b.is_auto_offset { "Auto" } else { "Fixed" },
                                    b.tick_rate_1s
                                ));
                            });
                            for (column, price) in [
                                (4, b.latest_quote.as_ref().map(|q| q.bid)),
                                (5, b.latest_quote.as_ref().map(|q| q.ask)),
                                (6, b.latest_quote.as_ref().map(|q| q.spread)),
                            ] {
                                cell(ui, widths[column], |ui| {
                                    value(
                                        ui,
                                        price.map_or_else(|| "—".into(), |v| format!("{v:.3}")),
                                        quote_color,
                                    );
                                });
                            }
                            cell(ui, widths[7], |ui| {
                                let age = b.latest_quote.as_ref().map(|q| {
                                    snapshot.built_mono_ns.0.saturating_sub(q.rx_mono_ns.0)
                                        / 1_000_000
                                });
                                value(
                                    ui,
                                    age.map_or_else(|| "—".into(), quote_age),
                                    if live { quote_color } else { status_color },
                                );
                            });
                            cell(ui, widths[8], |ui| {
                                ui.label(
                                    RichText::new(format!("● {status}"))
                                        .small()
                                        .color(status_color),
                                )
                                .on_hover_text(status_help);
                            });
                            cell(ui, widths[9], |ui| {
                                let mut target = app.is_mt5_target(id);
                                if ui
                                    .checkbox(&mut target, "")
                                    .on_hover_text("MT5一括起動・終了の対象")
                                    .changed()
                                {
                                    app.set_mt5_target(id, target);
                                }
                            });
                            cell(ui, widths[10], |ui| {
                                let process = app.terminal_manager.get_status(id);
                                let (label, color) = match process {
                                    TerminalProcessStatus::Running { .. } => {
                                        ("起動中", style::LIVE)
                                    }
                                    TerminalProcessStatus::Stopped => ("停止中", style::MUTED),
                                    TerminalProcessStatus::NotFound => ("未設定", style::WARNING),
                                };
                                ui.menu_button(
                                    RichText::new(format!("{label} ▾")).color(color),
                                    |ui| {
                                        let normal = app.mt5_non_minimized_broker == Some(id);
                                        ui.strong(&b.name);
                                        ui.label(if normal {
                                            "起動モード: 通常表示"
                                        } else if app.mt5_minimized {
                                            "起動モード: 最小化"
                                        } else {
                                            "起動モード: 通常表示"
                                        });
                                        match process {
                                            TerminalProcessStatus::Running { pid } => {
                                                ui.label(format!("PID: {pid}"));
                                                if ui.button("MT5を停止").clicked() {
                                                    app.terminal_manager.stop(
                                                        id,
                                                        std::time::Duration::from_secs(5),
                                                    );
                                                    ui.close_menu();
                                                }
                                            }
                                            TerminalProcessStatus::Stopped => {
                                                if ui.button("MT5を起動").clicked() {
                                                    let _ = app
                                                        .terminal_manager
                                                        .launch(id, !normal && app.mt5_minimized);
                                                    app.terminal_manager.poll_status(
                                                        &app.broker_configs,
                                                        &app.discovered_terminals,
                                                        true,
                                                    );
                                                    ui.close_menu();
                                                }
                                            }
                                            TerminalProcessStatus::NotFound => {
                                                ui.label("MT5実行ファイルが見つかりません。");
                                                ui.label("設定のterminal_pathを確認してください。");
                                            }
                                        }
                                    },
                                );
                            });
                        });
                    });

                    // Accept drops across the row, including its price and status cells.
                    if let Some(payload) = egui::DragAndDrop::payload::<BrokerId>(ctx) {
                        if let Some(pointer) = ctx.pointer_interact_pos() {
                            if row_rect.intersect(ui.clip_rect()).contains(pointer)
                                && *payload != id
                            {
                                let after = pointer.y > row_rect.center().y;
                                let y = if after {
                                    row_rect.bottom()
                                } else {
                                    row_rect.top()
                                };
                                ui.painter().line_segment(
                                    [
                                        egui::pos2(row_rect.left(), y),
                                        egui::pos2(row_rect.right(), y),
                                    ],
                                    egui::Stroke::new(2.0_f32, style::LIVE),
                                );
                                if ctx.input(|i| i.pointer.any_released()) {
                                    egui::DragAndDrop::take_payload::<BrokerId>(ctx);
                                    movement = Some((*payload, id, after));
                                }
                            }
                        }
                    }
                }
            });
        if let Some((source, target, after)) = movement {
            app.move_broker_order(source, target, after);
            ctx.request_repaint();
        }
    });
}

#[cfg(test)]
mod layout_tests {
    use super::*;
    use crate::core::types::{MonoNs, Quote, TickId};
    use crate::ui::test_support::headless_app;

    #[test]
    fn default_window_columns_stay_inside_viewport_across_feed_changes() {
        let mut app = headless_app().with_show_broker_overview(true);
        let ctx = egui::Context::default();
        crate::ui::fonts::setup_fonts(&ctx);
        style::configure(&ctx);
        let mut snapshot = UiSnapshot::default();
        let mut broker = BrokerOverview {
            broker_id: 1,
            name: "A very long broker name that must not expand the table".into(),
            symbol: "USDJPY.long-symbol-suffix".into(),
            ..Default::default()
        };
        broker.health.data_freshness = FreshnessState::Live;
        broker.latest_quote = Some(Quote {
            tick_id: TickId {
                broker_id: 1,
                session_id: 1,
                sequence: 1,
            },
            bid: 155.1,
            ask: 155.2,
            mid: 155.15,
            spread: 0.1,
            rx_mono_ns: MonoNs::ZERO,
            utc_ms: None,
            is_warmup: false,
            is_valid: true,
        });
        snapshot.broker_overviews.push(broker);
        // egui measures a newly opened panel before painting its full contents.
        for _ in 0..2 {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1100.0, 750.0),
                )),
                ..Default::default()
            };
            let _ = ctx.run(input, |ctx| {
                render_broker_overview(&mut app, ctx, &snapshot);
            });
        }
        let mut expected_positions = None;
        for (connection, age_ms, expected_age) in [
            (ConnectionState::Connected, 25, "25 ms"),
            (ConnectionState::Disconnected, 2_300, "2.3 s"),
            (ConnectionState::Connecting, 180_000, "3 min"),
            (ConnectionState::Connected, 3_600_000, "1 h"),
        ] {
            snapshot.broker_overviews[0].health.connection = connection;
            snapshot.built_mono_ns = MonoNs(age_ms * 1_000_000);
            // Include the first frame after each change, where sizing regressions occur.
            for _ in 0..2 {
                let input = egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1100.0, 750.0),
                    )),
                    ..Default::default()
                };
                let output = ctx.run(input, |ctx| {
                    render_broker_overview(&mut app, ctx, &snapshot);
                });
                let positions: Vec<_> = HEADERS
                    .iter()
                    .map(|header| {
                        let text = output
                            .shapes
                            .iter()
                            .find_map(|shape| {
                                if let egui::Shape::Text(text) = &shape.shape {
                                    if text.galley.job.text == *header {
                                        return Some(text);
                                    }
                                }
                                None
                            })
                            .unwrap_or_else(|| panic!("Missing header: {header}"));
                        assert!(
                            text.pos.x + text.galley.size().x <= 1100.0,
                            "{header} overflowed"
                        );
                        text.pos.x
                    })
                    .collect();
                if let Some(expected) = &expected_positions {
                    assert_eq!(&positions, expected, "Feed changes must not move columns");
                } else {
                    expected_positions = Some(positions);
                }
                assert!(ctx.used_rect().right() <= 1100.0);
                let feed_text = format!("● {}", feed_status(&snapshot.broker_overviews[0]).0);
                let feed = output
                    .shapes
                    .iter()
                    .find_map(|shape| {
                        if let egui::Shape::Text(text) = &shape.shape {
                            if text.galley.job.text == feed_text {
                                return Some((text, shape.clip_rect));
                            }
                        }
                        None
                    })
                    .expect("Feed status should be visible");
                assert_eq!(feed.0.galley.rows.len(), 1);
                assert!(feed.0.pos.x + feed.0.galley.size().x <= feed.1.right());
                let age = output
                    .shapes
                    .iter()
                    .find_map(|shape| {
                        if let egui::Shape::Text(text) = &shape.shape {
                            if text.galley.job.text == expected_age {
                                return Some((text, shape.clip_rect));
                            }
                        }
                        None
                    })
                    .expect("Quote age should be visible");
                assert_eq!(age.0.galley.rows.len(), 1);
                assert!(
                    age.0.pos.x <= age.1.right()
                        && age.0.pos.x - age.0.galley.size().x >= age.1.left(),
                    "Quote age {expected_age:?} overflowed: pos={:?}, size={:?}, clip={:?}",
                    age.0.pos,
                    age.0.galley.size(),
                    age.1
                );
                for shape in &output.shapes {
                    if let egui::Shape::Text(text) = &shape.shape {
                        if ["A", "B", "↑", "↓"].contains(&text.galley.job.text.as_str()) {
                            assert_eq!(text.galley.rows.len(), 1);
                            assert!(
                                text.pos.x + text.galley.size().x <= shape.clip_rect.right(),
                                "Control {} was clipped",
                                text.galley.job.text
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn controls_are_not_clipped_on_left_or_right() {
        let mut app = headless_app().with_show_broker_overview(true);
        let ctx = egui::Context::default();
        crate::ui::fonts::setup_fonts(&ctx);
        style::configure(&ctx);
        let mut snapshot = UiSnapshot::default();
        let mut broker = BrokerOverview {
            broker_id: 1,
            name: "Test Broker".into(),
            symbol: "USDJPY".into(),
            ..Default::default()
        };
        broker.health.connection = ConnectionState::Connected;
        broker.health.data_freshness = FreshnessState::Live;
        snapshot.broker_overviews.push(broker);

        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1100.0, 750.0),
            )),
            ..Default::default()
        };
        let _ = ctx.run(input.clone(), |ctx| {
            render_broker_overview(&mut app, ctx, &snapshot);
        });
        let output = ctx.run(input, |ctx| {
            render_broker_overview(&mut app, ctx, &snapshot);
        });

        for shape in &output.shapes {
            match &shape.shape {
                egui::Shape::Rect(rect_shape) => {
                    // Controls with visible strokes must stay inside their clip rect on both sides
                    if rect_shape.stroke.width > 0.0 && shape.clip_rect.width() < 1000.0 {
                        assert!(
                            rect_shape.rect.left() >= shape.clip_rect.left(),
                            "Control rect left {:?} clipped by {:?}",
                            rect_shape.rect,
                            shape.clip_rect
                        );
                        assert!(
                            rect_shape.rect.right() <= shape.clip_rect.right(),
                            "Control rect right {:?} clipped by {:?}",
                            rect_shape.rect,
                            shape.clip_rect
                        );
                    }
                }
                egui::Shape::Text(text_shape)
                    if ["A", "B", "↑", "↓"].contains(&text_shape.galley.job.text.as_str()) =>
                {
                    assert!(
                        text_shape.pos.x >= shape.clip_rect.left(),
                        "Text left clipped: {} at {:?}",
                        text_shape.galley.job.text,
                        text_shape.pos
                    );
                    assert!(
                        text_shape.pos.x + text_shape.galley.size().x <= shape.clip_rect.right(),
                        "Text right clipped: {} at {:?}",
                        text_shape.galley.job.text,
                        text_shape.pos
                    );
                }
                _ => {}
            }
        }
    }

    #[test]
    fn auto_fit_height_scales_with_broker_count() {
        let measure_height = |count: usize| -> f32 {
            let mut app = headless_app().with_show_broker_overview(true);
            let ctx = egui::Context::default();
            crate::ui::fonts::setup_fonts(&ctx);
            style::configure(&ctx);

            let mut snapshot = UiSnapshot::default();
            for id in 1..=count {
                snapshot.broker_overviews.push(BrokerOverview {
                    broker_id: id as u32,
                    name: format!("Broker {id}"),
                    ..Default::default()
                });
            }
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1100.0, 750.0),
                )),
                ..Default::default()
            };
            // Run 3 frames so egui can measure and resize TopBottomPanel
            for _ in 0..3 {
                let _ = ctx.run(input.clone(), |ctx| {
                    render_broker_overview(&mut app, ctx, &snapshot);
                });
            }
            let output = ctx.run(input, |ctx| {
                render_broker_overview(&mut app, ctx, &snapshot);
            });
            // Find the bottom panel separator line
            output
                .shapes
                .iter()
                .filter_map(|s| {
                    if let egui::Shape::LineSegment { points, .. } = &s.shape {
                        Some(points[0].y)
                    } else {
                        None
                    }
                })
                .max_by(|a, b| a.partial_cmp(b).unwrap())
                .unwrap_or(0.0)
        };

        let h2 = measure_height(2);
        let h8 = measure_height(8);
        assert!(
            h8 > h2 + 100.0,
            "Overview height must scale with broker count: h2={h2}, h8={h8}"
        );
    }
}
