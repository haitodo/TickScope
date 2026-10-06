use crate::metrics::StageLatencySummary;
use egui::Color32;

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
            ui.label(format!("  {name}: No samples"));
        }
    };
    draw_stage(ui, "Tick→Engine", &summary.tick_to_engine);
    draw_stage(ui, "Engine→Proj", &summary.engine_to_projection);
    draw_stage(ui, "Proj→Snap", &summary.projection_to_snapshot);
    draw_stage(ui, "Snap→UI", &summary.snapshot_to_ui);
    draw_stage(ui, "Total", &summary.total_pipeline);
}
