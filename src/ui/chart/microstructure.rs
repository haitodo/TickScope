use crate::core::models::{BrokerOverview, MoveDirection, MoveQuality, PairComparison};
use crate::core::types::{BrokerId, ConnectionState, FreshnessState};
use crate::metrics::{EventCluster, MoveBreadth, ObservedBrokerConsensus, StageLatencySummary};
use super::theme::{broker_color_for, ChartTheme};
use egui::{Color32, Pos2, Rect, Stroke};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum BottomMetric {
    MidDiff,
    BidAskDiff,
    SpreadDiff,
    LeadLag,
    MidDispersion,
    MoveBreadthView,
    QuotePersistence,
    #[default]
    QuotePath,
}

impl BottomMetric {
    pub const ALL: [BottomMetric; 8] = [
        BottomMetric::QuotePath,
        BottomMetric::MidDiff,
        BottomMetric::BidAskDiff,
        BottomMetric::SpreadDiff,
        BottomMetric::LeadLag,
        BottomMetric::MidDispersion,
        BottomMetric::MoveBreadthView,
        BottomMetric::QuotePersistence,
    ];

    pub fn key_number(&self) -> u32 {
        match self {
            BottomMetric::MidDiff => 2,
            BottomMetric::BidAskDiff => 3,
            BottomMetric::SpreadDiff => 4,
            BottomMetric::LeadLag => 5,
            BottomMetric::MidDispersion => 6,
            BottomMetric::MoveBreadthView => 7,
            BottomMetric::QuotePersistence => 8,
            BottomMetric::QuotePath => 1,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            BottomMetric::MidDiff => "2: Mid Diff",
            BottomMetric::BidAskDiff => "3: Bid/Ask Diff",
            BottomMetric::SpreadDiff => "4: Spread Diff",
            BottomMetric::LeadLag => "5: Lead/Lag",
            BottomMetric::MidDispersion => "6: Dispersion",
            BottomMetric::MoveBreadthView => "7: Breadth",
            BottomMetric::QuotePersistence => "8: Persistence",
            BottomMetric::QuotePath => "1: Quote Path",
        }
    }

