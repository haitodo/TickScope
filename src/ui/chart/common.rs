use crate::core::models::BrokerOverview;
use crate::core::types::{ConnectionState, FreshnessState, MonoNs};
use super::quote_path::ChartXAxisMode;
use egui::{Color32, Pos2, Rect};

pub const CHART_HEADER_HEIGHT: f32 = 40.0;
pub const CHART_FOOTER_HEIGHT: f32 = 18.0;
pub const PRICE_AXIS_WIDTH: f32 = 88.0;

pub fn chart_plot_rect(rect: Rect, header_height: f32) -> Rect {
    let left = rect.left() + 2.0;
    let right = (rect.right() - PRICE_AXIS_WIDTH).max(left + 1.0);
    let top = (rect.top() + header_height).min(rect.bottom() - 1.0);
    let bottom = (rect.bottom() - CHART_FOOTER_HEIGHT).max(top + 1.0);
    Rect::from_min_max(Pos2::new(left, top), Pos2::new(right, bottom))
}

pub fn is_live(broker: &BrokerOverview) -> bool {
    broker.health.connection == ConnectionState::Connected
        && broker.health.data_freshness == FreshnessState::Live
}

pub fn format_price_delta(value: f64) -> String {
    format!("{value:+.5}")
}

pub fn fixed_time_window(now_mono: MonoNs, visible_seconds: u64) -> (u64, u64) {
    let span_ns = visible_seconds.max(1).saturating_mul(1_000_000_000);
    (now_mono.0.saturating_sub(span_ns), now_mono.0)
}

pub fn x_axis_coordinate(
    mode: ChartXAxisMode,
    rect: Rect,
    sample_index: usize,
    sample_count: usize,
    mono_ns: MonoNs,
    now_mono: MonoNs,
    visible_seconds: u64,
    visible_ticks: usize,
) -> Option<f32> {
    match mode {
        ChartXAxisMode::ReceiveTime => {
            let (start_ns, end_ns) = fixed_time_window(now_mono, visible_seconds);
            if mono_ns.0 < start_ns || mono_ns.0 > end_ns {
                return None;
            }
            let span_ns = end_ns.saturating_sub(start_ns).max(1);
            Some(
                rect.left()
                    + (mono_ns.0.saturating_sub(start_ns) as f64 / span_ns as f64) as f32
                        * rect.width(),
            )
        }
        ChartXAxisMode::TickCount => {
            let slots = visible_ticks.max(1);
            let first_visible = sample_count.saturating_sub(slots);
            if sample_index < first_visible {
                return None;
            }
            if slots == 1 {
                return Some(rect.right());
            }
            let shown_count = sample_count - first_visible;
            let slot_index = slots - shown_count + sample_index - first_visible;
            Some(rect.left() + slot_index as f32 / (slots - 1) as f32 * rect.width())
        }
    }
}

pub fn draw_x_axis_caption(
    painter: &egui::Painter,
    rect: Rect,
    mode: ChartXAxisMode,
    visible_seconds: u64,
    visible_ticks: usize,
) {
    let caption = match mode {
        ChartXAxisMode::ReceiveTime => {
            format!("Receive time · fixed {} s window", visible_seconds.max(1))
        }
        ChartXAxisMode::TickCount => format!("Tick count · fixed {} updates", visible_ticks.max(1)),
    };
    painter.text(
        Pos2::new(rect.left() + 6.0, rect.bottom() - 3.0),
        egui::Align2::LEFT_BOTTOM,
        caption,
        egui::FontId::monospace(11.0),
        Color32::from_gray(170),
    );
}

pub fn price_decimals_for_pip(pip_size: f64) -> usize {
    if !pip_size.is_finite() || pip_size <= 0.0 {
        return 3;
    }
    ((-pip_size.log10()).round() as i32 + 1).clamp(3, 6) as usize
}
