use crate::core::models::{BrokerOverview, CandleView, Ohlc, PriceMode, ReplayTrade, SlotState};
use crate::core::types::{BrokerId, MonoNs};
use crate::ui::settings::{CandleFollowCriteria, CandlePriceScaleMode};
use super::common::{
    chart_plot_rect, price_decimals_for_pip, CHART_HEADER_HEIGHT, PRICE_AXIS_WIDTH,
};
use super::scale::{
    draw_clip_marker, extract_valid_quote_candidates_for_mode, resolve_candle_follow_scale,
    FollowStatusInfo, MarginEdgeLatchSide,
};
use super::theme::{broker_color_for_name, dim_candle_color, ChartTheme};
use egui::{Color32, Pos2, Rect, Stroke};

pub fn draw_candlestick_chart(
    painter: &egui::Painter,
    rect: Rect,
    candle_view: Option<&CandleView>,
    broker_a: BrokerId,
    broker_b: BrokerId,
    bar_width: f32,
    scale_mode: CandlePriceScaleMode,
    pip_size: f64,
    chart_anchor: &mut Option<f64>,
    fallback_price: Option<f64>,
    theme: &ChartTheme,
) {
    let broker_ids = [broker_a, broker_b];
    let mut latch = None;
    draw_candlestick_chart_for_brokers(
        painter,
        rect,
        candle_view,
        &broker_ids,
        &[],
        bar_width,
        scale_mode,
        CandleFollowCriteria::Median,
        pip_size,
        chart_anchor,
        &mut latch,
        fallback_price,
        1000,
        MonoNs(0),
        PriceMode::Bid,
        theme,
    );
}

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
            broker_ids.sort();
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
    painter.rect_filled(rect, 4.0, theme.bg_color);
    let plot_rect = chart_plot_rect(rect, CHART_HEADER_HEIGHT);

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
        let p = chart_min + (price_range / grid_steps as f64) * i as f64;
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
            format!("{:.*}", price_decimals, p),
            egui::FontId::monospace(11.0),
            Color32::from_gray(180),
        );
    }

    if let CandlePriceScaleMode::Fixed(span) = scale_mode {
        let half_span = span / 2.0;
        let detail_str = match &follow_status {
            Some(FollowStatusInfo::Median { valid_brokers, .. }) => {
                if *valid_brokers >= 3 {
                    format!("(Median, {} brokers)", valid_brokers)
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
                    format!("(Edge {}: {} {}ms)", side_str, edge_broker_name, edge_age_ms)
                } else {
                    "(Edge: 0 brokers - Hold)".to_string()
                }
            }
            Some(FollowStatusInfo::NoValidQuotes { .. }) => "(0 valid quotes - Hold)".to_string(),
            None => "".to_string(),
        };
        let half_span_str = if (half_span.fract()).abs() < 1e-4 {
            format!("{:.0}", half_span)
        } else if ((half_span * 10.0).fract()).abs() < 1e-4 {
            format!("{:.1}", half_span)
        } else {
            format!("{:.2}", half_span)
        };
        painter.text(
            Pos2::new(rect.right() - PRICE_AXIS_WIDTH - 6.0, rect.top() + 6.0),
            egui::Align2::RIGHT_TOP,
            format!("Fixed: ±{} pip {}", half_span_str, detail_str),
            egui::FontId::monospace(11.0),
            Color32::from_gray(190),
        );
    }

    if !has_ohlc {
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "Waiting for Candle Data in current window...\n(Ensure broker UTC offset is verified)",
            egui::FontId::proportional(13.0),
            Color32::from_gray(140),
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
                        let color_index = broker_overviews
                            .iter()
                            .position(|b| b.broker_id == *broker_id)
                            .unwrap_or(broker_index);
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
        let color_index = broker_overviews
            .iter()
            .position(|b| b.broker_id == *broker_id)
            .unwrap_or(broker_index);
        let name = broker_overviews
            .iter()
            .find(|b| b.broker_id == *broker_id)
            .map(|b| b.name.as_str())
            .unwrap_or("Broker");
        let color = broker_color_for_name(theme, Some(name), color_index);
        let label = format!("{} [{}]", name, broker_id);
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
            format!("+{} brokers", hidden_legends),
            egui::FontId::monospace(11.0),
            Color32::from_gray(180),
        );
    }

    if let Some((open_pos, hist)) = trade_items {
        draw_trade_overlays(
            painter,
            plot_rect,
            view,
            right_slot_center_x,
            slot_width,
            price_to_y,
            open_pos,
            hist,
            pip_size,
        );
    }
}