    pub fn title(&self) -> &'static str {
        match self {
            BottomMetric::MidDiff => "Mid Price Difference (A - B)",
            BottomMetric::BidAskDiff => "Bid & Ask Difference (A - B)",
            BottomMetric::SpreadDiff => "Spread Difference (A - B)",
            BottomMetric::LeadLag => "Lead / Lag Diagnostics",
            BottomMetric::MidDispersion => "Mid Dispersion (Deviation from Broker Median)",
            BottomMetric::MoveBreadthView => "Directional Move Breadth",
            BottomMetric::QuotePersistence => "Quote Freshness / Age per Broker",
            BottomMetric::QuotePath => "Realtime Multi-Broker Quote Path",
        }
    }

    pub fn next(&self) -> Self {
        match self {
            BottomMetric::MidDiff => BottomMetric::BidAskDiff,
            BottomMetric::BidAskDiff => BottomMetric::SpreadDiff,
            BottomMetric::SpreadDiff => BottomMetric::LeadLag,
            BottomMetric::LeadLag => BottomMetric::MidDispersion,
            BottomMetric::MidDispersion => BottomMetric::MoveBreadthView,
            BottomMetric::MoveBreadthView => BottomMetric::QuotePersistence,
            BottomMetric::QuotePersistence => BottomMetric::QuotePath,
            BottomMetric::QuotePath => BottomMetric::MidDiff,
        }
    }

    pub fn prev(&self) -> Self {
        match self {
            BottomMetric::MidDiff => BottomMetric::QuotePath,
            BottomMetric::BidAskDiff => BottomMetric::MidDiff,
            BottomMetric::SpreadDiff => BottomMetric::BidAskDiff,
            BottomMetric::LeadLag => BottomMetric::SpreadDiff,
            BottomMetric::MidDispersion => BottomMetric::LeadLag,
            BottomMetric::MoveBreadthView => BottomMetric::MidDispersion,
            BottomMetric::QuotePersistence => BottomMetric::MoveBreadthView,
            BottomMetric::QuotePath => BottomMetric::QuotePersistence,
        }
    }

    pub fn from_key_number(n: u32) -> Option<Self> {
        match n {
            2 => Some(BottomMetric::MidDiff),
            3 => Some(BottomMetric::BidAskDiff),
            4 => Some(BottomMetric::SpreadDiff),
            5 => Some(BottomMetric::LeadLag),
            6 => Some(BottomMetric::MidDispersion),
            7 => Some(BottomMetric::MoveBreadthView),
            8 => Some(BottomMetric::QuotePersistence),
            1 => Some(BottomMetric::QuotePath),
            _ => None,
        }
    }

    pub fn category(&self) -> BottomMetricCategory {
        match self {
            Self::MidDiff | Self::BidAskDiff | Self::SpreadDiff | Self::LeadLag => {
                BottomMetricCategory::PairDiff
            }
            Self::MidDispersion | Self::MoveBreadthView | Self::QuotePersistence => {
                BottomMetricCategory::MarketConsensus
            }
            Self::QuotePath => BottomMetricCategory::RawQuotes,
        }
    }

    pub fn is_pair_metric(&self) -> bool {
        matches!(self.category(), BottomMetricCategory::PairDiff)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BottomMetricCategory {
    PairDiff,
    MarketConsensus,
    RawQuotes,
}

impl BottomMetricCategory {
    pub const ALL: [BottomMetricCategory; 3] = [
        BottomMetricCategory::RawQuotes,
        BottomMetricCategory::PairDiff,
        BottomMetricCategory::MarketConsensus,
    ];

    pub fn title(&self) -> &'static str {
        match self {
            Self::PairDiff => "Pair Differentials (2社比較)",
            Self::MarketConsensus => "Market Consensus (市場統計)",
            Self::RawQuotes => "Raw Quotes / Ticks (リアルタイム価格)",
        }
    }

    pub fn metrics(&self) -> &'static [BottomMetric] {
        match self {
            Self::PairDiff => &[
                BottomMetric::MidDiff,
                BottomMetric::BidAskDiff,
                BottomMetric::SpreadDiff,
                BottomMetric::LeadLag,
            ],
            Self::MarketConsensus => &[
                BottomMetric::MidDispersion,
                BottomMetric::MoveBreadthView,
                BottomMetric::QuotePersistence,
            ],
            Self::RawQuotes => &[BottomMetric::QuotePath],
        }
    }
}

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

