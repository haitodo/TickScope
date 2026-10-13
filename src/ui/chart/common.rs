use super::quote_path::ChartXAxisMode;
use crate::core::models::BrokerOverview;
use crate::core::types::{BrokerId, ConnectionState, FreshnessState, MonoNs, UtcMs};
use crate::ui::settings::ChartTimeDisplayMode;
use egui::{Color32, Pos2, Rect, Stroke};

pub const CHART_HEADER_HEIGHT: f32 = 40.0;
pub const CHART_FOOTER_HEIGHT: f32 = 18.0;
pub const PRICE_AXIS_WIDTH: f32 = 88.0;

#[must_use]
pub fn chart_plot_rect(rect: Rect, header_height: f32) -> Rect {
    let left = rect.left() + 2.0;
    let right = (rect.right() - PRICE_AXIS_WIDTH).max(left + 1.0);
    let top = (rect.top() + header_height).min(rect.bottom() - 1.0);
    let bottom = (rect.bottom() - CHART_FOOTER_HEIGHT).max(top + 1.0);
    Rect::from_min_max(Pos2::new(left, top), Pos2::new(right, bottom))
}

#[must_use]
pub fn is_live(broker: &BrokerOverview) -> bool {
    broker.health.connection == ConnectionState::Connected
        && broker.health.data_freshness == FreshnessState::Live
}

#[must_use]
pub fn format_price_delta(value: f64) -> String {
    format!("{value:+.5}")
}

#[must_use]
pub fn fixed_time_window(now_mono: MonoNs, visible_seconds: u64) -> (u64, u64) {
    let span_ns = visible_seconds.max(1).saturating_mul(1_000_000_000);
    (now_mono.0.saturating_sub(span_ns), now_mono.0)
}

#[must_use]
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
        crate::ui::style::TEXT_CAPTION,
    );
}

#[must_use]
pub fn price_decimals_for_pip(pip_size: f64) -> usize {
    if !pip_size.is_finite() || pip_size <= 0.0 {
        return 3;
    }
    ((-pip_size.log10()).round() as i32 + 1).clamp(3, 6) as usize
}

/// Resolves the timezone offset in milliseconds and mode label for chart clock and timestamps.
///
/// Rules:
/// - Realtime trade mode (`!is_replay`): Always fixed to JST (UTC+9, 32400s).
/// - Replay verification mode (`is_replay`):
///   - `ChartTimeDisplayMode::Jst`: Fixed to JST (UTC+9).
///   - `ChartTimeDisplayMode::Server`: Broker's server time based on `active_utc_offset_sec` (e.g. UTC+2/UTC+3).
#[must_use]
pub fn resolve_chart_offset_ms(
    mode: ChartTimeDisplayMode,
    is_replay: bool,
    broker_id: BrokerId,
    broker_overviews: &[BrokerOverview],
) -> (i64, &'static str) {
    if !is_replay || mode == ChartTimeDisplayMode::Jst {
        (9 * 3600 * 1000, "JST")
    } else {
        let offset_sec = broker_overviews
            .iter()
            .find(|b| b.broker_id == broker_id)
            .map(|b| b.active_utc_offset_sec)
            .or_else(|| broker_overviews.first().map(|b| b.active_utc_offset_sec))
            .unwrap_or(7200);
        (i64::from(offset_sec) * 1000, "SERVER")
    }
}

