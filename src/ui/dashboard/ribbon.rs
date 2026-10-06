use crate::core::models::{BrokerOverview, MoveDirection};
use crate::core::types::{ConnectionState, FreshnessState};
use crate::metrics::{EventCluster, MoveBreadth, ObservedBrokerConsensus};
use egui::Color32;

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
                ui.label(format!("Observed Broker Median {median:.3}"));
                ui.separator();
            }
            if let Some(range) = c.mid_range {
                ui.label(format!("Range {range:.3}"));
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
