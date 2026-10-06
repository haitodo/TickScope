use egui::Color32;

/// Broker Brand Identity Colors (RFC §87)
pub const COLOR_OANDA: Color32 = Color32::from_rgb(0, 211, 126); // #00D37E (OANDA)
pub const COLOR_AXIORY: Color32 = Color32::from_rgb(138, 41, 213); // #8A29D5 (Axiory)
pub const COLOR_TRADEVIEW: Color32 = Color32::from_rgb(229, 57, 53); // #E53935 (Tradeview)
pub const COLOR_JFX: Color32 = Color32::from_rgb(168, 201, 43); // #A8C92B (JFX)
pub const COLOR_DUKASCOPY: Color32 = Color32::from_rgb(59, 130, 246); // #3B82F6 (Dukascopy)

pub struct ChartTheme {
    pub bg_color: Color32,
    pub grid_color: Color32,
    pub candle_up_a: Color32,
    pub candle_down_a: Color32,
    pub candle_up_b: Color32,
    pub candle_down_b: Color32,
    pub diff_line: Color32,
    pub bid_diff_line: Color32,
    pub ask_diff_line: Color32,
    pub spread_diff_line: Color32,
    pub zero_line: Color32,
    /// Per-broker distinguishing colors for Realtime Quote Path chart (RFC §87: no green=buy/red=sell)
    pub broker_colors: [Color32; 8],
    /// Consensus / Broker Median line color
    pub median_line: Color32,
}

impl Default for ChartTheme {
    fn default() -> Self {
        Self {
            bg_color: crate::ui::style::BACKGROUND,
            grid_color: Color32::from_rgba_unmultiplied(175, 195, 220, 24),
            // Neutral broker-identity colors (RFC §87: no green=buy/red=sell semantics)
            candle_up_a: COLOR_OANDA,
            candle_down_a: dim_candle_color(COLOR_OANDA),
            candle_up_b: COLOR_TRADEVIEW,
            candle_down_b: dim_candle_color(COLOR_TRADEVIEW),
            diff_line: crate::ui::style::WARNING,        // Gold for mid diff
            bid_diff_line: Color32::from_rgb(0, 191, 255), // Deep Sky Blue for bid diff
            ask_diff_line: Color32::from_rgb(255, 105, 180), // Hot Pink for ask diff
            spread_diff_line: Color32::from_rgb(175, 125, 255), // Light Purple for spread diff
            zero_line: Color32::from_rgba_unmultiplied(255, 255, 255, 72),
            // Default broker colors corresponding to default.toml brokers
            // (0: OANDA, 1: Tradeview, 2: Dukascopy, 3: Axiory, 4: JFX)
            broker_colors: [
                COLOR_OANDA,     // #00D37E OANDA
                COLOR_TRADEVIEW, // #E53935 Tradeview
                COLOR_DUKASCOPY, // #3B82F6 Dukascopy
                COLOR_AXIORY,    // #8A29D5 Axiory
                COLOR_JFX,       // #A8C92B JFX
                Color32::from_rgb(255, 130, 170), // Pink
                Color32::from_rgb(255, 220, 80),  // Gold
                Color32::from_rgb(200, 200, 200), // Silver
            ],
            median_line: Color32::from_rgba_unmultiplied(255, 255, 255, 180),
        }
    }
}

pub const fn broker_color_for(theme: &ChartTheme, index: usize) -> Color32 {
    theme.broker_colors[index % theme.broker_colors.len()]
}

/// Match broker brand identity color based on broker name.
pub fn broker_color_by_name(name: &str) -> Option<Color32> {
    let lower = name.to_lowercase();
    if lower.contains("oanda") {
        Some(COLOR_OANDA)
    } else if lower.contains("axiory") {
        Some(COLOR_AXIORY)
    } else if lower.contains("tradeview") {
        Some(COLOR_TRADEVIEW)
    } else if lower.contains("jfx") {
        Some(COLOR_JFX)
    } else if lower.contains("dukascopy") || lower.contains("dukas") {
        Some(COLOR_DUKASCOPY)
    } else {
        None
    }
}

/// Resolve broker color prioritizing broker name match, falling back to theme palette index.
pub fn broker_color_for_name(theme: &ChartTheme, name: Option<&str>, index: usize) -> Color32 {
    if let Some(n) = name {
        if let Some(c) = broker_color_by_name(n) {
            return c;
        }
    }
    broker_color_for(theme, index)
}

pub fn dim_color(color: Color32, alpha: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), alpha)
}

pub fn dim_candle_color(color: Color32) -> Color32 {
    Color32::from_rgba_unmultiplied(color.r() / 2, color.g() / 2, color.b() / 2, color.a())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_broker_brand_colors() {
        assert_eq!(broker_color_by_name("OANDA"), Some(COLOR_OANDA));
        assert_eq!(broker_color_by_name("Axiory"), Some(COLOR_AXIORY));
        assert_eq!(broker_color_by_name("Tradeview"), Some(COLOR_TRADEVIEW));
        assert_eq!(broker_color_by_name("JFX"), Some(COLOR_JFX));
        assert_eq!(broker_color_by_name("Dukascopy"), Some(COLOR_DUKASCOPY));
        assert_eq!(broker_color_by_name("Unknown"), None);

        let theme = ChartTheme::default();
        assert_eq!(broker_color_for_name(&theme, Some("OANDA"), 0), COLOR_OANDA);
        assert_eq!(broker_color_for_name(&theme, Some("Axiory"), 0), COLOR_AXIORY);
        assert_eq!(broker_color_for_name(&theme, Some("Unknown"), 1), COLOR_TRADEVIEW);
    }
}
