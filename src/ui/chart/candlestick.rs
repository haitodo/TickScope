use super::common::{
    chart_plot_rect, price_decimals_for_pip, CHART_HEADER_HEIGHT, PRICE_AXIS_WIDTH,
};
use super::scale::{
    draw_clip_marker, extract_valid_quote_candidates_for_mode, resolve_candle_follow_scale,
    FollowStatusInfo, MarginEdgeLatchSide,
};
use super::theme::{broker_color_for_name, dim_candle_color, ChartTheme};
use crate::core::models::{BrokerOverview, CandleView, Ohlc, PriceMode, ReplayTrade, SlotState};
use crate::core::types::{BrokerId, MonoNs};
use crate::ui::settings::{CandleFollowCriteria, CandlePriceScaleMode};
use crate::ui::shared::broker_name;
use egui::{Color32, Pos2, Rect, Stroke};

/// Multiple broker Candlestick Chart (RFC §84, §85, §86, §87)
/// Distinguishes brokers by identity colors and distinguish up/down bars
/// by brightness so colors do not imply buy or sell semantics.
pub fn draw_candlestick_chart_multi(
    painter: &egui::Painter,
    rect: Rect,
    candle_view: Option<&CandleView>,
    broker_overviews: &[BrokerOverview],
    bar_width: f32,
    scale_mode: CandlePriceScaleMode,
    follow_criteria: CandleFollowCriteria,
    pip_size: f64,
    chart_anchor: &mut Option<f64>,
    margin_edge_latch: &mut Option<MarginEdgeLatchSide>,
    fallback_price: Option<f64>,
    chart_max_quote_age_ms: u64,
    now_mono: MonoNs,
    theme: &ChartTheme,
) {
    draw_candlestick_chart_multi_with_mode(
        painter,
        rect,
        candle_view,
        broker_overviews,
        bar_width,
        scale_mode,
        follow_criteria,
        pip_size,
        chart_anchor,
        margin_edge_latch,
        fallback_price,
        chart_max_quote_age_ms,
        now_mono,
        PriceMode::Bid,
        theme,
    );
}

pub fn draw_candlestick_chart_multi_with_mode(
    painter: &egui::Painter,
    rect: Rect,
    candle_view: Option<&CandleView>,
    broker_overviews: &[BrokerOverview],
    bar_width: f32,
    scale_mode: CandlePriceScaleMode,
    follow_criteria: CandleFollowCriteria,
    pip_size: f64,
    chart_anchor: &mut Option<f64>,
    margin_edge_latch: &mut Option<MarginEdgeLatchSide>,
    fallback_price: Option<f64>,
    chart_max_quote_age_ms: u64,
    now_mono: MonoNs,
    price_mode: PriceMode,
    theme: &ChartTheme,
) {
    let mut broker_ids: Vec<BrokerId> = broker_overviews.iter().map(|b| b.broker_id).collect();
    if broker_ids.is_empty() {
        if let Some(view) = candle_view {
            broker_ids = view.slots_by_broker.keys().copied().collect();
            broker_ids.sort_unstable();
        }
    }
    draw_candlestick_chart_for_brokers(
        painter,
        rect,
        candle_view,
        &broker_ids,
        broker_overviews,
        bar_width,
        scale_mode,
        follow_criteria,
        pip_size,
        chart_anchor,
        margin_edge_latch,
        fallback_price,
        chart_max_quote_age_ms,
        now_mono,
        price_mode,
        theme,
    );
}

pub fn draw_candlestick_chart_for_brokers(
    painter: &egui::Painter,
    rect: Rect,
    candle_view: Option<&CandleView>,
    broker_ids: &[BrokerId],
    broker_overviews: &[BrokerOverview],
    bar_width: f32,
    scale_mode: CandlePriceScaleMode,
    follow_criteria: CandleFollowCriteria,
    pip_size: f64,
    chart_anchor: &mut Option<f64>,
    margin_edge_latch: &mut Option<MarginEdgeLatchSide>,
    fallback_price: Option<f64>,
    chart_max_quote_age_ms: u64,
    now_mono: MonoNs,
    price_mode: PriceMode,
    theme: &ChartTheme,
) {
    draw_candlestick_chart_for_brokers_with_trades(
        painter,
        rect,
        candle_view,
        broker_ids,
        broker_overviews,
        bar_width,
        scale_mode,
        follow_criteria,
        pip_size,
        chart_anchor,
        margin_edge_latch,
        fallback_price,
        chart_max_quote_age_ms,
        now_mono,
        price_mode,
        theme,
        None,
    );
}

pub fn draw_candlestick_chart_for_brokers_with_trades(
    painter: &egui::Painter,
    rect: Rect,
    candle_view: Option<&CandleView>,
    broker_ids: &[BrokerId],
    broker_overviews: &[BrokerOverview],
    bar_width: f32,
    scale_mode: CandlePriceScaleMode,
    follow_criteria: CandleFollowCriteria,
    pip_size: f64,
    chart_anchor: &mut Option<f64>,
    margin_edge_latch: &mut Option<MarginEdgeLatchSide>,
    fallback_price: Option<f64>,
    chart_max_quote_age_ms: u64,
    now_mono: MonoNs,
    price_mode: PriceMode,
    theme: &ChartTheme,
    trade_items: Option<(&[ReplayTrade], &[ReplayTrade])>,
) {
    draw_candlestick_chart_for_brokers_with_trades_impl(
        None,
        painter,
        rect,
        candle_view,
        broker_ids,
        broker_overviews,
        bar_width,
        scale_mode,
        follow_criteria,
        pip_size,
        chart_anchor,
        margin_edge_latch,
        fallback_price,
        chart_max_quote_age_ms,
        now_mono,
        price_mode,
        theme,
        trade_items,
        None,
        false,
    );
}

