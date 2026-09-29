use crate::core::models::{BrokerOverview, MoveDirection, MoveQuality, PairComparison};
use crate::core::types::BrokerId;
use crate::ui::chart::theme::ChartTheme;
use egui::{Color32, Pos2, Rect, Stroke};

/// Lead / Lag diagnostics view between broker A and broker B (RFC §29, §61).
pub fn draw_lead_lag_view(
    painter: &egui::Painter,
    rect: Rect,
    comparison: Option<&PairComparison>,
    broker_overviews: &[BrokerOverview],
    theme: &ChartTheme,
) {
    painter.rect_filled(rect, 4.0, theme.bg_color);

    let comp = match comparison {
        Some(c) => c,
        None => {
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "No active pair comparison available",
                egui::FontId::proportional(13.0),
                Color32::GRAY,
            );
            return;
        }
    };

    let name_of = |bid: BrokerId| -> String {
        broker_overviews
            .iter()
            .find(|b| b.broker_id == bid)
            .map(|b| b.name.clone())
            .unwrap_or_else(|| format!("Broker #{}", bid))
    };

    let broker_a_name = name_of(comp.broker_a);
    let broker_b_name = name_of(comp.broker_b);

    if let Some(m) = &comp.latest_match {
        let leader_name = name_of(m.leader);
        let follower_name = name_of(m.follower);

        let ema_text = comp
            .ema_lead_lag_ms
            .map(|e| format!("  |  EMA Lead: {:+.1} ms", e))
            .unwrap_or_default();

        let header_text = format!(
            "First observed on this PC: {} ({:.1} ms{})",
            leader_name,
            m.raw_delta_ms.abs(),
            ema_text
        );

        painter.text(
            Pos2::new(rect.left() + 16.0, rect.top() + 14.0),
            egui::Align2::LEFT_TOP,
            header_text,
            egui::FontId::proportional(16.0),
            Color32::from_rgb(255, 215, 0),
        );

        // Visual bar showing relative lead direction
        let bar_center_y = rect.top() + 50.0;
        let bar_center_x = rect.center().x;
        let max_bar_half_width = rect.width() * 0.35;

        // Base line
        painter.line_segment(
            [
                Pos2::new(bar_center_x - max_bar_half_width, bar_center_y),
                Pos2::new(bar_center_x + max_bar_half_width, bar_center_y),
            ],
            Stroke::new(2.0_f32, Color32::from_gray(60)),
        );

        // Center tick
        painter.line_segment(
            [
                Pos2::new(bar_center_x, bar_center_y - 8.0),
                Pos2::new(bar_center_x, bar_center_y + 8.0),
            ],
            Stroke::new(2.0_f32, Color32::from_gray(140)),
        );

        // Labels for A (Left) and B (Right)
        painter.text(
            Pos2::new(bar_center_x - max_bar_half_width - 8.0, bar_center_y),
            egui::Align2::RIGHT_CENTER,
            format!("{} (A)", broker_a_name),
            egui::FontId::proportional(12.0),
            if m.leader == comp.broker_a {
                theme.candle_up_a
            } else {
                Color32::GRAY
            },
        );

        painter.text(
            Pos2::new(bar_center_x + max_bar_half_width + 8.0, bar_center_y),
            egui::Align2::LEFT_CENTER,
            format!("{} (B)", broker_b_name),
            egui::FontId::proportional(12.0),
            if m.leader == comp.broker_b {
                theme.candle_up_b
            } else {
                Color32::GRAY
            },
        );

        // Bar indicator
        let bar_len = ((m.raw_delta_ms.abs() / 100.0) as f32 * max_bar_half_width)
            .clamp(8.0, max_bar_half_width);
        let (bar_rect, bar_color) = if m.leader == comp.broker_a {
            (
                Rect::from_min_max(
                    Pos2::new(bar_center_x - bar_len, bar_center_y - 6.0),
                    Pos2::new(bar_center_x, bar_center_y + 6.0),
                ),
                theme.candle_up_a,
            )
        } else {
            (
                Rect::from_min_max(
                    Pos2::new(bar_center_x, bar_center_y - 6.0),
                    Pos2::new(bar_center_x + bar_len, bar_center_y + 6.0),
                ),
                theme.candle_up_b,
            )
        };
        painter.rect_filled(bar_rect, 2.0, bar_color);

        // Event detail section
        let dir_str = match m.leader_event.direction {
            MoveDirection::Up => "UP",
            MoveDirection::Down => "DOWN",
        };
        let quality_str = match m.leader_event.quality {
            MoveQuality::BothSides => "BothSides",
            MoveQuality::BidOnly => "BidOnly",
            MoveQuality::AskOnly => "AskOnly",
            MoveQuality::SpreadDriven => "SpreadDriven",
        };

        let details = format!(
            "Match #{}: {} followed {} | Delta: {:.1} ms | Dir: {} | Quality: {} | ΔMid: {:.1} pts",
            m.match_id,
            follower_name,
            leader_name,
            m.raw_delta_ms.abs(),
            dir_str,
            quality_str,
            m.leader_event.mid_delta_points
        );

        painter.text(
            Pos2::new(rect.left() + 16.0, rect.top() + 75.0),
            egui::Align2::LEFT_TOP,
            details,
            egui::FontId::monospace(12.0),
            Color32::from_gray(180),
        );
    } else {
        let ema_info = comp
            .ema_lead_lag_ms
            .map(|e| format!("Current EMA Lead/Lag: {:+.1} ms\n", e))
            .unwrap_or_default();

        let msg = format!(
            "{}Waiting for significant price moves between {} and {}...",
            ema_info, broker_a_name, broker_b_name
        );

        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            msg,
            egui::FontId::proportional(13.0),
            Color32::GRAY,
        );
    }
}
