use egui::{Color32, Context, FontId, Margin, Rounding, Stroke, TextStyle, Vec2, Visuals};

pub const BACKGROUND: Color32 = Color32::from_rgb(17, 22, 29);
pub const SURFACE: Color32 = Color32::from_rgb(24, 31, 40);
pub const BORDER: Color32 = Color32::from_rgb(48, 60, 75);
pub const TEXT: Color32 = Color32::from_rgb(221, 229, 239);
pub const MUTED: Color32 = Color32::from_rgb(151, 167, 187);
pub const ACCENT: Color32 = Color32::from_rgb(103, 196, 232);
pub const LIVE: Color32 = Color32::from_rgb(102, 209, 170);
pub const WARNING: Color32 = Color32::from_rgb(233, 193, 113);
pub const ERROR: Color32 = Color32::from_rgb(239, 139, 147);

/// Configure once per dashboard so every native control shares the chart palette.
pub fn configure(ctx: &Context) {
    let mut style = (*ctx.style()).clone();
    style
        .text_styles
        .insert(TextStyle::Body, FontId::proportional(13.0));
    style
        .text_styles
        .insert(TextStyle::Button, FontId::proportional(13.0));
    style
        .text_styles
        .insert(TextStyle::Small, FontId::proportional(11.0));
    style
        .text_styles
        .insert(TextStyle::Heading, FontId::proportional(17.0));
    style
        .text_styles
        .insert(TextStyle::Monospace, FontId::monospace(13.0));
    style.spacing.item_spacing = Vec2::new(8.0, 7.0);
    style.spacing.button_padding = Vec2::new(9.0, 5.0);
    style.spacing.interact_size.y = 26.0;
    style.spacing.window_margin = Margin::same(14.0);
    style.animation_time = 0.12;

    let mut visuals = Visuals::dark();
    visuals.panel_fill = BACKGROUND;
    visuals.window_fill = SURFACE;
    visuals.extreme_bg_color = BACKGROUND;
    visuals.faint_bg_color = Color32::from_rgb(29, 37, 48);
    visuals.window_stroke = Stroke::new(1.0_f32, BORDER);
    visuals.window_rounding = Rounding::same(10.0);
    visuals.menu_rounding = Rounding::same(7.0);
    visuals.selection.bg_fill = Color32::from_rgb(37, 73, 94);
    visuals.selection.stroke = Stroke::new(1.0_f32, ACCENT);
    visuals.hyperlink_color = ACCENT;
    visuals.warn_fg_color = WARNING;
    visuals.error_fg_color = ERROR;
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, BORDER);
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, TEXT);
    visuals.widgets.inactive.weak_bg_fill = SURFACE;
    visuals.widgets.inactive.bg_fill = SURFACE;
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, BORDER);
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, TEXT);
    visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(38, 53, 69);
    visuals.widgets.hovered.bg_fill = Color32::from_rgb(38, 53, 69);
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, ACCENT);
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0_f32, TEXT);
    visuals.widgets.active.weak_bg_fill = visuals.selection.bg_fill;
    visuals.widgets.active.bg_fill = visuals.selection.bg_fill;
    visuals.widgets.active.bg_stroke = Stroke::new(1.0_f32, ACCENT);
    visuals.widgets.active.fg_stroke = Stroke::new(1.0_f32, TEXT);
    for widget in [
        &mut visuals.widgets.inactive,
        &mut visuals.widgets.hovered,
        &mut visuals.widgets.active,
        &mut visuals.widgets.open,
    ] {
        widget.rounding = Rounding::same(5.0);
        widget.expansion = 0.0;
    }
    style.visuals = visuals;
    ctx.set_style(style);
}