pub fn draw_candlestick_chart_for_brokers_with_trades_interactive(
    ui: &mut egui::Ui,
    painter: &egui::Painter,
    rect: Rect,
    candle_view: Option<&CandleView>,
    broker_ids: &[BrokerId],
    broker_overviews: &[BrokerOverview],
    bar_width: f32,
    scale_mode: CandlePriceScaleMode,
    follow_criteria: CandleFollowCriteria,
    pip_size: f64,
    chart_anchor: &mut Option<f64>,
    margin_edge_latch: &mut Option<MarginEdgeLatchSide>,
    fallback_price: Option<f64>,
    chart_max_quote_age_ms: u64,
    now_mono: MonoNs,
    price_mode: PriceMode,
    theme: &ChartTheme,
    trade_items: Option<(&[ReplayTrade], &[ReplayTrade])>,
    selected_broker: Option<BrokerId>,
    show_current_price: bool,
) {
    draw_candlestick_chart_for_brokers_with_trades_impl(
        Some(ui),
        painter,
        rect,
        candle_view,
        broker_ids,
        broker_overviews,
        bar_width,
        scale_mode,
        follow_criteria,
        pip_size,
        chart_anchor,
        margin_edge_latch,
        fallback_price,
        chart_max_quote_age_ms,
        now_mono,
        price_mode,
        theme,
        trade_items,
        selected_broker,
        show_current_price,
    );
}

