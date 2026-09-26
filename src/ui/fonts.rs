//! Custom font loading with CJK (Japanese) fallback support for egui.

use egui::{FontData, FontDefinitions, FontFamily};

/// Configures fonts for egui, adding a system CJK (Japanese) font as a fallback
/// so that Japanese text displays properly without mojibake / tofu characters.
pub fn setup_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();

    if let Some((name, font_bytes)) = load_system_cjk_font() {
        fonts.font_data.insert(
            name.clone(),
            FontData::from_owned(font_bytes),
        );

        // Append the CJK font as a fallback for both Proportional and Monospace.
        // This preserves default clean ASCII/Latin glyphs while resolving Japanese
        // characters from the system font.
        fonts
            .families
            .entry(FontFamily::Proportional)
            .or_default()
            .push(name.clone());

        fonts
            .families
            .entry(FontFamily::Monospace)
            .or_default()
            .push(name);

        ctx.set_fonts(fonts);
    }
}

/// Attempts to find and read a system CJK/Japanese font file.
fn load_system_cjk_font() -> Option<(String, Vec<u8>)> {
    let mut candidate_paths = Vec::new();

    #[cfg(windows)]
    {
        let win_dir = std::env::var("SystemRoot")
            .or_else(|_| std::env::var("windir"))
            .unwrap_or_else(|_| "C:\\Windows".to_string());
        let fonts_dir = std::path::Path::new(&win_dir).join("Fonts");
        for name in &[
            "meiryo.ttc",
            "YuGothM.ttc",
            "YuGothR.ttc",
            "msgothic.ttc",
            "BIZ-UDGothicR.ttc",
        ] {
            candidate_paths.push(fonts_dir.join(name));
        }
    }

    #[cfg(target_os = "macos")]
    {
        for p in &[
            "/System/Library/Fonts/PingFang.ttc",
            "/System/Library/Fonts/Hiragino Sans GB.ttc",
            "/System/Library/Fonts/Supplemental/Arial Unicode.ttf",
            "/Library/Fonts/Arial Unicode.ttf",
        ] {
            candidate_paths.push(std::path::PathBuf::from(p));
        }
    }

    #[cfg(not(any(windows, target_os = "macos")))]
    {
        for p in &[
            "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/google-noto-cjk/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/truetype/fonts-japanese-gothic.ttf",
            "/usr/share/fonts/truetype/takao-gothic/TakaoPGothic.ttf",
            "/usr/share/fonts/ipaexg.ttf",
        ] {
            candidate_paths.push(std::path::PathBuf::from(p));
        }
    }

    for path in candidate_paths {
        if path.exists() {
            if let Ok(bytes) = std::fs::read(&path) {
                let font_name = path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("cjk_fallback")
                    .to_string();
                return Some((font_name, bytes));
            }
        }
    }

    None
}
