use super::common::{chart_plot_rect, draw_x_axis_caption, format_price_delta, x_axis_coordinate};
use super::quote_path::ChartXAxisMode;
use super::theme::ChartTheme;
use crate::core::models::{DiffPoint, PairComparison};
use crate::core::types::MonoNs;
use egui::{Color32, Pos2, Rect, Stroke};

#[derive(Debug, Clone)]
pub struct DifferenceHeader {
    pub text: String,
    pub is_current: bool,
}

/// Builds the value label for a difference chart without treating the last
/// retained history point as a live value. A missing current value means that
/// the selected pair cannot presently be compared (for example, one quote is
/// stale), even when the chart still has valid historical points to show.
pub fn difference_header<FCurrent, FLast>(
    comparison: Option<&PairComparison>,
    series: &[DiffPoint],
    now_mono: MonoNs,
    current_label: FCurrent,
    last_valid_label: FLast,
) -> DifferenceHeader
where
    FCurrent: Fn(&PairComparison) -> Option<String>,
    FLast: Fn(&DiffPoint) -> String,
{
    if let Some(text) = comparison.and_then(current_label) {
        return DifferenceHeader {
            text: format!("LIVE · {text}"),
            is_current: true,
        };
    }

    if let Some(last) = series.last() {
        let age_ms = now_mono.0.saturating_sub(last.mono_ns.0) / 1_000_000;
        let age = if age_ms < 1_000 {
            format!("{age_ms} ms ago")
        } else if age_ms < 60_000 {
            format!("{:.1} s ago", age_ms as f64 / 1_000.0)
        } else {
            format!("{} min ago", age_ms / 60_000)
        };
        return DifferenceHeader {
            text: format!(
                "Comparison unavailable · Last valid {} ({age})",
                last_valid_label(last)
            ),
            is_current: false,
        };
    }

    DifferenceHeader {
        text: "Comparison unavailable · No valid comparison yet".to_owned(),
        is_current: false,
    }
}

pub fn draw_difference_header(
    painter: &egui::Painter,
    plot_rect: Rect,
    rect: Rect,
    header: &DifferenceHeader,
    current_color: Color32,
) {
    painter.text(
        Pos2::new(plot_rect.right() - 4.0, rect.top() + 6.0),
        egui::Align2::RIGHT_TOP,
        &header.text,
        egui::FontId::monospace(if header.is_current { 12.0 } else { 11.0 }),
        if header.is_current {
            current_color
        } else {
            crate::ui::chart::theme::DIFF_HEADER
        },
    );
}

pub fn visible_diff_extreme<F>(
    series: &[DiffPoint],
    x_axis_mode: ChartXAxisMode,
    plot_rect: Rect,
    now_mono: MonoNs,
    visible_seconds: u64,
    visible_ticks: usize,
    minimum: f64,
    value: F,
) -> f64
where
    F: Fn(&DiffPoint) -> f64,
{
    series
        .iter()
        .enumerate()
        .filter(|(index, point)| {
            x_axis_coordinate(
                x_axis_mode,
                plot_rect,
                *index,
                series.len(),
                point.mono_ns,
                now_mono,
                visible_seconds,
                visible_ticks,
            )
            .is_some()
        })
        .map(|(_, point)| value(point).abs())
        .filter(|value| value.is_finite())
        .fold(minimum, f64::max)
}

pub fn draw_diff_scale(
    painter: &egui::Painter,
    rect: Rect,
    plot_rect: Rect,
    extreme: f64,
    theme: &ChartTheme,
) {
    for value in [extreme, 0.0, -extreme] {
        let y = plot_rect.top() + ((extreme - value) / (2.0 * extreme)) as f32 * plot_rect.height();
        let stroke = if value == 0.0 {
            Stroke::new(1.0_f32, theme.zero_line)
        } else {
            Stroke::new(1.0_f32, theme.grid_color)
        };
        painter.line_segment(
            [
                Pos2::new(plot_rect.left(), y),
                Pos2::new(plot_rect.right(), y),
            ],
            stroke,
        );
        let label = if value == 0.0 {
            "0.00000 price".to_owned()
        } else {
            format!("{} price", format_price_delta(value))
        };
        painter.text(
            Pos2::new(rect.right() - 4.0, y - 2.0),
            egui::Align2::RIGHT_CENTER,
            label,
            egui::FontId::monospace(11.0),
            crate::ui::style::TEXT_AXIS,
        );
    }
}

