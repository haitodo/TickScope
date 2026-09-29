use crate::core::models::{BrokerOverview, RealtimeQuotePoint};
use crate::core::types::{BrokerId, ConnectionState, FreshnessState, MonoNs};
use super::common::{
    chart_plot_rect, draw_x_axis_caption, is_live, price_decimals_for_pip, x_axis_coordinate,
    CHART_HEADER_HEIGHT,
};
use super::theme::{broker_color_for, dim_color, ChartTheme};
use egui::{Color32, Pos2, Rect, Stroke};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ChartXAxisMode {
    #[default]
    ReceiveTime,
    TickCount,
}

pub fn advance_chart_anchor(
    chart_anchor: &mut Option<f64>,
    center_price: f64,
    half_span: f64,
    deadzone_pct: f64,
) -> f64 {
    match chart_anchor {
        Some(anchor) => {
            let delta = center_price - *anchor;
            let deadzone_half = half_span * (1.0 - deadzone_pct.clamp(0.0, 0.95));
            let required_shift = (delta.abs() - deadzone_half).max(0.0) * delta.signum();
            // Move at most 12% of the visible range per repaint. This keeps a
            // large price move visible without replacing the whole chart at once.
            let max_step = half_span * 0.12;
            *anchor += required_shift.clamp(-max_step, max_step);
            *anchor
        }
        None => {
            *chart_anchor = Some(center_price);
            center_price
        }
    }
}

/// Realtime Quote Path Chart (RFC §23, §24, §60)
pub fn draw_realtime_quote_path_chart(
    painter: &egui::Painter,
    rect: Rect,
    quote_points: &[RealtimeQuotePoint],
    broker_overviews: &[BrokerOverview],
    selected_pair: (BrokerId, BrokerId),
    x_axis_mode: ChartXAxisMode,
    now_mono: MonoNs,
    visible_seconds: u64,
    visible_ticks: usize,
    pip_size: f64,
    fixed_follow_span_pips: f64,
    deadzone_pct: f64,
    chart_anchor: &mut Option<f64>,
    theme: &ChartTheme,
) {
    draw_realtime_quote_path_chart_with_visibility(
        painter,
        rect,
        quote_points,
        broker_overviews,
        None,
        selected_pair,
        x_axis_mode,
        now_mono,
        visible_seconds,
        visible_ticks,
        pip_size,
        fixed_follow_span_pips,
        deadzone_pct,
        chart_anchor,
        theme,
    );
}

