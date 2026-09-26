use tick_compare::ui::setup_fonts;

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

    let _ = ctx.run(Default::default(), |ctx| {
        for text in &test_strings {
            let prop_galley = ctx.fonts(|f| {
                f.layout_no_wrap(
                    text.to_string(),
                    egui::FontId::proportional(14.0),
                    egui::Color32::WHITE,
                )
            });
            assert!(
                !prop_galley.rows.is_empty(),
                "Proportional galley should not be empty for text: {}",
                text
            );
            assert!(
                !prop_galley.rows[0].glyphs.is_empty(),
                "Proportional glyphs should not be empty for text: {}",
                text
            );

            let mono_galley = ctx.fonts(|f| {
                f.layout_no_wrap(
                    text.to_string(),
                    egui::FontId::monospace(14.0),
                    egui::Color32::WHITE,
                )
            });
            assert!(
                !mono_galley.rows.is_empty(),
                "Monospace galley should not be empty for text: {}",
                text
            );
            assert!(
                !mono_galley.rows[0].glyphs.is_empty(),
                "Monospace glyphs should not be empty for text: {}",
                text
            );
        }
    });
}