/// One-Line State Ribbon (RFC §57)
pub fn draw_state_ribbon(
    ui: &mut egui::Ui,
    brokers: &[BrokerOverview],
    consensus: &Option<ObservedBrokerConsensus>,
    clusters: &[EventCluster],
    breadth: &Option<MoveBreadth>,
) {
    ui.horizontal(|ui| {
        let live_count = brokers
            .iter()
            .filter(|b| {
                b.health.connection == ConnectionState::Connected
                    && b.health.data_freshness == FreshnessState::Live
            })
            .count();
        let overloaded = brokers.iter().any(|b| {
            let flags = b.health.overload;
            flags.receiver || flags.engine || flags.logger || flags.analysis
        });
        let (global_status, global_color) = if overloaded {
            ("OVERLOAD", Color32::RED)
        } else if !brokers.is_empty() && live_count == brokers.len() {
            ("SYSTEM_OK", Color32::GREEN)
        } else if live_count > 0 {
            ("PARTIAL", Color32::YELLOW)
        } else {
            ("DEGRADED", Color32::RED)
        };
        ui.colored_label(global_color, global_status);
        ui.separator();
        if let Some(c) = consensus {
            let fresh_color = if c.fresh_count == c.total_count {
                Color32::from_rgb(0, 200, 160)
            } else {
                Color32::from_rgb(255, 200, 80)
            };
            ui.colored_label(
                fresh_color,
                format!("Fresh {}/{}", c.fresh_count, c.total_count),
            );
            ui.separator();
            if let Some(median) = c.consensus_mid {
                ui.label(format!("Observed Broker Median {:.3}", median));
                ui.separator();
            }
            if let Some(range) = c.mid_range {
                ui.label(format!("Range {:.3}", range));
            }
            ui.separator();
        }
        if let Some(cluster) = clusters.last() {
            let dir = match cluster.direction {
                MoveDirection::Up => "UP",
                MoveDirection::Down => "DOWN",
            };
            ui.label(format!(
                "{} Cluster {}/{} {:.0}ms",
                dir,
                cluster.participating_brokers.len(),
                cluster.total_brokers,
                cluster.observed_span_ms
            ));
            ui.separator();
        }
        if let Some(b) = breadth {
            if b.up_count > 0 || b.down_count > 0 {
                ui.label(format!(
                    "Breadth UP {} DN {}",
                    b.up_ratio_str(),
                    b.down_ratio_str()
                ));
            } else {
                ui.label("Breadth: Quiet");
            }
        }
    });
}

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
        let color = broker_color_for(theme, orig_idx);
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
            Color32::from_rgb(80, 200, 220),
        );
        painter.rect_filled(
            Rect::from_min_size(Pos2::new(bl, y0), egui::Vec2::new(bw * up_f, 12.0)),
            2.0,
            Color32::from_rgb(80, 200, 220),
        );

        let dn_f = b.down_count as f32 / total as f32;
        let dy = y0 + 20.0;
        painter.text(
            Pos2::new(bl - 8.0, dy + 4.0),
            egui::Align2::RIGHT_CENTER,
            format!("DN {}", b.down_ratio_str()),
            egui::FontId::monospace(11.0),
            Color32::from_rgb(255, 160, 80),
        );
        painter.rect_filled(
            Rect::from_min_size(Pos2::new(bl, dy), egui::Vec2::new(bw * dn_f, 12.0)),
            2.0,
            Color32::from_rgb(255, 160, 80),
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
            Color32::from_gray(160),
        );
        let name_of = |bid: BrokerId| -> &str {
            broker_overviews
                .iter()
                .find(|b| b.broker_id == bid)
                .map(|b| b.name.as_str())
                .unwrap_or("?")
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
                Color32::from_gray(140),
            );
        }
    }
}

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
        let color = broker_color_for(theme, orig_idx);
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
            Color32::from_gray(180),
        );
        if let Some(q) = &b.latest_quote {
            painter.text(
                Pos2::new(rect.right() - 8.0, y),
                egui::Align2::RIGHT_CENTER,
                format!("Spread: {:.3}", q.spread),
                egui::FontId::monospace(10.0),
                Color32::from_gray(140),
            );
        }
    }
}

/// Latency Dashboard for Debug overlay (RFC §66)
pub fn draw_latency_dashboard(ui: &mut egui::Ui, summary: &StageLatencySummary) {
    ui.separator();
    ui.label(
        egui::RichText::new("Pipeline Latency")
            .strong()
            .color(Color32::from_rgb(200, 180, 255)),
    );
    let draw_stage = |ui: &mut egui::Ui, name: &str, stats: &crate::metrics::PercentileStats| {
        if stats.sample_count > 0 {
            ui.label(format!(
                "  {} (n={}): p50 {:.0}µs  p95 {:.0}µs  p99 {:.0}µs  max {:.0}µs",
                name, stats.sample_count, stats.p50_us, stats.p95_us, stats.p99_us, stats.max_us
            ));
        } else {
            ui.label(format!("  {}: No samples", name));
        }
    };
    draw_stage(ui, "Tick→Engine", &summary.tick_to_engine);
    draw_stage(ui, "Engine→Proj", &summary.engine_to_projection);
    draw_stage(ui, "Proj→Snap", &summary.projection_to_snapshot);
    draw_stage(ui, "Snap→UI", &summary.snapshot_to_ui);
    draw_stage(ui, "Total", &summary.total_pipeline);
}
