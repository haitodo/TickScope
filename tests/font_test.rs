use tick_scope::ui::setup_fonts;

#[test]
fn test_setup_fonts_and_japanese_shaping() {
    let ctx = egui::Context::default();
    setup_fonts(&ctx);

    let test_strings = [
        "MT5接続待ち: 対象銘柄チャートに共通EA TickCollector を追加してください。既にEAが動作中なら一度外して再追加してください。接続が拒否された場合はMT5のエキスパートログを確認してください。",
        "Broker Overview",
        "CONNECTING · 150 ms",
        "—",
    ];

    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
        let ctx = ui.ctx();
        for text in &test_strings {
            let prop_galley = ctx.fonts_mut(|f| {
                f.layout_no_wrap(
                    text.to_string(),
                    egui::FontId::proportional(14.0),
                    egui::Color32::WHITE,
                )
            });
            assert!(
                !prop_galley.rows.is_empty(),
                "Proportional galley should not be empty for text: {text}"
            );
            assert!(
                !prop_galley.rows[0].glyphs.is_empty(),
                "Proportional glyphs should not be empty for text: {text}"
            );

            let mono_galley = ctx.fonts_mut(|f| {
                f.layout_no_wrap(
                    text.to_string(),
                    egui::FontId::monospace(14.0),
                    egui::Color32::WHITE,
                )
            });
            assert!(
                !mono_galley.rows.is_empty(),
                "Monospace galley should not be empty for text: {text}"
            );
            assert!(
                !mono_galley.rows[0].glyphs.is_empty(),
                "Monospace glyphs should not be empty for text: {text}"
            );
        }
    });
    output.textures_delta.clear();
}

#[test]
fn test_ui_icons_have_glyphs() {
    let ctx = egui::Context::default();
    setup_fonts(&ctx);

    let icons = [
        ('📌', "Pin"),
        ('⚙', "Settings"),
        ('▶', "Play/Launch"),
        ('⏹', "Stop"),
        ('●', "Status dot"),
        ('▾', "Dropdown arrow"),
        ('▲', "Expand up"),
        ('▼', "Expand down"),
        ('◀', "Prev"),
        ('⟲', "Reset"),
        ('⚡', "Fast/Flash"),
    ];

    let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
        let ctx = ui.ctx();
        let font_id = egui::FontId::proportional(14.0);
        for (ch, label) in icons {
            let has = ctx.fonts_mut(|f| f.has_glyph(&font_id, ch));
            assert!(
                has,
                "Missing glyph for {label} icon '{ch}' (U+{:04X})",
                ch as u32
            );
        }
    });
    output.textures_delta.clear();
}