fn draw_candlestick_chart_for_brokers_with_trades_impl(
    ui: Option<&mut egui::Ui>,
    painter: &egui::Painter,
    rect: Rect,
    candle_view: Option<&CandleView>,
    broker_ids: &[BrokerId],
    broker_overviews: &[BrokerOverview],
    bar_width: f32,
    scale_mode: CandlePriceScaleMode,
    follow_criteria: CandleFollowCriteria,
    pip_size: f64,
    chart_anchor: &mut Option<f64>,
    margin_edge_latch: &mut Option<MarginEdgeLatchSide>,
    fallback_price: Option<f64>,
    chart_max_quote_age_ms: u64,
    now_mono: MonoNs,
    price_mode: PriceMode,
    theme: &ChartTheme,
    trade_items: Option<(&[ReplayTrade], &[ReplayTrade])>,
    selected_broker: Option<BrokerId>,
    show_current_price: bool,
) {
    painter.rect_filled(rect, 4.0, theme.bg_color);
    let plot_rect = chart_plot_rect(rect, CHART_HEADER_HEIGHT);
    let pointer_pos = ui.as_ref().and_then(|u| u.input(|i| i.pointer.hover_pos()));

    let view = match candle_view {
        Some(v) if !v.slot_starts.is_empty() => v,
        _ => {
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "Waiting for Candle Data...",
                egui::FontId::proportional(14.0),
                Color32::GRAY,
            );
            return;
        }
    };

    let broker_ids: Vec<BrokerId> = broker_ids
        .iter()
        .copied()
        .filter(|id| view.slots_by_broker.contains_key(id))
        .collect();
    if broker_ids.is_empty() {
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "Waiting for Broker Candle Data...",
            egui::FontId::proportional(14.0),
            Color32::GRAY,
        );
        return;
    }

    let broker_count = broker_ids.len() as f32;
    let candle_gap = 1.0_f32;
    let total_gap = candle_gap * (broker_count - 1.0).max(0.0);
    let candle_group_width = bar_width * broker_count + total_gap;
    let slot_width = (candle_group_width + 4.0).max(12.0);
    let total_slots = view.slot_starts.len();

    // Right-aligned anchor: latest slot (index total_slots - 1) is pinned to the right edge
    let right_slot_center_x = (plot_rect.right() - slot_width * 0.5).round();
    let available_width = (right_slot_center_x + slot_width * 0.5 - plot_rect.left()).max(0.0);
    let max_visible_slots = ((available_width / slot_width).ceil() as usize).max(1);
    let start_idx = total_slots.saturating_sub(max_visible_slots);

    // Find min and max prices across visible broker slots only
    let mut min_price = f64::MAX;
    let mut max_price = f64::MIN;

    for broker_id in &broker_ids {
        if let Some(slots) = view.slots_by_broker.get(broker_id) {
            for (i, s) in slots.iter().enumerate().skip(start_idx) {
                let offset_from_latest = (total_slots - 1).saturating_sub(i) as f32;
                let slot_center_x = right_slot_center_x - offset_from_latest * slot_width;
                if slot_center_x + slot_width * 0.5 >= plot_rect.left() {
                    if let Some(ohlc) = &s.ohlc {
                        min_price = min_price.min(ohlc.low);
                        max_price = max_price.max(ohlc.high);
                    }
                }
            }
        }
    }

    // Fallback if no visible slots have OHLC yet: inspect all available slots
    if min_price > max_price || min_price == f64::MAX {
        for broker_id in &broker_ids {
            if let Some(slots) = view.slots_by_broker.get(broker_id) {
                for s in slots {
                    if let Some(ohlc) = &s.ohlc {
                        min_price = min_price.min(ohlc.low);
                        max_price = max_price.max(ohlc.high);
                    }
                }
            }
        }
    }

    let has_ohlc = min_price <= max_price && min_price < f64::MAX;

    let candidates = extract_valid_quote_candidates_for_mode(
        &broker_ids,
        broker_overviews,
        chart_max_quote_age_ms,
        now_mono,
        price_mode,
    );

    let mut follow_status: Option<FollowStatusInfo> = None;

    let (chart_min, chart_max) = match scale_mode {
        CandlePriceScaleMode::Auto => {
            *chart_anchor = None;
            *margin_edge_latch = None;
            if has_ohlc {
                if (max_price - min_price).abs() < 1e-5 {
                    // Single price (High == Low): add a reasonable margin (e.g. ±0.025)
                    let margin = (min_price * 0.0005).max(0.025);
                    (min_price - margin, max_price + margin)
                } else {
                    // Add 10% padding
                    let price_padding = (max_price - min_price) * 0.1;
                    (min_price - price_padding, max_price + price_padding)
                }
            } else if let Some(fb) = fallback_price {
                // No candle data yet, but have current quote: center around quote
                let margin = (fb * 0.0005).max(0.025);
                (fb - margin, fb + margin)
            } else {
                // Neither candle data nor quote available
                painter.text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "Waiting for Tick Data...",
                    egui::FontId::proportional(14.0),
                    Color32::GRAY,
                );
                return;
            }
        }
        CandlePriceScaleMode::Fixed(span_pips) => {
            let (bounds, status) = resolve_candle_follow_scale(
                follow_criteria,
                &candidates,
                span_pips,
                pip_size,
                chart_anchor,
                margin_edge_latch,
                fallback_price,
            );
            follow_status = Some(status);
            bounds
        }
    };

    let price_range = (chart_max - chart_min).max(0.0001);
    let price_decimals = price_decimals_for_pip(pip_size);

    let price_to_y = |p: f64| -> f32 {
        let normalized = (chart_max - p) / price_range;
        plot_rect.top() + (normalized as f32) * plot_rect.height()
    };

    // Draw horizontal price grid lines
    let grid_steps = 4;
    for i in 0..=grid_steps {
        let p = chart_min + (price_range / f64::from(grid_steps)) * f64::from(i);
        let y = price_to_y(p);
        painter.line_segment(
            [
                Pos2::new(plot_rect.left(), y),
                Pos2::new(plot_rect.right(), y),
            ],
            Stroke::new(1.0_f32, theme.grid_color),
        );
        painter.text(
            Pos2::new(rect.right() - 4.0, y - 2.0),
            egui::Align2::RIGHT_BOTTOM,
            format!("{p:.price_decimals$}"),
            egui::FontId::monospace(11.0),
            crate::ui::style::TEXT_SUBDUED,
        );
    }

    if let CandlePriceScaleMode::Fixed(span) = scale_mode {
        let half_span = span / 2.0;
        let detail_str = match &follow_status {
            Some(FollowStatusInfo::Median { valid_brokers, .. }) => {
                if *valid_brokers >= 3 {
                    format!("(Median, {valid_brokers} brokers)")
                } else if *valid_brokers == 2 {
                    "(Mid of 2 brokers)".to_string()
                } else if *valid_brokers == 1 {
                    "(1 broker)".to_string()
                } else {
                    "(0 brokers - Hold)".to_string()
                }
            }
            Some(FollowStatusInfo::MarginEdge {
                edge_broker_name,
                edge_age_ms,
                is_top,
                valid_brokers,
            }) => {
                let side_str = if *is_top { "High" } else { "Low" };
                if *valid_brokers > 0 {
                    format!("(Edge {side_str}: {edge_broker_name} {edge_age_ms}ms)")
                } else {
                    "(Edge: 0 brokers - Hold)".to_string()
                }
            }
            Some(FollowStatusInfo::NoValidQuotes { .. }) => "(0 valid quotes - Hold)".to_string(),
            None => String::new(),
        };
        let half_span_str = if (half_span.fract()).abs() < 1e-4 {
            format!("{half_span:.0}")
        } else if ((half_span * 10.0).fract()).abs() < 1e-4 {
            format!("{half_span:.1}")
        } else {
            format!("{half_span:.2}")
        };
        painter.text(
            Pos2::new(rect.right() - PRICE_AXIS_WIDTH - 6.0, rect.top() + 6.0),
            egui::Align2::RIGHT_TOP,
            format!("Fixed: ±{half_span_str} pip {detail_str}"),
            egui::FontId::monospace(11.0),
            crate::ui::style::TEXT_OVERLAY,
        );
    }

    if !has_ohlc {
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "Waiting for Candle Data in current window...\n(Ensure broker UTC offset is verified)",
            egui::FontId::proportional(13.0),
            crate::ui::style::TEXT_FAINT,
        );
    }

    // Draw candles with user-specified bar_width and pixel-aligned layout
    let plot_painter = painter.with_clip_rect(plot_rect);

    for i in start_idx..total_slots {
        let offset_from_latest = (total_slots - 1).saturating_sub(i) as f32;
        let slot_center_x = (right_slot_center_x - offset_from_latest * slot_width).round();

        if slot_center_x + slot_width * 0.5 < plot_rect.left() {
            continue;
        }

        for (broker_index, broker_id) in broker_ids.iter().enumerate() {
            if let Some(s) = view
                .slots_by_broker
                .get(broker_id)
                .and_then(|slots| slots.get(i))
            {
                if s.state != SlotState::Empty {
                    if let Some(ohlc) = &s.ohlc {
                        let group_left = slot_center_x - candle_group_width * 0.5;
                        let cx = (group_left
                            + broker_index as f32 * (bar_width + candle_gap)
                            + bar_width * 0.5)
                            .round();
                        let color_index = crate::ui::shared::broker_index(
                            broker_overviews,
                            *broker_id,
                            broker_index,
                        );
                        let broker_name = broker_overviews
                            .iter()
                            .find(|b| b.broker_id == *broker_id)
                            .map(|b| b.name.as_str());
                        let color = broker_color_for_name(theme, broker_name, color_index);
                        draw_single_candle(
                            &plot_painter,
                            cx,
                            bar_width,
                            ohlc,
                            price_to_y,
                            color,
                            dim_candle_color(color),
                        );

                        // Draw out-of-bounds clip indicators if candle exceeds chart boundaries
                        if ohlc.high > chart_max {
                            draw_clip_marker(painter, cx, plot_rect.top() + 3.0, true, color);
                        }
                        if ohlc.low < chart_min {
                            draw_clip_marker(painter, cx, plot_rect.bottom() - 3.0, false, color);
                        }
                    }
                }
            }
        }
    }

    // Keep the mapping visible when multiple brokers overlap at the same
    // price. The legend uses broker identity colors only.
    let mut legend_x = rect.left() + 8.0;
    let mut legend_y = rect.top() + 6.0;
    let mut hidden_legends = 0;
    for (broker_index, broker_id) in broker_ids.iter().enumerate() {
        let color_index =
            crate::ui::shared::broker_index(broker_overviews, *broker_id, broker_index);
        let name = broker_name(broker_overviews, *broker_id, "Broker");
        let color = broker_color_for_name(theme, Some(name), color_index);
        let label = format!("{name} [{broker_id}]");
        let width = 8.0 + label.len() as f32 * 7.0 + 12.0;
        if legend_x + width > rect.right() - 8.0 {
            legend_x = rect.left() + 8.0;
            legend_y += 14.0;
        }
        if legend_y + 12.0 > plot_rect.top() {
            hidden_legends += 1;
            continue;
        }
        painter.rect_filled(
            Rect::from_min_size(
                Pos2::new(legend_x, legend_y + 2.0),
                egui::Vec2::new(7.0, 7.0),
            ),
            1.0,
            color,
        );
        painter.text(
            Pos2::new(legend_x + 11.0, legend_y),
            egui::Align2::LEFT_TOP,
            label,
            egui::FontId::monospace(10.0),
            color,
        );
        legend_x += width;
    }
    if hidden_legends > 0 {
        painter.text(
            Pos2::new(rect.right() - PRICE_AXIS_WIDTH - 6.0, rect.top() + 20.0),
            egui::Align2::RIGHT_TOP,
            format!("+{hidden_legends} brokers"),
            egui::FontId::monospace(11.0),
            crate::ui::style::TEXT_SUBDUED,
        );
    }

    let live_market_price = fallback_price.or_else(|| {
        broker_overviews
            .iter()
            .find_map(|b| b.latest_quote.as_ref().map(|q| q.mid))
    });

    if let Some((open_pos, hist)) = trade_items {
        draw_trade_overlays_impl(
            ui,
            painter,
            rect,
            plot_rect,
            view,
            candle_group_width,
            right_slot_center_x,
            slot_width,
            price_to_y,
            open_pos,
            hist,
            pip_size,
            price_decimals,
            live_market_price,
        );
    }

    // Draw current price line and price axis badge for selected broker
    if show_current_price {
        let target_broker_id = selected_broker.or_else(|| broker_ids.first().copied());
        if let Some(target_id) = target_broker_id {
            let target_overview = broker_overviews.iter().find(|b| b.broker_id == target_id);
            let cur_price = target_overview
                .and_then(|b| b.latest_quote.as_ref())
                .map(|q| match price_mode {
                    PriceMode::Bid => q.bid,
                    PriceMode::Ask => q.ask,
                    PriceMode::Mid => q.mid,
                })
                .or_else(|| {
                    view.slots_by_broker
                        .get(&target_id)
                        .and_then(|slots| slots.iter().rev().find_map(|s| s.ohlc.as_ref().map(|o| o.close)))
                })
                .or(fallback_price);

            if let Some(price) = cur_price {
                let cur_y = price_to_y(price);
                if cur_y >= plot_rect.top() && cur_y <= plot_rect.bottom() {
                    let broker_index = broker_ids.iter().position(|&id| id == target_id).unwrap_or(0);
                    let color_index = crate::ui::shared::broker_index(broker_overviews, target_id, broker_index);
                    let target_name = target_overview.map(|b| b.name.as_str());
                    let broker_color = broker_color_for_name(theme, target_name, color_index);

                    // 1. Current price horizontal line across the plot
                    painter.line_segment(
                        [
                            Pos2::new(plot_rect.left(), cur_y),
                            Pos2::new(plot_rect.right() + 4.0, cur_y),
                        ],
                        Stroke::new(1.0_f32, broker_color.gamma_multiply(0.85)),
                    );

                    // 2. Current price badge on the right price axis
                    let axis_left = plot_rect.right();
                    let axis_right = rect.right();
                    if axis_right > axis_left + 24.0 {
                        let full_price = format!("{price:.price_decimals$}");
                        let font_id = egui::FontId::monospace(10.0);
                        let galley = painter.layout_no_wrap(full_price, font_id, Color32::WHITE);
                        let text_w = galley.size().x;
                        let badge_w = (text_w + 8.0).clamp(54.0, 84.0);
                        let badge_h = 15.0;

                        let badge_rect = Rect::from_center_size(
                            Pos2::new(axis_left + badge_w * 0.5 + 4.0, cur_y),
                            egui::vec2(badge_w, badge_h),
                        );

                        painter.rect_filled(badge_rect, 2.0, Color32::from_rgb(30, 40, 55));
                        painter.rect_stroke(badge_rect, 2.0, Stroke::new(1.0_f32, broker_color));
                        painter.galley(
                            Pos2::new(
                                badge_rect.center().x - text_w * 0.5,
                                badge_rect.center().y - galley.size().y * 0.5,
                            ),
                            galley,
                            Color32::WHITE,
                        );
                    }
                }
            }
        }
    }

    // Mouse hover crosshair and price/slot inspection
    if let Some(pos) = pointer_pos {
        if plot_rect.contains(pos) {
            // 1. Horizontal crosshair at pointer Y
            painter.line_segment(
                [Pos2::new(plot_rect.left(), pos.y), Pos2::new(plot_rect.right(), pos.y)],
                Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(180, 210, 255, 70)),
            );

            // 2. Hovered price badge on the right price axis
            let norm_y = (pos.y - plot_rect.top()) / plot_rect.height();
            let hover_price = chart_max - (norm_y as f64) * price_range;
            let axis_left = plot_rect.right();
            let axis_right = rect.right();
            if axis_right > axis_left + 24.0 {
                let full_price = format!("{hover_price:.price_decimals$}");
                let font_id = egui::FontId::monospace(10.0);
                let galley = painter.layout_no_wrap(full_price, font_id, Color32::WHITE);
                let text_w = galley.size().x;
                let badge_w = (text_w + 8.0).clamp(54.0, 84.0);
                let badge_h = 15.0;

                let badge_cy = pos.y.clamp(plot_rect.top() + 8.0, plot_rect.bottom() - 8.0);
                let badge_rect = Rect::from_center_size(
                    Pos2::new(axis_left + badge_w * 0.5 + 4.0, badge_cy),
                    egui::vec2(badge_w, badge_h),
                );

                painter.rect_filled(badge_rect, 2.0, Color32::from_rgb(30, 45, 65));
                painter.rect_stroke(
                    badge_rect,
                    2.0,
                    Stroke::new(1.0_f32, Color32::from_rgb(100, 180, 255)),
                );
                painter.galley(
                    Pos2::new(
                        badge_rect.center().x - text_w * 0.5,
                        badge_rect.center().y - galley.size().y * 0.5,
                    ),
                    galley,
                    Color32::from_rgb(180, 220, 255),
                );
            }

            // 3. Find closest slot in visible viewport
            let offset_f = (right_slot_center_x - pos.x) / slot_width;
            let offset_round = offset_f.round();
            let idx_from_latest = if offset_round <= 0.0 {
                0usize
            } else {
                offset_round as usize
            };

            if idx_from_latest < total_slots {
                let slot_idx = (total_slots - 1) - idx_from_latest;
                if slot_idx >= start_idx && slot_idx < total_slots {
                    let hovered_slot_cx = (right_slot_center_x - (idx_from_latest as f32) * slot_width).round();

                    // Find closest broker candle within this slot
                    let group_left = hovered_slot_cx - candle_group_width * 0.5;
                    let mut closest_broker = None;
                    let mut min_dist = f32::MAX;

                    for (b_idx, &b_id) in broker_ids.iter().enumerate() {
                        let bcx = (group_left + (b_idx as f32) * (bar_width + candle_gap) + bar_width * 0.5).round();
                        let dist = (pos.x - bcx).abs();
                        if dist < min_dist {
                            min_dist = dist;
                            closest_broker = Some((b_id, bcx));
                        }
                    }

                    let (target_id, target_cx) = closest_broker
                        .unwrap_or_else(|| (selected_broker.unwrap_or(1), hovered_slot_cx));

                    // Vertical crosshair line at hovered broker candle center
                    painter.line_segment(
                        [
                            Pos2::new(target_cx, plot_rect.top()),
                            Pos2::new(target_cx, plot_rect.bottom()),
                        ],
                        Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(180, 210, 255, 90)),
                    );

                    // Time label on footer at vertical crosshair
                    if let Some(utc_ms) = view.slot_starts.get(slot_idx) {
                        let day_ms = utc_ms.0.rem_euclid(86_400_000);
                        let hrs = day_ms / 3_600_000;
                        let mins = (day_ms % 3_600_000) / 60_000;
                        let secs = (day_ms % 60_000) / 1_000;
                        let time_str = format!("{hrs:02}:{mins:02}:{secs:02}");

                        let badge_cx = target_cx.clamp(rect.left() + 28.0, plot_rect.right() - 28.0);
                        let time_badge_rect = Rect::from_center_size(
                            Pos2::new(badge_cx, plot_rect.bottom() + 9.0),
                            egui::vec2(52.0, 14.0),
                        );
                        painter.rect_filled(time_badge_rect, 2.0, Color32::from_rgb(25, 35, 50));
                        painter.rect_stroke(
                            time_badge_rect,
                            1.0,
                            Stroke::new(1.0_f32, Color32::from_rgb(100, 180, 255)),
                        );
                        painter.text(
                            time_badge_rect.center(),
                            egui::Align2::CENTER_CENTER,
                            time_str,
                            egui::FontId::monospace(9.5),
                            Color32::from_rgb(180, 220, 255),
                        );

                        // 4. Header inspection info (OHLC for closest broker)
                        let b_name = broker_name(broker_overviews, target_id, "Broker");
                        let slot_opt = view.slots_by_broker.get(&target_id).and_then(|slots| slots.get(slot_idx));
                        if let Some(ohlc) = slot_opt.and_then(|s| s.ohlc.as_ref()) {
                            let diff_pips = (ohlc.close - ohlc.open) / pip_size;
                            let header_info = format!(
                                "{b_name} | HOVER [{hrs:02}:{mins:02}:{secs:02}] | O: {:.prec$} H: {:.prec$} L: {:.prec$} C: {:.prec$} ({diff_pips:+.1}p)",
                                ohlc.open, ohlc.high, ohlc.low, ohlc.close,
                                prec = price_decimals,
                            );

                            let galley = painter.layout_no_wrap(
                                header_info,
                                egui::FontId::monospace(11.0),
                                Color32::from_rgb(255, 215, 100), // Gold
                            );
                            let bg_rect = Rect::from_min_size(
                                Pos2::new(plot_rect.left() + 4.0, rect.top() + 21.0),
                                egui::vec2(galley.size().x + 8.0, 16.0),
                            );
                            painter.rect_filled(
                                bg_rect,
                                2.0,
                                Color32::from_rgba_unmultiplied(20, 30, 45, 220),
                            );
                            painter.galley(
                                Pos2::new(bg_rect.left() + 4.0, bg_rect.top() + 1.0),
                                galley,
                                Color32::from_rgb(255, 215, 100),
                            );
                        }
                    }
                }
            }
        }
    }
}

