use crate::core::models::BrokerOverview;
use crate::core::types::{BrokerId, ConnectionState, FreshnessState};
use crate::metrics::ObservedBrokerConsensus;
use crate::ui::chart::theme::{broker_color_for_name, ChartTheme};
use egui::{Color32, Pos2, Rect, Stroke};

/// Mid Dispersion View (RFC §26, §61)
pub fn draw_mid_dispersion_view(
    painter: &egui::Painter,
    rect: Rect,
    consensus: &Option<ObservedBrokerConsensus>,
    broker_overviews: &[BrokerOverview],
    theme: &ChartTheme,
) {
    draw_mid_dispersion_view_with_visibility(painter, rect, consensus, broker_overviews, None, theme);
}

pub fn draw_mid_dispersion_view_with_visibility(
    painter: &egui::Painter,
    rect: Rect,
    consensus: &Option<ObservedBrokerConsensus>,
    broker_overviews: &[BrokerOverview],
    visible_broker_ids: Option<&[BrokerId]>,
    theme: &ChartTheme,
) {
    painter.rect_filled(rect, 4.0, theme.bg_color);
    let cons = match consensus {
        Some(c) => c,
        None => {
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "No consensus data yet",
                egui::FontId::proportional(13.0),
                Color32::GRAY,
            );
            return;
        }
    };
    let median = match cons.consensus_mid {
        Some(m) => m,
        None => {
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "Insufficient fresh brokers",
                egui::FontId::proportional(13.0),
                Color32::GRAY,
            );
            return;
        }
    };

    painter.text(
        Pos2::new(rect.left() + 8.0, rect.top() + 6.0),
        egui::Align2::LEFT_TOP,
        format!(
            "Observed Broker Median: {:.3}  |  Fresh: {}/{}  |  Range: {:.1}pt",
            median,
            cons.fresh_count,
            cons.total_count,
            cons.mid_range.unwrap_or(0.0) * 1000.0
        ),
        egui::FontId::monospace(11.0),
        Color32::WHITE,
    );

    if let Some(mad) = cons.median_abs_deviation {
        painter.text(
            Pos2::new(rect.right() - 8.0, rect.top() + 6.0),
            egui::Align2::RIGHT_TOP,
            format!("MAD: {:.2}pt", mad * 1000.0),
            egui::FontId::monospace(11.0),
            Color32::from_rgb(200, 180, 255),
        );
    }

    let is_visible = |b: &BrokerOverview| -> bool {
        visible_broker_ids.is_none_or(|v| v.contains(&b.broker_id))
    };

    let bar_top = rect.top() + 26.0;
    let bar_height = (rect.height() - 32.0).max(20.0);
    let visible_count = broker_overviews.iter().filter(|b| is_visible(b)).count();
    let n = visible_count.max(1);
    let row_h = (bar_height / n as f32).min(22.0);
    let cx = rect.center().x;
    let max_half = rect.width() * 0.35;
    let max_dev = broker_overviews
        .iter()
        .filter(|b| {
            is_visible(b)
                && b.health.connection == ConnectionState::Connected
                && b.health.data_freshness == FreshnessState::Live
        })
        .filter_map(|b| b.latest_quote.as_ref().map(|q| (q.mid - median).abs()))
        .fold(0.001_f64, f64::max);

    painter.line_segment(
        [Pos2::new(cx, bar_top), Pos2::new(cx, bar_top + bar_height)],
        Stroke::new(1.0_f32, theme.zero_line),
    );

    let mut row_idx = 0;
    for (orig_idx, b) in broker_overviews.iter().enumerate() {
        if !is_visible(b) {
            continue;
        }
        let y = bar_top + (row_idx as f32) * row_h + row_h * 0.5;
        row_idx += 1;
        let color = broker_color_for_name(theme, Some(&b.name), orig_idx);
        painter.text(
            Pos2::new(rect.left() + 8.0, y),
            egui::Align2::LEFT_CENTER,
            &b.name,
            egui::FontId::monospace(11.0),
            color,
        );

        if b.health.connection != ConnectionState::Connected
            || b.health.data_freshness != FreshnessState::Live
        {
            painter.text(
                Pos2::new(rect.right() - 8.0, y),
                egui::Align2::RIGHT_CENTER,
                if b.health.connection == ConnectionState::Disconnected {
                    "DISCONNECTED"
                } else {
                    "STALE / WARMING"
                },
                egui::FontId::monospace(10.0),
                Color32::GRAY,
            );
            continue;
        }
        if let Some(q) = &b.latest_quote {
            let dev = q.mid - median;
            let bar_len = ((dev.abs() / max_dev) as f32 * max_half).clamp(2.0, max_half);
            let br = if dev >= 0.0 {
                Rect::from_min_max(Pos2::new(cx, y - 4.0), Pos2::new(cx + bar_len, y + 4.0))
            } else {
                Rect::from_min_max(Pos2::new(cx - bar_len, y - 4.0), Pos2::new(cx, y + 4.0))
            };
            let is_outlier = cons.outliers.iter().any(|o| o.broker_id == b.broker_id);
            painter.rect_filled(
                br,
                2.0,
                if is_outlier {
                    Color32::from_rgb(255, 100, 100)
                } else {
                    color
                },
            );
            let (lx, al) = if dev >= 0.0 {
                (br.right() + 4.0, egui::Align2::LEFT_CENTER)
            } else {
                (br.left() - 4.0, egui::Align2::RIGHT_CENTER)
            };
            painter.text(
                Pos2::new(lx, y),
                al,
                format!("{:+.1}pt", dev * 1000.0),
                egui::FontId::monospace(10.0),
                Color32::from_gray(180),
            );
            if is_outlier {
                painter.text(
                    Pos2::new(rect.right() - 8.0, y),
                    egui::Align2::RIGHT_CENTER,
                    "Large Deviation",
                    egui::FontId::monospace(9.0),
                    Color32::from_rgb(255, 140, 100),
                );
            }
        }
    }
}
