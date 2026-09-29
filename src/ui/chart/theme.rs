use egui::Color32;

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
            candle_up_a: Color32::from_rgb(80, 180, 220), // Light cyan for A (bright)
            candle_down_a: Color32::from_rgb(40, 100, 140), // Dark cyan for A (dim)
            candle_up_b: Color32::from_rgb(255, 165, 80), // Light orange for B (bright)
            candle_down_b: Color32::from_rgb(160, 100, 40), // Dark orange for B (dim)
            diff_line: crate::ui::style::WARNING,        // Gold for mid diff
            bid_diff_line: Color32::from_rgb(0, 191, 255), // Deep Sky Blue for bid diff
            ask_diff_line: Color32::from_rgb(255, 105, 180), // Hot Pink for ask diff
            spread_diff_line: Color32::from_rgb(175, 125, 255), // Light Purple for spread diff
            zero_line: Color32::from_rgba_unmultiplied(255, 255, 255, 72),
            // Per-broker identity colors for Realtime Quote Path
            broker_colors: [
                Color32::from_rgb(0, 200, 255),   // Cyan
                Color32::from_rgb(255, 165, 0),   // Orange
                Color32::from_rgb(180, 120, 255), // Purple
                Color32::from_rgb(0, 200, 160),   // Teal
                Color32::from_rgb(255, 130, 170), // Pink
                Color32::from_rgb(255, 220, 80),  // Gold
                Color32::from_rgb(120, 200, 120), // Soft green (not buy-signal)
                Color32::from_rgb(200, 200, 200), // Silver
            ],
            median_line: Color32::from_rgba_unmultiplied(255, 255, 255, 180),
        }
    }
}

pub fn broker_color_for(theme: &ChartTheme, index: usize) -> Color32 {
    theme.broker_colors[index % theme.broker_colors.len()]
}

pub fn dim_color(color: Color32, alpha: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), alpha)
}