fn draw_trade_overlays_impl(
    mut ui: Option<&mut egui::Ui>,
    painter: &egui::Painter,
    chart_rect: Rect,
    plot_rect: Rect,
    view: &CandleView,
    candle_group_width: f32,
    right_slot_center_x: f32,
    slot_width: f32,
    price_to_y: impl Fn(f64) -> f32,
    open_positions: &[ReplayTrade],
    history: &[ReplayTrade],
    pip_size: f64,
    price_decimals: usize,
    live_market_price: Option<f64>,
) {
    if view.slot_starts.is_empty() || view.period_ms <= 0 {
        return;
    }

    let clip_painter = painter.with_clip_rect(plot_rect);

    // 1. 履歴トレード (振り返りモード時に全履歴を展開)
    for trade in history {
        let (entry_left, entry_right, entry_cx) = slot_span_for_time(
            view,
            trade.open_utc_ms,
            right_slot_center_x,
            slot_width,
            candle_group_width,
        );
        let entry_y = price_to_y(trade.open_price);
        let entry_pt = Pos2::new(entry_cx, entry_y);

        let exit_data = trade
            .close_utc_ms
            .zip(trade.close_price)
            .map(|(close_time, close_p)| {
                let (x_left, x_right, x_cx) = slot_span_for_time(
                    view,
                    close_time,
                    right_slot_center_x,
                    slot_width,
                    candle_group_width,
                );
                let y = price_to_y(close_p);
                (x_left, x_right, x_cx, y, Pos2::new(x_cx, y))
            });

        let entry_visible = plot_rect.contains(entry_pt)
            || (entry_y >= plot_rect.top()
                && entry_y <= plot_rect.bottom()
                && entry_right >= plot_rect.left()
                && entry_left <= plot_rect.right());
        let exit_visible = exit_data.as_ref().is_some_and(|d| {
            plot_rect.contains(d.4)
                || (d.3 >= plot_rect.top()
                    && d.3 <= plot_rect.bottom()
                    && d.1 >= plot_rect.left()
                    && d.0 <= plot_rect.right())
        });

        if !entry_visible && !exit_visible {
            continue;
        }

        let is_buy = trade.side.eq_ignore_ascii_case("BUY");
        let entry_color = if is_buy {
            crate::ui::chart::theme::TRADE_ENTRY_BUY
        } else {
            crate::ui::chart::theme::TRADE_ENTRY_SELL
        };
        let exit_color = if trade.profit >= 0.0 {
            crate::ui::chart::theme::TRADE_EXIT_PROFIT
        } else {
            crate::ui::chart::theme::TRADE_EXIT_LOSS
        };

        // バー全体をホバー判定領域にする
        let entry_bar_rect = Rect::from_min_max(
            Pos2::new(entry_left.max(plot_rect.left()), entry_y - 6.0),
            Pos2::new(entry_right.min(plot_rect.right()), entry_y + 6.0),
        );
        let entry_hovered = entry_visible
            && interact_trade_rect(
                ui.as_deref_mut(),
                entry_bar_rect,
                plot_rect,
                trade,
                0,
                true,
                pip_size,
                price_decimals,
            );

        let exit_hovered = if let Some((x_left, x_right, _, y, _)) = exit_data {
            if exit_visible {
                let exit_bar_rect = Rect::from_min_max(
                    Pos2::new(x_left.max(plot_rect.left()), y - 6.0),
                    Pos2::new(x_right.min(plot_rect.right()), y + 6.0),
                );
                interact_trade_rect(
                    ui.as_deref_mut(),
                    exit_bar_rect,
                    plot_rect,
                    trade,
                    1,
                    true,
                    pip_size,
                    price_decimals,
                )
            } else {
                false
            }
        } else {
            false
        };

        // トレード結果ライン（エントリー〜エグジットの結線）
        if let Some((_, _, _, _, exit_pt)) = exit_data {
            let line_color = if trade.profit >= 0.0 {
                crate::ui::chart::theme::TRADE_LINE_PROFIT
            } else {
                crate::ui::chart::theme::TRADE_LINE_LOSS
            };
            let stroke = if entry_hovered || exit_hovered {
                Stroke::new(1.6_f32, line_color.gamma_multiply(0.9))
            } else {
                Stroke::new(1.0_f32, line_color.gamma_multiply(0.4))
            };
            clip_painter.line_segment([entry_pt, exit_pt], stroke);
        }

        // エントリー足: 全ブローカー横断バー
        if entry_visible {
            draw_slot_cross_bar(
                &clip_painter,
                entry_left.max(plot_rect.left()),
                entry_right.min(plot_rect.right()),
                entry_y,
                entry_color,
                is_buy,
                true,
            );
        }

        // 決済足: 全ブローカー横断バー & ×印
        if let Some((x_left, x_right, x_cx, y, _)) = exit_data {
            if exit_visible {
                draw_slot_cross_bar(
                    &clip_painter,
                    x_left.max(plot_rect.left()),
                    x_right.min(plot_rect.right()),
                    y,
                    exit_color,
                    !is_buy,
                    false,
                );
                draw_exit_marker(&clip_painter, Pos2::new(x_cx, y), exit_color);
            }
        }
    }

    // 2. オープンポジション (建玉保有中)
    for trade in open_positions {
        let is_buy = trade.side.eq_ignore_ascii_case("BUY");
        let (entry_left, entry_right, _) = slot_span_for_time(
            view,
            trade.open_utc_ms,
            right_slot_center_x,
            slot_width,
            candle_group_width,
        );
        let entry_y = price_to_y(trade.open_price);
        let entry_color = if is_buy {
            crate::ui::chart::theme::TRADE_MARK_BUY
        } else {
            crate::ui::chart::theme::TRADE_MARK_SELL
        };

        let entry_bar_rect = Rect::from_min_max(
            Pos2::new(entry_left.max(plot_rect.left()), entry_y - 6.0),
            Pos2::new(plot_rect.right(), entry_y + 6.0),
        );
        let hovered = interact_trade_rect(
            ui.as_deref_mut(),
            entry_bar_rect,
            plot_rect,
            trade,
            2,
            false,
            pip_size,
            price_decimals,
        );

        // ① エントリー足: 全ブローカー横断バー
        let bar_left = entry_left.clamp(plot_rect.left(), plot_rect.right());
        let bar_right = entry_right.clamp(plot_rect.left(), plot_rect.right());
        if bar_right > bar_left {
            draw_slot_cross_bar(
                &clip_painter,
                bar_left,
                bar_right,
                entry_y,
                entry_color,
                is_buy,
                true,
            );
        }

        // ② エントリー足の右端から最新足・価格軸への極薄ガイド線
        let guide_start_x = bar_right.max(plot_rect.left());
        if plot_rect.right() > guide_start_x {
            clip_painter.line_segment(
                [
                    Pos2::new(guide_start_x, entry_y),
                    Pos2::new(plot_rect.right(), entry_y),
                ],
                Stroke::new(
                    if hovered { 1.2_f32 } else { 0.8_f32 },
                    entry_color.gamma_multiply(if hovered { 0.7 } else { 0.28 }),
                ),
            );
        }

        // ③ 右側価格軸上のポジションタグ (形式B: 建値 (+pips))
        let cur_price = live_market_price.or(trade.current_price);
        let pips = if pip_size > 0.0 {
            if let Some(cur) = cur_price {
                let diff = if is_buy {
                    cur - trade.open_price
                } else {
                    trade.open_price - cur
                };
                diff / pip_size
            } else {
                0.0
            }
        } else {
            0.0
        };
        draw_price_axis_position_badge(
            painter,
            plot_rect,
            chart_rect,
            entry_y,
            trade.open_price,
            pips,
            price_decimals,
            entry_color,
        );
    }
}

