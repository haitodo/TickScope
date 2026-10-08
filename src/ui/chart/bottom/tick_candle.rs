//! Noise-cancelled tick candlestick chart (0.2p zigzag threshold cancellation).
//!
//! Inspired by MQL5 `replay-TickCandle.mq5`, this module aggregates raw price updates
//! into high-definition, noise-filtered candlesticks, ignoring sub-threshold price vibration
//! and canceling bidirectional noise within the threshold.

use super::super::common::{
    chart_plot_rect, price_decimals_for_pip, CHART_HEADER_HEIGHT,
};
use super::super::theme::ChartTheme;
use egui::{Color32, FontId, Pos2, Rect, Stroke};

pub use crate::core::models::TickCandleBar;

/// Generate noise-cancelled tick candles from a sequence of raw `(price, time_sec)` tuples.
///
/// Mimics the 0.2pips zigzag cancellation logic from `replay-TickCandle.mq5`:
/// 1. Price changes below epsilon (0.1 point = 0.01 pip) are skipped.
/// 2. If the current price move is opposite and equal in size (within `threshold_pips`)
///    to the previous unconfirmed candle's move, it cancels that move and reuses the candle.
/// 3. Candles are bounded to at most `max_candles`.
pub fn generate_tick_candles(
    ticks: &[(f64, i64)],
    pip_size: f64,
    threshold_pips: f64,
    max_candles: usize,
) -> Vec<TickCandleBar> {
    if ticks.is_empty() || pip_size <= 0.0 || max_candles == 0 {
        return Vec::new();
    }

    let epsilon = (pip_size * 0.01).max(1e-9);
    let mut candles: Vec<TickCandleBar> = Vec::with_capacity(max_candles);

    let mut last_price = 0.0;
    let mut has_unconfirmed = false;
    let mut is_reusing_candle = false;

    for &(price, time_sec) in ticks {
        if price <= 0.0 || price.is_nan() {
            continue;
        }

        if last_price == 0.0 {
            last_price = price;
            continue;
        }

        let diff = price - last_price;
        if diff.abs() < epsilon {
            continue;
        }

        let diff_pips = diff / pip_size;
        let abs_diff_pips = diff_pips.abs();

        if has_unconfirmed && !candles.is_empty() && !is_reusing_candle {
            let last_idx = candles.len() - 1;
            let prev_diff = candles[last_idx].close - candles[last_idx].open;
            let prev_diff_pips = prev_diff / pip_size;

            let is_opposite = prev_diff_pips * diff_pips < 0.0;
            let is_same_size = (prev_diff_pips.abs() - abs_diff_pips).abs() < 1e-4;
            let is_within_threshold = abs_diff_pips <= threshold_pips + 1e-4;

            if is_opposite && is_same_size && is_within_threshold {
                // Cancel out bidirectional noise: restore previous open and mark for reuse
                let open = candles[last_idx].open;
                candles[last_idx].close = open;
                candles[last_idx].high = open;
                candles[last_idx].low = open;
                candles[last_idx].is_confirmed = false;

                last_price = open;
                is_reusing_candle = true;
                continue;
            } else {
                candles[last_idx].is_confirmed = true;
            }
        }

        if is_reusing_candle && !candles.is_empty() {
            let target_idx = candles.len() - 1;
            candles[target_idx].close = price;
            candles[target_idx].high = candles[target_idx].high.max(price);
            candles[target_idx].low = candles[target_idx].low.min(price);
            candles[target_idx].time_sec = time_sec;

            if target_idx > 0 {
                let prev_time = candles[target_idx - 1].time_sec;
                candles[target_idx].is_minute_changed = time_sec / 60 != prev_time / 60;
            }
            is_reusing_candle = false;
        } else {
            if candles.len() >= max_candles {
                candles.remove(0);
            }

            let prev_minute = candles.last().map(|c| c.time_sec / 60);
            let is_minute_changed = prev_minute.map_or(false, |pm| time_sec / 60 != pm);

            candles.push(TickCandleBar {
                open: last_price,
                high: last_price.max(price),
                low: last_price.min(price),
                close: price,
                time_sec,
                is_minute_changed,
                is_confirmed: false,
            });
        }

        has_unconfirmed = true;
        last_price = price;
    }

    candles
}

