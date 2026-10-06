use crate::core::models::{BrokerOverview, MoveDirection};
use crate::core::types::BrokerId;
use crate::metrics::{EventCluster, MoveBreadth};
use crate::ui::chart::theme::ChartTheme;
use egui::{Color32, Pos2, Rect};

/// Move Breadth View (RFC §42, §61)
pub fn draw_move_breadth_view(
    painter: &egui::Painter,
    rect: Rect,
    breadth: &Option<MoveBreadth>,
    clusters: &[EventCluster],
    broker_overviews: &[BrokerOverview],
    theme: &ChartTheme,
) {
    painter.rect_filled(rect, 4.0, theme.bg_color);
    painter.text(
        Pos2::new(rect.left() + 8.0, rect.top() + 6.0),
        egui::Align2::LEFT_TOP,
        "Directional Move Breadth",
        egui::FontId::monospace(12.0),
        Color32::WHITE,
    );

    let y0 = rect.top() + 28.0;
    if let Some(b) = breadth {
        let total = b.total_brokers.max(1);
        let bw = rect.width() * 0.6;
        let bl = rect.left() + rect.width() * 0.25;

        let up_f = b.up_count as f32 / total as f32;
        painter.text(
            Pos2::new(bl - 8.0, y0 + 4.0),
            egui::Align2::RIGHT_CENTER,
            format!("UP {}", b.up_ratio_str()),
            egui::FontId::monospace(11.0),
            crate::ui::chart::theme::BREADTH_UP,
        );
        painter.rect_filled(
            Rect::from_min_size(Pos2::new(bl, y0), egui::Vec2::new(bw * up_f, 12.0)),
            2.0,
            crate::ui::chart::theme::BREADTH_UP,
        );

        let dn_f = b.down_count as f32 / total as f32;
        let dy = y0 + 20.0;
        painter.text(
            Pos2::new(bl - 8.0, dy + 4.0),
            egui::Align2::RIGHT_CENTER,
            format!("DN {}", b.down_ratio_str()),
            egui::FontId::monospace(11.0),
            crate::ui::chart::theme::BREADTH_DOWN,
        );
        painter.rect_filled(
            Rect::from_min_size(Pos2::new(bl, dy), egui::Vec2::new(bw * dn_f, 12.0)),
            2.0,
            crate::ui::chart::theme::BREADTH_DOWN,
        );
    } else {
        painter.text(
            Pos2::new(rect.center().x, y0 + 10.0),
            egui::Align2::CENTER_TOP,
            "No breadth data yet",
            egui::FontId::proportional(13.0),
            Color32::GRAY,
        );
    }

    let cy = y0 + 55.0;
    if !clusters.is_empty() {
        painter.text(
            Pos2::new(rect.left() + 8.0, cy),
            egui::Align2::LEFT_TOP,
            "Recent Clusters:",
            egui::FontId::monospace(11.0),
            crate::ui::style::TEXT_LABEL,
        );
        let name_of = |bid: BrokerId| -> &str {
            broker_overviews
                .iter()
                .find(|b| b.broker_id == bid)
                .map_or("?", |b| b.name.as_str())
        };
        for (i, cl) in clusters.iter().rev().take(3).enumerate() {
            let d = match cl.direction {
                MoveDirection::Up => "UP",
                MoveDirection::Down => "DN",
            };
            painter.text(
                Pos2::new(rect.left() + 16.0, cy + 16.0 + (i as f32) * 14.0),
                egui::Align2::LEFT_TOP,
                format!(
                    "{} {}/{} {:.0}ms First:{} Last:{}",
                    d,
                    cl.participating_brokers.len(),
                    cl.total_brokers,
                    cl.observed_span_ms,
                    name_of(cl.first_observed),
                    name_of(cl.last_observed)
                ),
                egui::FontId::monospace(10.0),
                crate::ui::style::TEXT_FAINT,
            );
        }
    }
}