fn slot_span_for_time(
    view: &CandleView,
    utc_ms: i64,
    right_slot_center_x: f32,
    slot_width: f32,
    candle_group_width: f32,
) -> (f32, f32, f32) {
    let total_slots = view.slot_starts.len();
    if total_slots == 0 || view.period_ms <= 0 {
        return (0.0, 0.0, 0.0);
    }

    let slot_idx = match view.slot_starts.binary_search_by_key(&utc_ms, |s| s.0) {
        Ok(idx) => Some(idx),
        Err(idx) => {
            if idx > 0 {
                let start = view.slot_starts[idx - 1].0;
                if utc_ms < start + view.period_ms {
                    Some(idx - 1)
                } else if idx < total_slots {
                    Some(idx)
                } else {
                    None
                }
            } else {
                Some(0)
            }
        }
    };

    let cx = if let Some(idx) = slot_idx {
        let offset = (total_slots - 1).saturating_sub(idx) as f32;
        (right_slot_center_x - offset * slot_width).round()
    } else {
        let latest_slot_start = view.slot_starts.last().map_or(0, |s| s.0);
        let latest_slot_center_time = latest_slot_start + (view.period_ms / 2);
        let dt = utc_ms.saturating_sub(latest_slot_center_time) as f64;
        right_slot_center_x + (dt / view.period_ms as f64) as f32 * slot_width
    };

    let half_w = (candle_group_width * 0.5 + 2.0).max(8.0);
    (cx - half_w, cx + half_w, cx)
}