/// Draws an interactive clock and 00-second progress badge at the bottom-right corner of the chart.
///
/// Features:
/// - Realtime / replay current time in HH:MM:SS format (clean digits without J/U suffix)
/// - Progress bar indicating progress towards next :00 second
/// - Visual warning tint at 55-59s, flash on :00s
/// - Realtime: Fixed to JST (no toggling)
/// - Replay: Clickable to toggle between JST and SERVER time
pub fn draw_chart_clock_badge(
    ui: Option<&mut egui::Ui>,
    painter: &egui::Painter,
    plot_rect: Rect,
    chart_rect: Rect,
    display_now_utc: UtcMs,
    is_replay: bool,
    time_mode: &mut ChartTimeDisplayMode,
    target_broker: BrokerId,
    broker_overviews: &[BrokerOverview],
) {
    let axis_left = plot_rect.right();
    let axis_right = chart_rect.right();
    if axis_right <= axis_left + 24.0 {
        return;
    }

    let badge_left = axis_left + 2.0;
    let badge_right = axis_right - 2.0;
    let badge_top = plot_rect.bottom() + 1.0;
    let badge_bottom = chart_rect.bottom() - 1.0;
    if badge_bottom <= badge_top {
        return;
    }
    let badge_rect = Rect::from_min_max(
        Pos2::new(badge_left, badge_top),
        Pos2::new(badge_right, badge_bottom),
    );

    let display_ms = display_now_utc.0;
    let now_utc_ms = if display_ms > 0 {
        display_ms
    } else {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0)
    };

    let (offset_ms, mode_label) = resolve_chart_offset_ms(*time_mode, is_replay, target_broker, broker_overviews);
    let local_ms = now_utc_ms + offset_ms;
    let ms_of_day = local_ms.rem_euclid(86_400_000);
    let hrs = ms_of_day / 3_600_000;
    let mins = (ms_of_day % 3_600_000) / 60_000;
    let secs = (ms_of_day % 60_000) / 1_000;
    let millis = ms_of_day % 1_000;

    let progress = (secs as f32 + millis as f32 / 1000.0) / 60.0;
    let is_flash = secs == 0 && millis < 800;
    let is_warning = secs >= 55;

    let (bg_color, stroke_color, bar_color, text_color) = if is_flash {
        (
            Color32::from_rgb(18, 55, 32),
            Color32::from_rgb(70, 230, 130),
            Color32::from_rgb(70, 230, 130),
            Color32::from_rgb(220, 255, 230),
        )
    } else if is_warning {
        (
            Color32::from_rgb(45, 35, 12),
            Color32::from_rgb(230, 180, 50),
            Color32::from_rgb(230, 180, 50),
            Color32::from_rgb(255, 235, 150),
        )
    } else {
        (
            Color32::from_rgb(16, 22, 32),
            Color32::from_rgb(45, 60, 85),
            Color32::from_rgb(60, 110, 180),
            Color32::from_rgb(220, 235, 255),
        )
    };

    painter.rect_filled(badge_rect, 2.0, bg_color);
    painter.rect_stroke(badge_rect, 2.0, Stroke::new(1.0_f32, stroke_color));

    let bar_w = (badge_rect.width() * progress).clamp(0.0, badge_rect.width());
    if bar_w > 0.0 {
        let bar_rect = Rect::from_min_max(
            Pos2::new(badge_rect.left(), badge_rect.bottom() - 2.0),
            Pos2::new(badge_rect.left() + bar_w, badge_rect.bottom()),
        );
        painter.rect_filled(bar_rect, 0.0, bar_color);
    }

    let time_text = format!("{hrs:02}:{mins:02}:{secs:02}");
    painter.text(
        badge_rect.center() - egui::vec2(0.0, 1.0),
        egui::Align2::CENTER_CENTER,
        time_text,
        egui::FontId::monospace(10.5),
        text_color,
    );

    if let Some(ui) = ui {
        let response = ui.interact(
            badge_rect,
            egui::Id::new("chart_clock_badge_interactive"),
            egui::Sense::click(),
        );
        if is_replay && response.clicked() {
            *time_mode = match *time_mode {
                ChartTimeDisplayMode::Jst => ChartTimeDisplayMode::Server,
                ChartTimeDisplayMode::Server => ChartTimeDisplayMode::Jst,
            };
        }
        let remaining = 60 - secs;
        let hover_text = if is_replay {
            let next_mode = match *time_mode {
                ChartTimeDisplayMode::Jst => "SERVER時間 (MT5時間)",
                ChartTimeDisplayMode::Server => "JST (日本時間)",
            };
            format!(
                "リプレイ時刻: {hrs:02}:{mins:02}:{secs:02}.{millis:03} ({mode_label})\n[クリック] {next_mode} に切替\n次の00秒まで: あと {remaining} 秒\n※55〜59秒で黄色に予告、00秒ジャストで緑にフラッシュします"
            )
        } else {
            format!(
                "現在時刻: {hrs:02}:{mins:02}:{secs:02}.{millis:03} (JST)\n次の00秒まで: あと {remaining} 秒\n※55〜59秒で黄色に予告、00秒ジャストで緑にフラッシュします"
            )
        };
        response.on_hover_text(hover_text);
    }
}

