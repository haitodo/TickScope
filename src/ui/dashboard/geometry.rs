//! Window geometry tracking and automatic state persistence.

use super::DashboardApp;
use crate::ui::settings::{MIN_WINDOW_HEIGHT, MIN_WINDOW_WIDTH};
use eframe::egui;

/// Monitor viewport resize and move events, marking state dirty and auto-saving when needed.
pub fn track_window_geometry(app: &mut DashboardApp, ctx: &egui::Context) {
    ctx.input(|i| {
        let vp = i.viewport();
        if let Some(maximized) = vp.maximized {
            if app.window_geometry.maximized != maximized {
                app.window_geometry.maximized = maximized;
                app.state_dirty = true;
            }
        }
        if !app.window_geometry.maximized {
            if let Some(rect) = vp.inner_rect {
                let size = [rect.width(), rect.height()];
                if size[0] >= MIN_WINDOW_WIDTH && size[1] >= MIN_WINDOW_HEIGHT
                    && ((app.window_geometry.inner_size[0] - size[0]).abs() > 1.0
                        || (app.window_geometry.inner_size[1] - size[1]).abs() > 1.0)
                {
                    app.window_geometry.inner_size = size;
                    app.state_dirty = true;
                }
            }
            if let Some(rect) = vp.outer_rect {
                let pos = [rect.min.x, rect.min.y];
                if app.window_geometry.position != Some(pos) {
                    app.window_geometry.position = Some(pos);
                    app.state_dirty = true;
                }
            }
        }
    });

    if app.state_dirty || ctx.input(|i| i.viewport().close_requested()) {
        app.save_state();
    }
}