fn draw_slot_cross_bar(
    painter: &egui::Painter,
    left: f32,
    right: f32,
    y: f32,
    color: Color32,
    is_buy: bool,
    show_marker: bool,
) {
    if right <= left {
        return;
    }
    // 全ブローカーを跨ぐ横断バー本体
    painter.line_segment(
        [Pos2::new(left, y), Pos2::new(right, y)],
        Stroke::new(1.6_f32, color),
    );
    // スロット範囲を明確にする両端キャップ
    painter.line_segment(
        [Pos2::new(left, y - 2.5), Pos2::new(left, y + 2.5)],
        Stroke::new(1.5_f32, color),
    );
    painter.line_segment(
        [Pos2::new(right, y - 2.5), Pos2::new(right, y + 2.5)],
        Stroke::new(1.5_f32, color),
    );
    // 方向を示す中央の小さなエントリーマーカー
    if show_marker {
        let cx = (left + right) * 0.5;
        draw_entry_marker(painter, cx, y, is_buy, color);
    }
}

fn draw_price_axis_position_badge(
    painter: &egui::Painter,
    plot_rect: Rect,
    chart_rect: Rect,
    y: f32,
    open_price: f64,
    pips: f64,
    price_decimals: usize,
    color: Color32,
) {
    let axis_left = plot_rect.right();
    let axis_right = chart_rect.right();
    if axis_right <= axis_left + 24.0 {
        return;
    }

    let badge_h = 16.0;
    let badge_y = y.clamp(
        plot_rect.top() + badge_h * 0.5,
        plot_rect.bottom() - badge_h * 0.5,
    );

    // 整数部を省略した建値 (例: 157.835 -> ".835") と 評価pips (例: "0.5p", "-1.2p")
    let full_price = format!("{open_price:.price_decimals$}");
    let decimal_price = match full_price.find('.') {
        Some(idx) => &full_price[idx..],
        None => &full_price,
    };
    let pips_val = if pips.abs() < 0.05 { 0.0 } else { pips };
    let text = format!("{decimal_price}({pips_val:.1}p)");
    let bg_color = if pips >= 0.0 {
        crate::ui::chart::theme::trade_tooltip_profit_bg()
    } else {
        crate::ui::chart::theme::trade_tooltip_loss_bg()
    };

    let font_id = egui::FontId::monospace(10.0);
    let galley = painter.layout_no_wrap(text, font_id, Color32::WHITE);
    let text_w = galley.size().x;
    let badge_w = (text_w + 6.0).clamp(52.0, 84.0);

    let badge_rect = Rect::from_center_size(
        Pos2::new(axis_left + badge_w * 0.5 + 2.0, badge_y),
        egui::vec2(badge_w, badge_h),
    );

    painter.rect_filled(badge_rect, 3.0, bg_color);
    painter.rect_stroke(badge_rect, 3.0, Stroke::new(1.0_f32, color));
    painter.galley(
        Pos2::new(
            badge_rect.center().x - text_w * 0.5,
            badge_rect.center().y - galley.size().y * 0.5,
        ),
        galley,
        Color32::WHITE,
    );
}