pub fn draw_mid_diff_chart(
    painter: &egui::Painter,
    rect: Rect,
    series: &[DiffPoint],
    comparison: Option<&PairComparison>,
    x_axis_mode: ChartXAxisMode,
    now_mono: MonoNs,
    visible_seconds: u64,
    visible_ticks: usize,
    theme: &ChartTheme,
) {
    painter.rect_filled(rect, 4.0, theme.bg_color);

    let header = difference_header(
        comparison,
        series,
        now_mono,
        |comparison| {
            comparison
                .mid_diff
                .map(|value| format!("Mid A - B: {} price", format_price_delta(value)))
        },
        |point| format!("Mid A - B: {} price", format_price_delta(point.mid_diff)),
    );

    if series.is_empty() {
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            &header.text,
            egui::FontId::proportional(13.0),
            crate::ui::chart::theme::DIFF_HEADER,
        );
        return;
    }

    let plot_rect = chart_plot_rect(rect, 26.0);
    let extreme = visible_diff_extreme(
        series,
        x_axis_mode,
        plot_rect,
        now_mono,
        visible_seconds,
        visible_ticks,
        0.005,
        |point| point.mid_diff,
    );
    let chart_min = -extreme;
    let chart_max = extreme;
    let range = chart_max - chart_min;

    let diff_to_y = |d: f64| -> f32 {
        let norm = (chart_max - d) / range;
        plot_rect.top() + (norm as f32) * plot_rect.height()
    };
    draw_diff_scale(painter, rect, plot_rect, extreme, theme);

    draw_difference_header(painter, plot_rect, rect, &header, theme.diff_line);

    draw_x_axis_caption(painter, rect, x_axis_mode, visible_seconds, visible_ticks);
    let mut previous = None;
    for (index, pt) in series.iter().enumerate() {
        if let Some(x) = x_axis_coordinate(
            x_axis_mode,
            plot_rect,
            index,
            series.len(),
            pt.mono_ns,
            now_mono,
            visible_seconds,
            visible_ticks,
        ) {
            let position = Pos2::new(x, diff_to_y(pt.mid_diff));
            if let Some(previous) = previous {
                painter.line_segment([previous, position], Stroke::new(1.5_f32, theme.diff_line));
            }
            previous = Some(position);
        }
    }
}

