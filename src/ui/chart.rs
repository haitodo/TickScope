use crate::contracts::models::{CandleView, DiffPoint, Ohlc, SlotState};
use crate::contracts::types::BrokerId;
use egui::{Color32, Pos2, Rect, Stroke};


pub struct ChartTheme {
    pub bg_color: Color32,
    pub grid_color: Color32,
    pub candle_up_a: Color32,
    pub candle_down_a: Color32,
    pub candle_up_b: Color32,
    pub candle_down_b: Color32,
    pub diff_line: Color32,
    pub zero_line: Color32,
}

impl Default for ChartTheme {
    fn default() -> Self {
        Self {
            bg_color: Color32::from_rgb(20, 24, 30),
            grid_color: Color32::from_rgba_unmultiplied(255, 255, 255, 15),
            candle_up_a: Color32::from_rgb(0, 200, 100),     // Green for A bull
            candle_down_a: Color32::from_rgb(230, 60, 60),    // Red for A bear
            candle_up_b: Color32::from_rgb(30, 144, 255),    // Blue for B bull
            candle_down_b: Color32::from_rgb(255, 140, 0),   // Orange for B bear
            diff_line: Color32::from_rgb(255, 215, 0),       // Gold for diff
            zero_line: Color32::from_rgba_unmultiplied(255, 255, 255, 40),
        }
    }
}

pub fn draw_candlestick_chart(
    painter: &egui::Painter,
    rect: Rect,
    candle_view: Option<&CandleView>,
    broker_a: BrokerId,
    broker_b: BrokerId,
    fallback_price: Option<f64>,
    theme: &ChartTheme,
) {
    painter.rect_filled(rect, 4.0, theme.bg_color);

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

    let num_slots = view.slot_starts.len();
    let slot_width = rect.width() / (num_slots as f32).max(1.0);

    // Find global min and max prices across broker A and B
    let mut min_price = f64::MAX;
    let mut max_price = f64::MIN;

    let empty_vec = Vec::new();
    let slots_a = view.slots_by_broker.get(&broker_a).unwrap_or(&empty_vec);
    let slots_b = view.slots_by_broker.get(&broker_b).unwrap_or(&empty_vec);

    for s in slots_a.iter().chain(slots_b.iter()) {
        if let Some(ohlc) = &s.ohlc {
            min_price = min_price.min(ohlc.low);
            max_price = max_price.max(ohlc.high);
        }
    }

    let has_ohlc = min_price <= max_price && min_price < f64::MAX;

    let (chart_min, chart_max) = if has_ohlc {
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
    };

    let price_range = (chart_max - chart_min).max(0.0001);

    let price_to_y = |p: f64| -> f32 {
        let normalized = (chart_max - p) / price_range;
        rect.top() + (normalized as f32) * rect.height()
    };

    // Draw horizontal price grid lines
    let grid_steps = 4;
    for i in 0..=grid_steps {
        let p = chart_min + (price_range / grid_steps as f64) * i as f64;
        let y = price_to_y(p);
        painter.line_segment([Pos2::new(rect.left(), y), Pos2::new(rect.right(), y)], Stroke::new(1.0_f32, theme.grid_color));
        painter.text(
            Pos2::new(rect.right() - 4.0, y - 2.0),
            egui::Align2::RIGHT_BOTTOM,
            format!("{:.3}", p),
            egui::FontId::monospace(10.0),
            Color32::from_gray(120),
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

    // Draw candles for Broker A (left half of slot) and Broker B (right half of slot)
    let bar_width = (slot_width * 0.38).clamp(2.0, 16.0);

    for (i, _) in view.slot_starts.iter().enumerate() {
        let slot_center_x = rect.left() + (i as f32 + 0.5) * slot_width;

        // Broker A candle
        if let Some(s) = slots_a.get(i) {
            if s.state != SlotState::Empty {
                if let Some(ohlc) = &s.ohlc {
                    let cx = slot_center_x - bar_width * 0.6;
                    draw_single_candle(painter, cx, bar_width, ohlc, price_to_y, theme.candle_up_a, theme.candle_down_a);
                }
            }
        }

        // Broker B candle
        if let Some(s) = slots_b.get(i) {
            if s.state != SlotState::Empty {
                if let Some(ohlc) = &s.ohlc {
                    let cx = slot_center_x + bar_width * 0.6;
                    draw_single_candle(painter, cx, bar_width, ohlc, price_to_y, theme.candle_up_b, theme.candle_down_b);
                }
            }
        }
    }
}

fn draw_single_candle<F>(
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
    let y_open = price_to_y(ohlc.open);
    let y_close = price_to_y(ohlc.close);
    let y_high = price_to_y(ohlc.high);
    let y_low = price_to_y(ohlc.low);

    let is_up = ohlc.close >= ohlc.open;
    let color = if is_up { up_color } else { down_color };

    // Wick
    painter.line_segment([Pos2::new(cx, y_high), Pos2::new(cx, y_low)], Stroke::new(1.0_f32, color));

    // Body
    let top_body = y_open.min(y_close);
    let bottom_body = y_open.max(y_close).max(top_body + 1.0);
    let body_rect = Rect::from_min_max(
        Pos2::new(cx - bar_width * 0.5, top_body),
        Pos2::new(cx + bar_width * 0.5, bottom_body),
    );
    painter.rect_filled(body_rect, 1.0, color);
}

pub fn draw_difference_chart(
    painter: &egui::Painter,
    rect: Rect,
    series: &[DiffPoint],
    theme: &ChartTheme,
) {
    painter.rect_filled(rect, 4.0, theme.bg_color);

    if series.is_empty() {
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "No difference data yet",
            egui::FontId::proportional(13.0),
            Color32::GRAY,
        );
        return;
    }

    // Min / max diff
    let mut min_diff = f64::MAX;
    let mut max_diff = f64::MIN;
    for pt in series {
        min_diff = min_diff.min(pt.mid_diff);
        max_diff = max_diff.max(pt.mid_diff);
    }

    let extreme = min_diff.abs().max(max_diff.abs()).max(0.005);
    let chart_min = -extreme;
    let chart_max = extreme;
    let range = chart_max - chart_min;

    let diff_to_y = |d: f64| -> f32 {
        let norm = (chart_max - d) / range;
        rect.top() + (norm as f32) * rect.height()
    };

    // Zero line
    let y_zero = diff_to_y(0.0);
    painter.line_segment(
        [Pos2::new(rect.left(), y_zero), Pos2::new(rect.right(), y_zero)],
        Stroke::new(1.0_f32, theme.zero_line),
    );
    painter.text(
        Pos2::new(rect.left() + 4.0, y_zero - 2.0),
        egui::Align2::LEFT_BOTTOM,
        "0.000",
        egui::FontId::monospace(10.0),
        Color32::from_gray(100),
    );

    // Plot Mid difference line
    let count = series.len();
    let step_x = rect.width() / (count as f32).max(1.0);

    let mut points = Vec::with_capacity(count);
    for (i, pt) in series.iter().enumerate() {
        let x = rect.left() + (i as f32) * step_x;
        let y = diff_to_y(pt.mid_diff);
        points.push(Pos2::new(x, y));
    }

    if points.len() >= 2 {
        for w in points.windows(2) {
            painter.line_segment([w[0], w[1]], Stroke::new(1.5_f32, theme.diff_line));
        }
    }
}