/// Realtime Quote Path Chart with explicit visible brokers list
pub fn draw_realtime_quote_path_chart_with_visibility(
    painter: &egui::Painter,
    rect: Rect,
    quote_points: &[RealtimeQuotePoint],
    broker_overviews: &[BrokerOverview],
    visible_broker_ids: Option<&[BrokerId]>,
    selected_pair: (BrokerId, BrokerId),
    x_axis_mode: ChartXAxisMode,
    now_mono: MonoNs,
    visible_seconds: u64,
    visible_ticks: usize,
    pip_size: f64,
    fixed_follow_span_pips: f64,
    deadzone_pct: f64,
    chart_anchor: &mut Option<f64>,
    theme: &ChartTheme,
) {
    painter.rect_filled(rect, 4.0, theme.bg_color);

    if quote_points.is_empty() {
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "Waiting for Realtime Quote Data...",
            egui::FontId::proportional(14.0),
            Color32::GRAY,
        );
        return;
    }

    let latest_mid = quote_points.last().and_then(|p| {
        p.consensus_mid.or_else(|| {
            broker_overviews
                .iter()
                .find_map(|b| p.broker_mids.get(&b.broker_id).copied())
        })
    });

    let center_price = match latest_mid {
        Some(m) => m,
        None => return,
    };

    let pip_size = pip_size.max(f64::EPSILON);
    let half_span = (fixed_follow_span_pips * pip_size / 2.0).max(f64::EPSILON);
    let anchor = advance_chart_anchor(chart_anchor, center_price, half_span, deadzone_pct);

    let chart_min = anchor - half_span;
    let chart_max = anchor + half_span;
    let price_range = chart_max - chart_min;
    let plot_rect = chart_plot_rect(rect, CHART_HEADER_HEIGHT);
    let price_decimals = price_decimals_for_pip(pip_size);

    let price_to_y = |p: f64| -> f32 {
        let normalized = (chart_max - p) / price_range;
        plot_rect.top() + (normalized as f32) * plot_rect.height()
    };

    // Grid lines
    let grid_steps = 5;
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
            Color32::from_gray(185),
        );
    }

    // Header and plot have separate rectangles, so neither the legend nor the
    // price axis obscures the latest part of the line.
    painter.text(
        Pos2::new(rect.left() + 6.0, rect.top() + 4.0),
        egui::Align2::LEFT_TOP,
        format!("Follow: ±{:.1} pip", price_range / pip_size / 2.0),
        egui::FontId::monospace(11.0),
        Color32::from_gray(190),
    );

    draw_x_axis_caption(painter, rect, x_axis_mode, visible_seconds, visible_ticks);

    let mut ordered_brokers: Vec<(usize, &BrokerOverview)> = broker_overviews
        .iter()
        .enumerate()
        .filter(|(_, broker)| {
            visible_broker_ids
                .map(|v| v.contains(&broker.broker_id))
                .unwrap_or(true)
        })
        .collect();
    ordered_brokers.sort_by_key(|(_, broker)| {
        if broker.broker_id == selected_pair.0 || broker.broker_id == selected_pair.1 {
            1
        } else {
            0
        }
    });

    let mut legend_x = rect.left() + 172.0;
    let mut legend_y = rect.top() + 5.0;
    let mut hidden_legends = 0;
    for (index, broker) in &ordered_brokers {
        let selected = broker.broker_id == selected_pair.0 || broker.broker_id == selected_pair.1;
        let role = if broker.broker_id == selected_pair.0 {
            "A"
        } else if broker.broker_id == selected_pair.1 {
            "B"
        } else {
            "·"
        };
        let quote = broker
            .latest_quote
            .as_ref()
            .map(|quote| format!("{:.*}", price_decimals, quote.mid))
            .unwrap_or_else(|| "--".to_owned());
        let availability = match broker.health.connection {
            ConnectionState::Disconnected => " DISCONNECTED",
            ConnectionState::Connecting => " CONNECTING",
            ConnectionState::Connected => match broker.health.data_freshness {
                FreshnessState::Live => "",
                FreshnessState::Stale => " STALE",
                FreshnessState::Unknown => " WARMING",
            },
        };
        let label = format!("{} {} {}{}", role, broker.name, quote, availability);
        let width = 10.0 + label.chars().count() as f32 * 7.0;
        if legend_x + width > plot_rect.right() - 4.0 {
            legend_x = rect.left() + 172.0;
            legend_y += 15.0;
        }
        if legend_y + 12.0 > plot_rect.top() {
            hidden_legends += 1;
            continue;
        }
        let base_color = broker_color_for(theme, *index);
        painter.line_segment(
            [
                Pos2::new(legend_x, legend_y + 6.0),
                Pos2::new(legend_x + 8.0, legend_y + 6.0),
            ],
            Stroke::new(
                if selected { 2.5_f32 } else { 1.0_f32 },
                if selected {
                    base_color
                } else {
                    dim_color(base_color, 130)
                },
            ),
        );
        painter.text(
            Pos2::new(legend_x + 11.0, legend_y),
            egui::Align2::LEFT_TOP,
            label,
            egui::FontId::monospace(if selected { 11.0 } else { 10.0 }),
            if selected {
                base_color
            } else {
                dim_color(base_color, 150)
            },
        );
        legend_x += width;
    }
    if hidden_legends > 0 {
        painter.text(
            Pos2::new(plot_rect.right() - 4.0, rect.top() + 21.0),
            egui::Align2::RIGHT_TOP,
            format!("+{} brokers", hidden_legends),
            egui::FontId::monospace(11.0),
            Color32::from_gray(180),
        );
    }

    // Draw background brokers first. The selected pair is drawn last with a
    // stronger stroke, so it remains readable where prices overlap.
    for (index, broker) in &ordered_brokers {
        let selected = broker.broker_id == selected_pair.0 || broker.broker_id == selected_pair.1;
        let color = broker_color_for(theme, *index);
        let line_color = if selected {
            color
        } else {
            dim_color(color, if is_live(broker) { 105 } else { 55 })
        };
        let stroke_width = if selected { 2.5_f32 } else { 1.0_f32 };
        let mut previous = None;
        for (sample_index, pt) in quote_points.iter().enumerate() {
            let current = pt
                .broker_mids
                .get(&broker.broker_id)
                .filter(|m| m.is_finite())
                .and_then(|&mid| {
                    x_axis_coordinate(
                        x_axis_mode,
                        plot_rect,
                        sample_index,
                        quote_points.len(),
                        pt.mono_ns,
                        now_mono,
                        visible_seconds,
                        visible_ticks,
                    )
                    .map(|x| Pos2::new(x, price_to_y(mid)))
                });
            if let (Some(a), Some(b)) = (previous, current) {
                painter.line_segment([a, b], Stroke::new(stroke_width, line_color));
            }
            previous = current;
        }
        if let Some(point) = previous {
            painter.circle_filled(point, if selected { 3.5 } else { 2.0 }, line_color);
        }
    }

    // Draw Broker Median (consensus_mid) as one continuous line.
    {
        let mut previous = None;
        for (index, pt) in quote_points.iter().enumerate() {
            let current = pt.consensus_mid.filter(|m| m.is_finite()).and_then(|mid| {
                x_axis_coordinate(
                    x_axis_mode,
                    plot_rect,
                    index,
                    quote_points.len(),
                    pt.mono_ns,
                    now_mono,
                    visible_seconds,
                    visible_ticks,
                )
                .map(|x| Pos2::new(x, price_to_y(mid)))
            });
            if let (Some(a), Some(b)) = (previous, current) {
                painter.line_segment(
                    [a, b],
                    Stroke::new(1.5_f32, dim_color(theme.median_line, 145)),
                );
            }
            previous = current;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn follow_anchor_limits_a_large_recenter_to_one_small_step() {
        let mut anchor = Some(100.0);
        let next = advance_chart_anchor(&mut anchor, 120.0, 10.0, 0.4);

        assert!((next - 101.2).abs() < f64::EPSILON);
        assert_eq!(anchor, Some(next));
    }
}
