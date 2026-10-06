use crate::core::models::BrokerOverview;
use crate::core::types::BrokerId;
use crate::ui::chart::theme::{broker_color_for_name, ChartTheme};
use egui::{Color32, Pos2, Rect};

/// Quote Persistence View (RFC §45, §61)
pub fn draw_quote_persistence_view(
    painter: &egui::Painter,
    rect: Rect,
    broker_overviews: &[BrokerOverview],
    theme: &ChartTheme,
) {
    draw_quote_persistence_view_with_visibility(painter, rect, broker_overviews, None, theme);
}

pub fn draw_quote_persistence_view_with_visibility(
    painter: &egui::Painter,
    rect: Rect,
    broker_overviews: &[BrokerOverview],
    visible_broker_ids: Option<&[BrokerId]>,
    theme: &ChartTheme,
) {
    painter.rect_filled(rect, 4.0, theme.bg_color);
    painter.text(
        Pos2::new(rect.left() + 8.0, rect.top() + 6.0),
        egui::Align2::LEFT_TOP,
        "Quote Freshness / Tick Rate per Broker",
        egui::FontId::monospace(12.0),
        Color32::WHITE,
    );

    let is_visible = |b: &BrokerOverview| -> bool {
        visible_broker_ids.is_none_or(|v| v.contains(&b.broker_id))
    };

    let visible_count = broker_overviews.iter().filter(|b| is_visible(b)).count();
    if visible_count == 0 {
        return;
    }

    let y0 = rect.top() + 28.0;
    let rh = ((rect.height() - 36.0) / visible_count as f32).min(24.0);
    let bl = rect.left() + rect.width() * 0.2;
    let bw = rect.width() * 0.55;
    let max_rate = broker_overviews
        .iter()
        .filter(|b| is_visible(b))
        .map(|b| b.tick_rate_1s)
        .fold(1.0_f64, f64::max);

    let mut row_idx = 0;
    for (orig_idx, b) in broker_overviews.iter().enumerate() {
        if !is_visible(b) {
            continue;
        }
        let y = y0 + (row_idx as f32) * rh + rh * 0.5;
        row_idx += 1;
        let color = broker_color_for_name(theme, Some(&b.name), orig_idx);
        painter.text(
            Pos2::new(rect.left() + 8.0, y),
            egui::Align2::LEFT_CENTER,
            &b.name,
            egui::FontId::monospace(11.0),
            color,
        );
        let frac = (b.tick_rate_1s / max_rate) as f32;
        let br = Rect::from_min_size(Pos2::new(bl, y - 5.0), egui::Vec2::new(bw * frac, 10.0));
        painter.rect_filled(br, 2.0, color);
        painter.text(
            Pos2::new(br.right() + 6.0, y),
            egui::Align2::LEFT_CENTER,
            format!("{:.0} t/s", b.tick_rate_1s),
            egui::FontId::monospace(10.0),
            crate::ui::style::TEXT_SUBDUED,
        );
        if let Some(q) = &b.latest_quote {
            painter.text(
                Pos2::new(rect.right() - 8.0, y),
                egui::Align2::RIGHT_CENTER,
                format!("Spread: {:.3}", q.spread),
                egui::FontId::monospace(10.0),
                crate::ui::style::TEXT_FAINT,
            );
        }
    }
}