pub fn draw_bid_ask_diff_chart(
    painter: &egui::Painter,
    rect: Rect,
    series: &[DiffPoint],
    comparison: Option<&PairComparison>,
    x_axis_mode: ChartXAxisMode,
    now_mono: MonoNs,
    visible_seconds: u64,
    visible_ticks: usize,
    theme: &ChartTheme,
) {
    painter.rect_filled(rect, 4.0, theme.bg_color);

    let header = difference_header(
        comparison,
        series,
        now_mono,
        |comparison| match (comparison.bid_diff, comparison.ask_diff) {
            (Some(bid), Some(ask)) => Some(format!(
                "Bid A - B: {}  Ask A - B: {} price",
                format_price_delta(bid),
                format_price_delta(ask)
            )),
            _ => None,
        },
        |point| {
            format!(
                "Bid A - B: {}  Ask A - B: {} price",
                format_price_delta(point.bid_diff),
                format_price_delta(point.ask_diff)
            )
        },
    );

    if series.is_empty() {
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            &header.text,
            egui::FontId::proportional(13.0),
            crate::ui::chart::theme::DIFF_HEADER,
        );
        return;
    }

    let plot_rect = chart_plot_rect(rect, 26.0);
    let extreme = visible_diff_extreme(
        series,
        x_axis_mode,
        plot_rect,
        now_mono,
        visible_seconds,
        visible_ticks,
        0.005,
        |point| point.bid_diff.abs().max(point.ask_diff.abs()),
    );
    let chart_min = -extreme;
    let chart_max = extreme;
    let range = chart_max - chart_min;

    let diff_to_y = |d: f64| -> f32 {
        let norm = (chart_max - d) / range;
        plot_rect.top() + (norm as f32) * plot_rect.height()
    };
    draw_diff_scale(painter, rect, plot_rect, extreme, theme);

    draw_difference_header(painter, plot_rect, rect, &header, Color32::WHITE);

    draw_x_axis_caption(painter, rect, x_axis_mode, visible_seconds, visible_ticks);
    let mut previous_bid = None;
    let mut previous_ask = None;
    for (index, pt) in series.iter().enumerate() {
        if let Some(x) = x_axis_coordinate(
            x_axis_mode,
            plot_rect,
            index,
            series.len(),
            pt.mono_ns,
            now_mono,
            visible_seconds,
            visible_ticks,
        ) {
            let position = Pos2::new(x, diff_to_y(pt.bid_diff));
            if let Some(previous) = previous_bid {
                painter.line_segment(
                    [previous, position],
                    Stroke::new(1.5_f32, theme.bid_diff_line),
                );
            }
            previous_bid = Some(position);
        }
        if let Some(x) = x_axis_coordinate(
            x_axis_mode,
            plot_rect,
            index,
            series.len(),
            pt.mono_ns,
            now_mono,
            visible_seconds,
            visible_ticks,
        ) {
            let position = Pos2::new(x, diff_to_y(pt.ask_diff));
            if let Some(previous) = previous_ask {
                painter.line_segment(
                    [previous, position],
                    Stroke::new(1.5_f32, theme.ask_diff_line),
                );
            }
            previous_ask = Some(position);
        }
    }
}