fn interact_trade_rect(
    ui: Option<&mut egui::Ui>,
    rect: Rect,
    plot_rect: Rect,
    trade: &ReplayTrade,
    marker_kind: u8,
    is_closed: bool,
    pip_size: f64,
    price_decimals: usize,
) -> bool {
    let Some(ui) = ui else {
        return false;
    };
    let hit_rect = rect.intersect(plot_rect).intersect(ui.clip_rect());
    if hit_rect.width() <= 0.0 || hit_rect.height() <= 0.0 {
        return false;
    }
    let response = ui.interact(
        hit_rect,
        egui::Id::new((
            "replay_trade_bar",
            trade.symbol.as_str(),
            trade.ticket,
            trade.open_time_msc,
            marker_kind,
        )),
        egui::Sense::hover(),
    );
    let hovered = response.hovered();
    if hovered {
        response
            .on_hover_ui(|ui| show_trade_tooltip(ui, trade, is_closed, pip_size, price_decimals));
    }
    hovered
}

fn show_trade_tooltip(
    ui: &mut egui::Ui,
    trade: &ReplayTrade,
    is_closed: bool,
    pip_size: f64,
    price_decimals: usize,
) {
    let symbol = if trade.symbol.is_empty() {
        "トレード"
    } else {
        trade.symbol.as_str()
    };
    let is_buy = trade.side.eq_ignore_ascii_case("BUY");
    let side = if is_buy {
        "買い"
    } else if trade.side.eq_ignore_ascii_case("SELL") {
        "売り"
    } else {
        trade.side.as_str()
    };

    ui.strong(format!("{}  #{}", symbol, trade.ticket));
    ui.label(format!("{}  {:.2} lot", side, trade.volume));
    ui.separator();
    ui.label(format!(
        "新規  {}  @ {:.*}",
        format_utc_timestamp(trade.open_utc_ms),
        price_decimals,
        trade.open_price
    ));

    if is_closed {
        if let (Some(close_time), Some(close_price)) = (trade.close_utc_ms, trade.close_price) {
            ui.label(format!(
                "決済  {}  @ {:.*}",
                format_utc_timestamp(close_time),
                price_decimals,
                close_price
            ));
            if pip_size > 0.0 {
                let price_delta = if is_buy {
                    close_price - trade.open_price
                } else {
                    trade.open_price - close_price
                };
                ui.label(format!("値幅  {:+.1} pips", price_delta / pip_size));
            }
            let duration_ms = close_time.saturating_sub(trade.open_utc_ms);
            ui.label(format!("保有時間  {}", format_trade_duration(duration_ms)));
        }
        ui.label(format!("損益  {:+.2}", trade.profit));
        if let Some(reason) = trade
            .close_reason
            .as_deref()
            .filter(|reason| !reason.trim().is_empty())
        {
            ui.label(format!("決済理由  {reason}"));
        }
    } else {
        if let Some(current_price) = trade.current_price {
            ui.label(format!("現在値  {current_price:.price_decimals$}"));
            if pip_size > 0.0 {
                let price_delta = if is_buy {
                    current_price - trade.open_price
                } else {
                    trade.open_price - current_price
                };
                ui.label(format!("含み値幅  {:+.1} pips", price_delta / pip_size));
            }
        }
        ui.label(format!("含み損益  {:+.2}", trade.profit));
        if let Some(sl) = trade.sl.filter(|price| *price > 0.0) {
            ui.label(format!("損切り  {sl:.price_decimals$}"));
        }
        if let Some(tp) = trade.tp.filter(|price| *price > 0.0) {
            ui.label(format!("利確  {tp:.price_decimals$}"));
        }
    }
}

