//! Global keyboard shortcut handler for the TickScope dashboard.

use super::DashboardApp;
use crate::core::models::UiSnapshot;
use crate::core::types::BrokerId;
use crate::ui::chart::BottomMetric;
use eframe::egui;

/// Process keyboard shortcuts for quick metric switching, pair cycling, and panel toggles.
pub fn handle_hotkeys(app: &mut DashboardApp, ctx: &egui::Context, snapshot: &UiSnapshot) {
    ctx.input(|i| {
        if i.key_pressed(egui::Key::Num2) {
            app.bottom_metric = BottomMetric::MidDiff;
        } else if i.key_pressed(egui::Key::Num3) {
            app.bottom_metric = BottomMetric::BidAskDiff;
        } else if i.key_pressed(egui::Key::Num4) {
            app.bottom_metric = BottomMetric::SpreadDiff;
        } else if i.key_pressed(egui::Key::Num5) {
            app.bottom_metric = BottomMetric::LeadLag;
        } else if i.key_pressed(egui::Key::Num6) {
            app.bottom_metric = BottomMetric::MidDispersion;
        } else if i.key_pressed(egui::Key::Num7) {
            app.bottom_metric = BottomMetric::MoveBreadthView;
        } else if i.key_pressed(egui::Key::Num8) {
            app.bottom_metric = BottomMetric::QuotePersistence;
        } else if i.key_pressed(egui::Key::Num1) {
            app.bottom_metric = BottomMetric::QuotePath;
        } else if i.key_pressed(egui::Key::Tab) {
            if i.modifiers.shift {
                app.bottom_metric = app.bottom_metric.prev();
            } else {
                app.bottom_metric = app.bottom_metric.next();
            }
        } else if i.key_pressed(egui::Key::P) {
            let broker_ids: Vec<BrokerId> = app.visible_broker_ids(&snapshot.broker_overviews);
            app.cycle_pair(&broker_ids, !i.modifiers.shift);
        } else if i.key_pressed(egui::Key::B) {
            app.show_broker_overview = !app.show_broker_overview;
        } else if i.key_pressed(egui::Key::S) || i.key_pressed(egui::Key::Comma) {
            app.show_quick_settings = !app.show_quick_settings;
        } else if i.key_pressed(egui::Key::Escape) {
            if app.show_quick_settings {
                app.show_quick_settings = false;
            } else if app.show_broker_overview {
                app.show_broker_overview = false;
            }
        }
    });
}
