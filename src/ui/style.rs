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

// ---------------------------------------------------------------------------
// Secondary palette.
//
// These tones are deliberately separate constants even where two of them look
// almost identical: they drifted apart over time and are *not* interchangeable.
// Merging any pair is a visual change and needs a design review, not a
// refactor. Chart-internal series colours live in `chart::theme`.
// ---------------------------------------------------------------------------

/// Text tones, roughly brightest to faintest.
pub const TEXT_OVERLAY: Color32 = Color32::from_gray(190); // chart overlay info labels
pub const TEXT_AXIS: Color32 = Color32::from_gray(185); // chart axis price labels
pub const TEXT_SUBDUED: Color32 = Color32::from_gray(180); // legends, badges, secondary values
pub const TEXT_CAPTION: Color32 = Color32::from_gray(170); // small captions
pub const TEXT_LABEL: Color32 = Color32::from_gray(160); // form labels and hints
pub const TEXT_HINT: Color32 = Color32::from_gray(150); // de-emphasised notes
pub const TEXT_FAINT: Color32 = Color32::from_gray(140); // helper text, inactive toggles
pub const TEXT_SEPARATOR: Color32 = Color32::from_gray(130); // inline separators such as "vs"
pub const TEXT_DISABLED: Color32 = Color32::from_gray(120); // zero / inactive counters
pub const RULE_DARK: Color32 = Color32::from_gray(60); // dark divider strokes

/// Row background tints for list panels.
///
/// Functions rather than constants: `Color32::from_rgba_unmultiplied` is not a
/// `const fn` in egui 0.29.
pub fn row_hover() -> Color32 {
    Color32::from_rgba_unmultiplied(255, 255, 255, 6)
}
pub fn row_stripe() -> Color32 {
    Color32::from_rgba_unmultiplied(255, 255, 255, 2)
}

/// Section headings, buttons and interactive highlights.
pub const SECTION_TITLE: Color32 = Color32::from_rgb(180, 220, 255);
pub const LABEL_STRONG: Color32 = Color32::from_rgb(180, 200, 220);
pub const BUTTON_TEXT: Color32 = Color32::from_rgb(220, 230, 255);
pub const HIGHLIGHT: Color32 = Color32::from_rgb(0, 220, 255); // active metric, Pin, broker A
pub const INFO: Color32 = Color32::from_rgb(100, 220, 255); // start/stop counters
pub const BROKER_B: Color32 = Color32::from_rgb(255, 120, 200); // broker B label
pub const OVERLAY_ON: Color32 = Color32::from_rgb(0, 240, 160); // trade overlay enabled
pub const METRIC_ACCENT: Color32 = Color32::from_rgb(200, 180, 255); // metric panel headings

/// Coarse status colours, one per indicator; keep them separate.
pub const STATUS_OK: Color32 = Color32::from_rgb(0, 220, 140); // header: all brokers live
pub const STATUS_WARN: Color32 = Color32::from_rgb(255, 200, 60); // header: some live
pub const STATUS_ALERT: Color32 = Color32::from_rgb(255, 120, 120); // header: none live / stop
pub const STATUS_ALERT_SOFT: Color32 = Color32::from_rgb(255, 140, 140); // running counter
pub const FRESH_ALL: Color32 = Color32::from_rgb(0, 200, 160); // ribbon: all quotes fresh
pub const FRESH_PARTIAL: Color32 = Color32::from_rgb(255, 200, 80); // ribbon: some quotes stale

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