/// Formats a UTC millisecond timestamp as `YYYY-MM-DD HH:MM:SS.mmm UTC`.
///
/// The calendar conversion is shared with the logger and the tlog writer through
/// [`crate::core::civil_date::civil_from_days`].
fn format_utc_timestamp(utc_ms: i64) -> String {
    let ms_of_day = utc_ms.rem_euclid(86_400_000);
    let (year, month, day_of_month) =
        crate::core::civil_date::civil_from_days(utc_ms.div_euclid(86_400_000));

    let hours = ms_of_day / 3_600_000;
    let minutes = (ms_of_day / 60_000) % 60;
    let seconds = (ms_of_day / 1_000) % 60;
    let millis = ms_of_day % 1_000;
    format!(
        "{year:04}-{month:02}-{day_of_month:02} {hours:02}:{minutes:02}:{seconds:02}.{millis:03} UTC"
    )
}

fn format_trade_duration(duration_ms: i64) -> String {
    let duration_ms = duration_ms.max(0);
    if duration_ms < 1_000 {
        format!("{duration_ms} ms")
    } else if duration_ms < 60_000 {
        format!("{:.1} 秒", duration_ms as f64 / 1_000.0)
    } else {
        format!(
            "{} 分 {:02} 秒",
            duration_ms / 60_000,
            (duration_ms / 1_000) % 60
        )
    }
}

fn draw_entry_marker(painter: &egui::Painter, x: f32, y: f32, is_buy: bool, color: Color32) {
    let points = if is_buy {
        [
            Pos2::new(x, y - 5.0),
            Pos2::new(x - 3.5, y),
            Pos2::new(x + 3.5, y),
        ]
    } else {
        [
            Pos2::new(x, y + 5.0),
            Pos2::new(x - 3.5, y),
            Pos2::new(x + 3.5, y),
        ]
    };
    painter.add(egui::Shape::convex_polygon(
        points.to_vec(),
        color,
        Stroke::new(0.75_f32, Color32::BLACK.gamma_multiply(0.7)),
    ));
}

fn draw_exit_marker(painter: &egui::Painter, point: Pos2, color: Color32) {
    let radius = 3.0;
    painter.line_segment(
        [
            Pos2::new(point.x - radius, point.y - radius),
            Pos2::new(point.x + radius, point.y + radius),
        ],
        Stroke::new(1.25_f32, color),
    );
    painter.line_segment(
        [
            Pos2::new(point.x - radius, point.y + radius),
            Pos2::new(point.x + radius, point.y - radius),
        ],
        Stroke::new(1.25_f32, color),
    );
}
pub fn draw_single_candle<F>(
    painter: &egui::Painter,
    cx: f32,
    bar_width: f32,
    ohlc: &Ohlc,
    price_to_y: F,
    up_color: Color32,
    down_color: Color32,
) where
    F: Fn(f64) -> f32,
{
    let y_open = price_to_y(ohlc.open).round();
    let y_close = price_to_y(ohlc.close).round();
    let y_high = price_to_y(ohlc.high).round();
    let y_low = price_to_y(ohlc.low).round();

    let is_up = ohlc.close >= ohlc.open;
    let color = if is_up { up_color } else { down_color };

    // Wick: Snap to integer X coordinate for a 1px solid crisp vertical line
    let wick_x = cx.round();
    painter.line_segment(
        [Pos2::new(wick_x, y_high), Pos2::new(wick_x, y_low)],
        Stroke::new(1.0_f32, color),
    );

    // Body: Snap edges to integer coordinates to eliminate subpixel blurring
    let top_body = y_open.min(y_close);
    let bottom_body = y_open.max(y_close).max(top_body + 1.0);
    let half_width = (bar_width * 0.5).floor().max(1.0);
    let body_rect = Rect::from_min_max(
        Pos2::new(wick_x - half_width, top_body),
        Pos2::new(wick_x + half_width, bottom_body),
    );
    painter.rect_filled(body_rect, 0.0, color);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_utc_timestamp_known_values() {
        assert_eq!(format_utc_timestamp(0), "1970-01-01 00:00:00.000 UTC");
        // 2026-02-28 23:59:59.999 UTC: one of the dates the old divisor bug used to break.
        assert_eq!(
            format_utc_timestamp(1_772_323_199_999),
            "2026-02-28 23:59:59.999 UTC"
        );
    }
}