pub fn draw_spread_diff_chart(
    painter: &egui::Painter,
    rect: Rect,
    series: &[DiffPoint],
    comparison: Option<&PairComparison>,
    x_axis_mode: ChartXAxisMode,
    now_mono: MonoNs,
    visible_seconds: u64,
    visible_ticks: usize,
    theme: &ChartTheme,
) {
    painter.rect_filled(rect, 4.0, theme.bg_color);

    let spread_status = |value: f64, tense: &str| {
        if value > 0.0001 {
            format!("(A {tense} wider)")
        } else if value < -0.0001 {
            format!("(B {tense} wider)")
        } else {
            "(Equal)".to_owned()
        }
    };
    let header = difference_header(
        comparison,
        series,
        now_mono,
        |comparison| {
            comparison.spread_diff.map(|value| {
                format!(
                    "Spread A - B: {} price {}",
                    format_price_delta(value),
                    spread_status(value, "is")
                )
            })
        },
        |point| {
            format!(
                "Spread A - B: {} price {}",
                format_price_delta(point.spread_diff),
                spread_status(point.spread_diff, "was")
            )
        },
    );

    if series.is_empty() {
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            &header.text,
            egui::FontId::proportional(13.0),
            crate::ui::chart::theme::DIFF_HEADER,
        );
        return;
    }

    let plot_rect = chart_plot_rect(rect, 26.0);
    let extreme = visible_diff_extreme(
        series,
        x_axis_mode,
        plot_rect,
        now_mono,
        visible_seconds,
        visible_ticks,
        0.002,
        |point| point.spread_diff,
    );
    let chart_min = -extreme;
    let chart_max = extreme;
    let range = chart_max - chart_min;

    let diff_to_y = |d: f64| -> f32 {
        let norm = (chart_max - d) / range;
        plot_rect.top() + (norm as f32) * plot_rect.height()
    };
    draw_diff_scale(painter, rect, plot_rect, extreme, theme);

    draw_difference_header(painter, plot_rect, rect, &header, theme.spread_diff_line);

    draw_x_axis_caption(painter, rect, x_axis_mode, visible_seconds, visible_ticks);
    let mut previous = None;
    for (index, pt) in series.iter().enumerate() {
        if let Some(x) = x_axis_coordinate(
            x_axis_mode,
            plot_rect,
            index,
            series.len(),
            pt.mono_ns,
            now_mono,
            visible_seconds,
            visible_ticks,
        ) {
            let position = Pos2::new(x, diff_to_y(pt.spread_diff));
            if let Some(previous) = previous {
                painter.line_segment(
                    [previous, position],
                    Stroke::new(1.5_f32, theme.spread_diff_line),
                );
            }
            previous = Some(position);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn visible_difference_scale_ignores_points_outside_the_time_window() {
        let series = [
            DiffPoint {
                mono_ns: MonoNs(1),
                bid_diff: 5.0,
                ask_diff: 5.0,
                mid_diff: 5.0,
                spread_diff: 5.0,
            },
            DiffPoint {
                mono_ns: MonoNs(60_000_000_000),
                bid_diff: 0.01,
                ask_diff: 0.01,
                mid_diff: 0.01,
                spread_diff: 0.01,
            },
        ];
        let plot_rect = Rect::from_min_size(Pos2::ZERO, egui::Vec2::new(400.0, 200.0));
        let extreme = visible_diff_extreme(
            &series,
            ChartXAxisMode::ReceiveTime,
            plot_rect,
            MonoNs(60_000_000_000),
            10,
            100,
            0.005,
            |point| point.mid_diff,
        );

        assert_eq!(extreme, 0.01);
    }

    #[test]
    fn difference_header_marks_retained_values_as_last_valid_when_live_diff_is_missing() {
        let comparison = PairComparison {
            broker_a: 1,
            broker_b: 2,
            as_of_mono_ns: MonoNs(3_500_000_000),
            bid_diff: None,
            ask_diff: None,
            mid_diff: None,
            spread_diff: None,
            recent_diff_series: Vec::new(),
            latest_match: None,
            ema_lead_lag_ms: None,
        };
        let series = [DiffPoint {
            mono_ns: MonoNs(2_000_000_000),
            bid_diff: 0.002,
            ask_diff: 0.002,
            mid_diff: 0.002,
            spread_diff: 0.0,
        }];

        let header = difference_header(
            Some(&comparison),
            &series,
            MonoNs(3_500_000_000),
            |comparison| {
                comparison
                    .mid_diff
                    .map(|value| format!("Mid A - B: {} price", format_price_delta(value)))
            },
            |point| format!("Mid A - B: {} price", format_price_delta(point.mid_diff)),
        );

        assert!(!header.is_current);
        assert_eq!(
            header.text,
            "Comparison unavailable · Last valid Mid A - B: +0.00200 price (1.5 s ago)"
        );
    }

    #[test]
    fn difference_header_uses_live_pair_value_instead_of_history() {
        let comparison = PairComparison {
            broker_a: 1,
            broker_b: 2,
            as_of_mono_ns: MonoNs(3_500_000_000),
            bid_diff: Some(0.004),
            ask_diff: Some(0.004),
            mid_diff: Some(0.004),
            spread_diff: Some(0.0),
            recent_diff_series: Vec::new(),
            latest_match: None,
            ema_lead_lag_ms: None,
        };
        let series = [DiffPoint {
            mono_ns: MonoNs(2_000_000_000),
            bid_diff: 0.002,
            ask_diff: 0.002,
            mid_diff: 0.002,
            spread_diff: 0.0,
        }];

        let header = difference_header(
            Some(&comparison),
            &series,
            MonoNs(3_500_000_000),
            |comparison| {
                comparison
                    .mid_diff
                    .map(|value| format!("Mid A - B: {} price", format_price_delta(value)))
            },
            |point| format!("Mid A - B: {} price", format_price_delta(point.mid_diff)),
        );

        assert!(header.is_current);
        assert_eq!(header.text, "LIVE · Mid A - B: +0.00400 price");
    }
}