/// Draw trade entry lines, execution markers, and profit tags onto the candlestick chart plot.
pub fn draw_trade_overlays(
    painter: &egui::Painter,
    plot_rect: Rect,
    view: &CandleView,
    right_slot_center_x: f32,
    slot_width: f32,
    price_to_y: impl Fn(f64) -> f32,
    open_positions: &[ReplayTrade],
    history: &[ReplayTrade],
    pip_size: f64,
) {
    if view.slot_starts.is_empty() || view.period_ms <= 0 {
        return;
    }
    let latest_slot_start = view.slot_starts.last().map(|s| s.0).unwrap_or(0);
    let latest_slot_center_time = latest_slot_start + (view.period_ms / 2);
    let time_to_x = |utc_ms: i64| -> f32 {
        let dt = (utc_ms - latest_slot_center_time) as f64;
        let slot_offset = dt / (view.period_ms as f64);
        right_slot_center_x + (slot_offset as f32) * slot_width
    };

    let clip_painter = painter.with_clip_rect(plot_rect);

    // 1. Draw closed trade history within visible range
    for h in history {
        let open_x = time_to_x(h.open_utc_ms);
        let close_x = h.close_utc_ms.map(&time_to_x).unwrap_or(open_x);
        if (open_x < plot_rect.left() - 100.0 && close_x < plot_rect.left() - 100.0)
            || (open_x > plot_rect.right() + 100.0 && close_x > plot_rect.right() + 100.0)
        {
            continue;
        }

        let is_buy = h.side.eq_ignore_ascii_case("BUY");
        let open_y = price_to_y(h.open_price);
        let close_y = h.close_price.map(&price_to_y).unwrap_or(open_y);

        let trade_color = if is_buy {
            Color32::from_rgb(0, 200, 255)
        } else {
            Color32::from_rgb(255, 120, 180)
        };

        // Dashed / solid line connecting entry to exit
        if (close_x - open_x).abs() > 2.0 || (close_y - open_y).abs() > 2.0 {
            clip_painter.line_segment(
                [Pos2::new(open_x, open_y), Pos2::new(close_x, close_y)],
                Stroke::new(1.0_f32, trade_color.gamma_multiply(0.6)),
            );
        }

        // Entry marker
        if open_x >= plot_rect.left() - 10.0 && open_x <= plot_rect.right() + 10.0 {
            draw_entry_marker(&clip_painter, open_x, open_y, is_buy, trade_color);
        }

        // Exit marker & profit tag
        if close_x >= plot_rect.left() - 10.0 && close_x <= plot_rect.right() + 10.0 {
            clip_painter.circle_filled(
                Pos2::new(close_x, close_y),
                3.5,
                if h.profit >= 0.0 {
                    Color32::from_rgb(0, 230, 120)
                } else {
                    Color32::from_rgb(255, 60, 60)
                },
            );

            let profit_text = format!("{:+.0}円", h.profit);
            let text_color = if h.profit >= 0.0 {
                Color32::from_rgb(0, 255, 140)
            } else {
                Color32::from_rgb(255, 80, 80)
            };
            clip_painter.text(
                Pos2::new(close_x, close_y - 8.0),
                egui::Align2::CENTER_BOTTOM,
                profit_text,
                egui::FontId::monospace(10.0),
                text_color,
            );
        }
    }

    // 2. Draw open positions
    let mut placed_badge_ys: Vec<f32> = Vec::new();
    for pos in open_positions {
        let is_buy = pos.side.eq_ignore_ascii_case("BUY");
        let open_y = price_to_y(pos.open_price);
        let open_x = time_to_x(pos.open_utc_ms);

        let line_color = if is_buy {
            Color32::from_rgb(0, 230, 200)
        } else {
            Color32::from_rgb(255, 80, 120)
        };

        // Horizontal entry price line from open_x (clamped) to right edge
        let start_x = open_x.clamp(plot_rect.left(), plot_rect.right());
        clip_painter.line_segment(
            [Pos2::new(start_x, open_y), Pos2::new(plot_rect.right(), open_y)],
            Stroke::new(1.5_f32, line_color),
        );

        // Entry marker at (open_x, open_y)
        if open_x >= plot_rect.left() - 10.0 && open_x <= plot_rect.right() + 10.0 {
            draw_entry_marker(&clip_painter, open_x, open_y, is_buy, line_color);
        }

        // Draw SL line if set
        if let Some(sl) = pos.sl {
            if sl > 0.0 {
                let sl_y = price_to_y(sl);
                clip_painter.line_segment(
                    [Pos2::new(start_x, sl_y), Pos2::new(plot_rect.right(), sl_y)],
                    Stroke::new(1.0_f32, Color32::from_rgb(255, 60, 60)),
                );
                clip_painter.text(
                    Pos2::new(plot_rect.right() - 4.0, sl_y - 2.0),
                    egui::Align2::RIGHT_BOTTOM,
                    format!("SL {:.3}", sl),
                    egui::FontId::monospace(10.0),
                    Color32::from_rgb(255, 100, 100),
                );
            }
        }

        // Draw TP line if set
        if let Some(tp) = pos.tp {
            if tp > 0.0 {
                let tp_y = price_to_y(tp);
                clip_painter.line_segment(
                    [Pos2::new(start_x, tp_y), Pos2::new(plot_rect.right(), tp_y)],
                    Stroke::new(1.0_f32, Color32::from_rgb(0, 220, 100)),
                );
                clip_painter.text(
                    Pos2::new(plot_rect.right() - 4.0, tp_y - 2.0),
                    egui::Align2::RIGHT_BOTTOM,
                    format!("TP {:.3}", tp),
                    egui::FontId::monospace(10.0),
                    Color32::from_rgb(50, 255, 140),
                );
            }
        }

        // Profit badge tag at right edge
        let pips = if pip_size > 0.0 {
            if let Some(cur) = pos.current_price {
                let diff = if is_buy { cur - pos.open_price } else { pos.open_price - cur };
                diff / pip_size
            } else {
                0.0
            }
        } else {
            0.0
        };

        let badge_text = format!(
            "{} {:.2}L @ {:.3} | {:+.1}p ({:+.0}円)",
            if is_buy { "BUY" } else { "SELL" },
            pos.volume,
            pos.open_price,
            pips,
            pos.profit
        );

        let badge_bg = if pos.profit >= 0.0 {
            Color32::from_rgba_unmultiplied(0, 120, 60, 230)
        } else {
            Color32::from_rgba_unmultiplied(160, 30, 30, 230)
        };

        let font_id = egui::FontId::monospace(11.0);
        let galley = clip_painter.layout_no_wrap(badge_text, font_id, Color32::WHITE);
        let text_size = galley.size();

        // Clamp vertically within plot_rect so badge never bleeds into headers/metric chart
        let min_y = plot_rect.top() + text_size.y * 0.5 + 4.0;
        let max_y = plot_rect.bottom() - text_size.y * 0.5 - 4.0;
        let mut badge_y = open_y.clamp(min_y, max_y);

        // Anti-collision offset for stacked or closely-priced open position badges
        for &placed_y in &placed_badge_ys {
            if (badge_y - placed_y).abs() < (text_size.y + 6.0) {
                badge_y = placed_y + text_size.y + 6.0;
            }
        }
        if badge_y > max_y {
            badge_y = max_y;
        }
        placed_badge_ys.push(badge_y);

        let badge_pos = Pos2::new(plot_rect.right() - 6.0, badge_y);
        let badge_rect = Rect::from_min_size(
            Pos2::new(badge_pos.x - text_size.x - 6.0, badge_pos.y - text_size.y * 0.5 - 2.0),
            egui::Vec2::new(text_size.x + 6.0, text_size.y + 4.0),
        );
        clip_painter.rect_filled(badge_rect, 3.0, badge_bg);
        clip_painter.rect_stroke(badge_rect, 3.0, Stroke::new(1.0_f32, line_color));
        clip_painter.galley(Pos2::new(badge_rect.min.x + 3.0, badge_rect.min.y + 2.0), galley, Color32::WHITE);
    }
}

fn draw_entry_marker(
    painter: &egui::Painter,
    x: f32,
    y: f32,
    is_buy: bool,
    color: Color32,
) {
    if is_buy {
        let pts = [
            Pos2::new(x, y - 8.0),
            Pos2::new(x - 5.0, y - 1.0),
            Pos2::new(x + 5.0, y - 1.0),
        ];
        painter.add(egui::Shape::convex_polygon(
            pts.to_vec(),
            color,
            Stroke::new(1.0_f32, Color32::BLACK),
        ));
    } else {
        let pts = [
            Pos2::new(x, y + 8.0),
            Pos2::new(x - 5.0, y + 1.0),
            Pos2::new(x + 5.0, y + 1.0),
        ];
        painter.add(egui::Shape::convex_polygon(
            pts.to_vec(),
            color,
            Stroke::new(1.0_f32, Color32::BLACK),
        ));
    }
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