/// Draw the noise-cancelled tick candlestick chart.
pub fn draw_tick_candle_view(
    painter: &egui::Painter,
    rect: Rect,
    candles: &[TickCandleBar],
    broker_name: &str,
    pip_size: f64,
    bar_width: f32,
    show_current_price: bool,
    theme: &ChartTheme,
    pointer_pos: Option<Pos2>,
) {
    painter.rect_filled(rect, 4.0, theme.bg_color);

    if candles.is_empty() {
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            format!("Waiting for tick data for {broker_name}..."),
            FontId::proportional(14.0),
            Color32::GRAY,
        );
        return;
    }

    let plot_rect = chart_plot_rect(rect, CHART_HEADER_HEIGHT);
    let pip_size = pip_size.max(1e-9);
    let price_prec = price_decimals_for_pip(pip_size);

    // Layout configuration (supports slim bars down to 1.0px for dense tick candle flows)
    let candle_width = bar_width.clamp(1.0, 15.0);
    let candle_spacing = if candle_width <= 2.0 {
        1.0_f32
    } else {
        (candle_width * 0.4).clamp(1.5, 4.0)
    };
    let rounding = if candle_width <= 2.0 { 0.0 } else { 1.0 };
    let step_x = candle_width + candle_spacing;
    let right_edge = plot_rect.right() - 8.0;

    // Determine candles visible inside the plot viewport (right-to-left)
    let max_visible_bars = if step_x > 0.0 {
        let available_w = (right_edge - (plot_rect.left() - candle_width)).max(0.0);
        ((available_w / step_x).ceil() as usize + 1).min(candles.len())
    } else {
        candles.len()
    };
    let visible_candles = &candles[candles.len().saturating_sub(max_visible_bars)..];

    // Compute price range strictly across visible candles for responsive auto-fit
    let mut min_price = f64::INFINITY;
    let mut max_price = f64::NEG_INFINITY;
    for c in visible_candles {
        min_price = min_price.min(c.low);
        max_price = max_price.max(c.high);
    }

    if !min_price.is_finite() || !max_price.is_finite() || min_price >= max_price {
        if let Some(first) = visible_candles.first().or_else(|| candles.last()) {
            min_price = first.close - 0.5 * pip_size;
            max_price = first.close + 0.5 * pip_size;
        } else {
            return;
        }
    }

    // Dynamic padding optimized for 0.2p noise-cancelled tick candles:
    // Scale tightly to visible swings without leaving excessive empty margins at top/bottom.
    let raw_span = max_price - min_price;
    let effective_span = raw_span.max(0.4 * pip_size);
    let padding = (effective_span * 0.08).max(0.15 * pip_size);
    let price_min = min_price - padding;
    let price_max = max_price + padding;
    let price_span = (price_max - price_min).max(1e-9);

    let price_to_y = |p: f64| -> f32 {
        let norm = (price_max - p) / price_span;
        plot_rect.top() + (norm as f32) * plot_rect.height()
    };

    // Precompute candle count per completed 1-minute block
    let mut block_counts = Vec::new();
    let mut current_block_len = 0usize;
    for c in candles {
        if c.is_minute_changed && current_block_len > 0 {
            block_counts.push(current_block_len);
            current_block_len = 1;
        } else {
            current_block_len += 1;
        }
    }
    let ongoing_block_count = current_block_len;

    // Hover detection
    let mut hovered_candle: Option<(&TickCandleBar, f32)> = None;
    if let Some(pos) = pointer_pos {
        if plot_rect.contains(pos) {
            for (i_from_end, candle) in candles.iter().rev().enumerate() {
                let center_x = right_edge - (i_from_end as f32) * step_x;
                if center_x < plot_rect.left() - candle_width {
                    break;
                }
                if (pos.x - center_x).abs() <= step_x * 0.5 {
                    hovered_candle = Some((candle, center_x));
                    break;
                }
            }
        }
    }

    // Header info (interactive on hover)
    let latest = candles.last().unwrap();
    let header_text = if let Some((hc, _)) = hovered_candle {
        let h_day_sec = hc.time_sec.rem_euclid(86400);
        let h_hrs = h_day_sec / 3600;
        let h_mins = (h_day_sec % 3600) / 60;
        let h_secs = h_day_sec % 60;
        let h_diff = (hc.close - hc.open) / pip_size;
        format!(
            "{} | HOVER [{:02}:{:02}:{:02}] | O: {:.prec$} H: {:.prec$} L: {:.prec$} C: {:.prec$} ({diff:+.1}p)",
            broker_name,
            h_hrs, h_mins, h_secs,
            hc.open, hc.high, hc.low, hc.close,
            diff = h_diff,
            prec = price_prec,
        )
    } else {
        let net_change = latest.close - latest.open;
        let net_pips = net_change / pip_size;
        format!(
            "{} | Tick Candle [0.2p] | Visible: {}/{} bars | Curr 1m: {} bars | Close: {:.prec$} ({net_pips:+.1}p)",
            broker_name,
            visible_candles.len(),
            candles.len(),
            ongoing_block_count,
            latest.close,
            net_pips = net_pips,
            prec = price_prec,
        )
    };

    let header_color = if hovered_candle.is_some() {
        Color32::from_rgb(255, 215, 100) // Gold when inspecting
    } else {
        Color32::from_rgb(220, 230, 245)
    };
    painter.text(
        Pos2::new(plot_rect.left() + 4.0, rect.top() + 12.0),
        egui::Align2::LEFT_CENTER,
        header_text,
        FontId::monospace(12.0),
        header_color,
    );

    // Horizontal grid lines & price labels on right margin
    let num_grid_lines = 4;
    for i in 0..=num_grid_lines {
        let ratio = i as f64 / num_grid_lines as f64;
        let p = price_min + ratio * price_span;
        let y = price_to_y(p);

        painter.line_segment(
            [Pos2::new(plot_rect.left(), y), Pos2::new(plot_rect.right(), y)],
            Stroke::new(0.5_f32, theme.grid_color),
        );

        painter.text(
            Pos2::new(plot_rect.right() + 6.0, y),
            egui::Align2::LEFT_CENTER,
            format!("{p:.prec$}", prec = price_prec),
            FontId::monospace(10.0),
            Color32::from_rgb(140, 155, 175),
        );
    }

    let bull_color = Color32::from_rgb(0, 211, 126); // Emerald Green
    let bear_color = Color32::from_rgb(239, 83, 80);  // Coral Red
    let doji_color = Color32::from_rgb(180, 180, 190);

    // Render candles and 1-minute boundary lines from right to left
    for (i_from_end, candle) in candles.iter().rev().enumerate() {
        let center_x = right_edge - (i_from_end as f32) * step_x;
        if center_x < plot_rect.left() - candle_width {
            break; // Past left boundary
        }

        // 1-minute boundary line & timestamp caption
        // Positioned cleanly in the gap BETWEEN the previous candle and the new minute candle
        if candle.is_minute_changed {
            let line_x = center_x - step_x * 0.5;
            if line_x >= plot_rect.left() && line_x <= plot_rect.right() {
                let day_sec = candle.time_sec.rem_euclid(86400);
                let hrs = day_sec / 3600;
                let mins = (day_sec % 3600) / 60;
                let is_hour_start = mins == 0;

                let line_stroke = if is_hour_start {
                    Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(140, 200, 255, 120))
                } else {
                    Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(80, 140, 220, 60))
                };

                // Vertical delimiter line (in the gap, never piercing through candle bodies)
                painter.line_segment(
                    [Pos2::new(line_x, plot_rect.top()), Pos2::new(line_x, plot_rect.bottom())],
                    line_stroke,
                );

                // Time label in footer (HH:MM on hour/5m marks, :MM elsewhere to prevent crowding)
                let time_str = if is_hour_start || mins % 5 == 0 {
                    format!("{hrs:02}:{mins:02}")
                } else {
                    format!(":{mins:02}")
                };
                let label_color = if is_hour_start {
                    Color32::from_rgb(180, 220, 255)
                } else {
                    Color32::from_rgb(100, 160, 220)
                };
                painter.text(
                    Pos2::new(line_x, plot_rect.bottom() + 10.0),
                    egui::Align2::CENTER_CENTER,
                    time_str,
                    FontId::monospace(9.0),
                    label_color,
                );

                // Volatility / Activity badge at top of the boundary (number of bars formed in completed minute)
                if let Some(bars_in_min) = block_counts.pop() {
                    let badge_color = if bars_in_min >= 20 {
                        Color32::from_rgb(255, 190, 60) // High Volatility (Gold)
                    } else if bars_in_min >= 10 {
                        Color32::from_rgb(140, 210, 160) // Active (Green)
                    } else {
                        Color32::from_rgb(110, 145, 185) // Moderate (Muted)
                    };
                    painter.text(
                        Pos2::new(line_x, plot_rect.top() + 8.0),
                        egui::Align2::CENTER_CENTER,
                        format!("{bars_in_min}b"),
                        FontId::monospace(8.5),
                        badge_color,
                    );
                }
            }
        }

        let y_open = price_to_y(candle.open);
        let y_close = price_to_y(candle.close);

        let body_color = if candle.is_bull() {
            bull_color
        } else if candle.is_bear() {
            bear_color
        } else {
            doji_color
        };

        // Body (Wick omitted for clean high-density tick candle display)
        let top_body = y_open.min(y_close);
        let bottom_body = y_open.max(y_close);
        let half_w = candle_width * 0.5;

        if (bottom_body - top_body).abs() < 1.0 {
            // Doji flat line
            painter.line_segment(
                [
                    Pos2::new(center_x - half_w, y_open),
                    Pos2::new(center_x + half_w, y_open),
                ],
                Stroke::new(1.0_f32, body_color),
            );
        } else {
            let body_rect = Rect::from_min_max(
                Pos2::new(center_x - half_w, top_body),
                Pos2::new(center_x + half_w, bottom_body),
            );
            painter.rect_filled(body_rect, rounding, body_color);
        }
    }

    // Hover crosshair and price inspection
    if let (Some((_, h_cx)), Some(pos)) = (hovered_candle, pointer_pos) {
        if plot_rect.contains(pos) {
            // Vertical crosshair at hovered candle
            painter.line_segment(
                [Pos2::new(h_cx, plot_rect.top()), Pos2::new(h_cx, plot_rect.bottom())],
                Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(180, 210, 255, 90)),
            );
            // Horizontal crosshair at pointer Y
            painter.line_segment(
                [Pos2::new(plot_rect.left(), pos.y), Pos2::new(plot_rect.right(), pos.y)],
                Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(180, 210, 255, 70)),
            );
            // Hovered price badge on Y axis
            let norm_y = (pos.y - plot_rect.top()) / plot_rect.height();
            let hover_price = price_max - (norm_y as f64) * price_span;
            let hover_badge_rect = Rect::from_center_size(
                Pos2::new(plot_rect.right() + 32.0, pos.y),
                egui::Vec2::new(54.0, 14.0),
            );
            painter.rect_filled(hover_badge_rect, 2.0, Color32::from_rgb(30, 45, 65));
            painter.rect_stroke(
                hover_badge_rect,
                2.0,
                Stroke::new(1.0_f32, Color32::from_rgb(100, 180, 255)),
            );
            painter.text(
                hover_badge_rect.center(),
                egui::Align2::CENTER_CENTER,
                format!("{hover_price:.prec$}", prec = price_prec),
                FontId::monospace(10.0),
                Color32::from_rgb(180, 220, 255),
            );
        }
    }

    // Current price dashed line and axis badge
    if show_current_price {
        let cur_y = price_to_y(latest.close);
        painter.line_segment(
            [
                Pos2::new(plot_rect.left(), cur_y),
                Pos2::new(plot_rect.right() + 4.0, cur_y),
            ],
            Stroke::new(
                1.0_f32,
                Color32::from_rgba_unmultiplied(255, 215, 0, 180), // Gold
            ),
        );

        let badge_rect = Rect::from_center_size(
            Pos2::new(plot_rect.right() + 32.0, cur_y),
            egui::Vec2::new(54.0, 14.0),
        );
        painter.rect_filled(badge_rect, 2.0, Color32::from_rgb(45, 55, 75));
        painter.rect_stroke(
            badge_rect,
            2.0,
            Stroke::new(1.0_f32, Color32::from_rgb(255, 215, 0)),
        );
        painter.text(
            badge_rect.center(),
            egui::Align2::CENTER_CENTER,
            format!("{:.prec$}", latest.close, prec = price_prec),
            FontId::monospace(10.0),
            Color32::from_rgb(255, 225, 80),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_ticks() {
        let candles = generate_tick_candles(&[], 0.001, 0.2, 200);
        assert!(candles.is_empty());
    }

    #[test]
    fn test_single_trend_candles() {
        let pip = 0.001; // JPY pip
        let ticks = vec![
            (150.000, 1000),
            (150.005, 1001),
            (150.010, 1002),
            (150.015, 1003),
        ];
        let candles = generate_tick_candles(&ticks, pip, 0.2, 200);
        assert!(!candles.is_empty());
        assert_eq!(candles.last().unwrap().close, 150.015);
    }

    #[test]
    fn test_noise_cancellation() {
        let pip = 0.001;
        // Move up 0.1 pip, then down 0.1 pip (sub-threshold opposite move of same size)
        let ticks = vec![
            (150.000, 1000),
            (150.001, 1001), // +0.1 pip
            (150.000, 1002), // -0.1 pip (should cancel back to open)
            (150.005, 1003), // +0.5 pip
        ];
        let candles = generate_tick_candles(&ticks, pip, 0.2, 200);
        // The noise should have been absorbed
        assert!(!candles.is_empty());
        assert_eq!(candles.last().unwrap().close, 150.005);
    }

    #[test]
    fn test_draw_tick_candle_view_scaling_headless() {
        let ctx = egui::Context::default();
        let _ = ctx.run(Default::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let rect = Rect::from_min_size(Pos2::new(0.0, 0.0), egui::vec2(600.0, 250.0));
                let painter = ui.painter_at(rect);
                let theme = ChartTheme::default();

                // Create 100 candles: past candles have a high price spike, recent are tight
                let mut candles = Vec::new();
                for i in 0..100 {
                    let base_price = if i < 20 { 160.0 } else { 150.0 };
                    candles.push(TickCandleBar {
                        open: base_price,
                        high: base_price + 0.005,
                        low: base_price - 0.005,
                        close: base_price + 0.002,
                        time_sec: 1000 + (i as i64),
                        is_minute_changed: i % 15 == 0,
                        is_confirmed: true,
                    });
                }

                // Render with standard bar width and verify no panics
                draw_tick_candle_view(
                    &painter,
                    rect,
                    &candles,
                    "Broker Test",
                    0.01,
                    5.0,
                    true,
                    &theme,
                    Some(Pos2::new(300.0, 100.0)),
                );

                // Render with slim 1.0px bar width and hidden current price line
                draw_tick_candle_view(
                    &painter,
                    rect,
                    &candles,
                    "Broker Test",
                    0.01,
                    1.0,
                    false,
                    &theme,
                    None,
                );
            });
        });
    }
}
