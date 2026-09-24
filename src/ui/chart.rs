use crate::contracts::models::{
    BrokerOverview, CandleView, DiffPoint, MoveDirection, MoveQuality, Ohlc, PairComparison,
    RealtimeQuotePoint, SlotState,
};
use crate::contracts::types::{BrokerId, ConnectionState, FreshnessState, MonoNs};
use crate::metrics::{EventCluster, MoveBreadth, ObservedBrokerConsensus, StageLatencySummary};
use egui::{Color32, Pos2, Rect, Stroke};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BottomMetric {
    #[default]
    MidDiff,
    BidAskDiff,
    SpreadDiff,
    LeadLag,
    MidDispersion,
    MoveBreadthView,
    QuotePersistence,
}

impl BottomMetric {
    pub const ALL: [BottomMetric; 7] = [
        BottomMetric::MidDiff,
        BottomMetric::BidAskDiff,
        BottomMetric::SpreadDiff,
        BottomMetric::LeadLag,
        BottomMetric::MidDispersion,
        BottomMetric::MoveBreadthView,
        BottomMetric::QuotePersistence,
    ];

    pub fn key_number(&self) -> u32 {
        match self {
            BottomMetric::MidDiff => 1,
            BottomMetric::BidAskDiff => 2,
            BottomMetric::SpreadDiff => 3,
            BottomMetric::LeadLag => 4,
            BottomMetric::MidDispersion => 5,
            BottomMetric::MoveBreadthView => 6,
            BottomMetric::QuotePersistence => 7,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            BottomMetric::MidDiff => "1: Mid Diff",
            BottomMetric::BidAskDiff => "2: Bid/Ask Diff",
            BottomMetric::SpreadDiff => "3: Spread Diff",
            BottomMetric::LeadLag => "4: Lead/Lag",
            BottomMetric::MidDispersion => "5: Dispersion",
            BottomMetric::MoveBreadthView => "6: Breadth",
            BottomMetric::QuotePersistence => "7: Persistence",
        }
    }

    pub fn title(&self) -> &'static str {
        match self {
            BottomMetric::MidDiff => "Mid Price Difference (A - B)",
            BottomMetric::BidAskDiff => "Bid & Ask Difference (A - B)",
            BottomMetric::SpreadDiff => "Spread Difference (A - B)",
            BottomMetric::LeadLag => "Lead / Lag Diagnostics",
            BottomMetric::MidDispersion => "Mid Dispersion (Deviation from Broker Median)",
            BottomMetric::MoveBreadthView => "Directional Move Breadth",
            BottomMetric::QuotePersistence => "Quote Freshness / Age per Broker",
        }
    }

    pub fn next(&self) -> Self {
        match self {
            BottomMetric::MidDiff => BottomMetric::BidAskDiff,
            BottomMetric::BidAskDiff => BottomMetric::SpreadDiff,
            BottomMetric::SpreadDiff => BottomMetric::LeadLag,
            BottomMetric::LeadLag => BottomMetric::MidDispersion,
            BottomMetric::MidDispersion => BottomMetric::MoveBreadthView,
            BottomMetric::MoveBreadthView => BottomMetric::QuotePersistence,
            BottomMetric::QuotePersistence => BottomMetric::MidDiff,
        }
    }

    pub fn prev(&self) -> Self {
        match self {
            BottomMetric::MidDiff => BottomMetric::QuotePersistence,
            BottomMetric::BidAskDiff => BottomMetric::MidDiff,
            BottomMetric::SpreadDiff => BottomMetric::BidAskDiff,
            BottomMetric::LeadLag => BottomMetric::SpreadDiff,
            BottomMetric::MidDispersion => BottomMetric::LeadLag,
            BottomMetric::MoveBreadthView => BottomMetric::MidDispersion,
            BottomMetric::QuotePersistence => BottomMetric::MoveBreadthView,
        }
    }

    pub fn from_key_number(n: u32) -> Option<Self> {
        match n {
            1 => Some(BottomMetric::MidDiff),
            2 => Some(BottomMetric::BidAskDiff),
            3 => Some(BottomMetric::SpreadDiff),
            4 => Some(BottomMetric::LeadLag),
            5 => Some(BottomMetric::MidDispersion),
            6 => Some(BottomMetric::MoveBreadthView),
            7 => Some(BottomMetric::QuotePersistence),
            _ => None,
        }
    }
}

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
            bg_color: Color32::from_rgb(20, 24, 30),
            grid_color: Color32::from_rgba_unmultiplied(255, 255, 255, 28),
            // Neutral broker-identity colors (RFC §87: no green=buy/red=sell semantics)
            candle_up_a: Color32::from_rgb(80, 180, 220), // Light cyan for A (bright)
            candle_down_a: Color32::from_rgb(40, 100, 140), // Dark cyan for A (dim)
            candle_up_b: Color32::from_rgb(255, 165, 80), // Light orange for B (bright)
            candle_down_b: Color32::from_rgb(160, 100, 40), // Dark orange for B (dim)
            diff_line: Color32::from_rgb(255, 215, 0),    // Gold for mid diff
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

pub fn draw_candlestick_chart(
    painter: &egui::Painter,
    rect: Rect,
    candle_view: Option<&CandleView>,
    broker_a: BrokerId,
    broker_b: BrokerId,
    fallback_price: Option<f64>,
    theme: &ChartTheme,
) {
    let broker_ids = [broker_a, broker_b];
    draw_candlestick_chart_for_brokers(
        painter,
        rect,
        candle_view,
        &broker_ids,
        &[],
        fallback_price,
        theme,
    );
}

const CHART_HEADER_HEIGHT: f32 = 40.0;
const CHART_FOOTER_HEIGHT: f32 = 18.0;
const PRICE_AXIS_WIDTH: f32 = 88.0;

fn chart_plot_rect(rect: Rect, header_height: f32) -> Rect {
    let left = rect.left() + 2.0;
    let right = (rect.right() - PRICE_AXIS_WIDTH).max(left + 1.0);
    let top = (rect.top() + header_height).min(rect.bottom() - 1.0);
    let bottom = (rect.bottom() - CHART_FOOTER_HEIGHT).max(top + 1.0);
    Rect::from_min_max(Pos2::new(left, top), Pos2::new(right, bottom))
}

fn dim_color(color: Color32, alpha: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), alpha)
}

fn is_live(broker: &BrokerOverview) -> bool {
    broker.health.connection == ConnectionState::Connected
        && broker.health.data_freshness == FreshnessState::Live
}

fn format_price_delta(value: f64) -> String {
    format!("{value:+.5}")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ChartXAxisMode {
    #[default]
    ReceiveTime,
    TickCount,
}

impl ChartXAxisMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::ReceiveTime => "Receive time",
            Self::TickCount => "Tick count",
        }
    }
}

/// Draw context candles for every configured broker on a shared time and
/// price axis. Broker identity is encoded by hue; candle direction is encoded
/// by brightness so colors do not imply buy or sell semantics.
pub fn draw_candlestick_chart_multi(
    painter: &egui::Painter,
    rect: Rect,
    candle_view: Option<&CandleView>,
    broker_overviews: &[BrokerOverview],
    fallback_price: Option<f64>,
    theme: &ChartTheme,
) {
    let broker_ids: Vec<BrokerId> = broker_overviews.iter().map(|b| b.broker_id).collect();
    draw_candlestick_chart_for_brokers(
        painter,
        rect,
        candle_view,
        &broker_ids,
        broker_overviews,
        fallback_price,
        theme,
    );
}

fn draw_candlestick_chart_for_brokers(
    painter: &egui::Painter,
    rect: Rect,
    candle_view: Option<&CandleView>,
    broker_ids: &[BrokerId],
    broker_overviews: &[BrokerOverview],
    fallback_price: Option<f64>,
    theme: &ChartTheme,
) {
    painter.rect_filled(rect, 4.0, theme.bg_color);
    let plot_rect = chart_plot_rect(rect, CHART_HEADER_HEIGHT);

    let view = match candle_view {
        Some(v) if !v.slot_starts.is_empty() => v,
        _ => {
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "Waiting for Candle Data...",
                egui::FontId::proportional(14.0),
                Color32::GRAY,
            );
            return;
        }
    };

    let broker_ids: Vec<BrokerId> = broker_ids
        .iter()
        .copied()
        .filter(|id| view.slots_by_broker.contains_key(id))
        .collect();
    if broker_ids.is_empty() {
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "Waiting for Broker Candle Data...",
            egui::FontId::proportional(14.0),
            Color32::GRAY,
        );
        return;
    }

    let num_slots = view.slot_starts.len();
    let slot_width = plot_rect.width() / (num_slots as f32).max(1.0);

    // Find global min and max prices across broker A and B
    let mut min_price = f64::MAX;
    let mut max_price = f64::MIN;

    for broker_id in &broker_ids {
        if let Some(slots) = view.slots_by_broker.get(broker_id) {
            for s in slots {
                if let Some(ohlc) = &s.ohlc {
                    min_price = min_price.min(ohlc.low);
                    max_price = max_price.max(ohlc.high);
                }
            }
        }
    }

    let has_ohlc = min_price <= max_price && min_price < f64::MAX;

    let (chart_min, chart_max) = if has_ohlc {
        if (max_price - min_price).abs() < 1e-5 {
            // Single price (High == Low): add a reasonable margin (e.g. ±0.025)
            let margin = (min_price * 0.0005).max(0.025);
            (min_price - margin, max_price + margin)
        } else {
            // Add 10% padding
            let price_padding = (max_price - min_price) * 0.1;
            (min_price - price_padding, max_price + price_padding)
        }
    } else if let Some(fb) = fallback_price {
        // No candle data yet, but have current quote: center around quote
        let margin = (fb * 0.0005).max(0.025);
        (fb - margin, fb + margin)
    } else {
        // Neither candle data nor quote available
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "Waiting for Tick Data...",
            egui::FontId::proportional(14.0),
            Color32::GRAY,
        );
        return;
    };

    let price_range = (chart_max - chart_min).max(0.0001);

    let price_to_y = |p: f64| -> f32 {
        let normalized = (chart_max - p) / price_range;
        plot_rect.top() + (normalized as f32) * plot_rect.height()
    };

    // Draw horizontal price grid lines
    let grid_steps = 4;
    for i in 0..=grid_steps {
        let p = chart_min + (price_range / grid_steps as f64) * i as f64;
        let y = price_to_y(p);
        painter.line_segment(
            [
                Pos2::new(plot_rect.left(), y),
                Pos2::new(plot_rect.right(), y),
            ],
            Stroke::new(1.0_f32, theme.grid_color),
        );
        painter.text(
            Pos2::new(rect.right() - 4.0, y - 2.0),
            egui::Align2::RIGHT_BOTTOM,
            format!("{:.3}", p),
            egui::FontId::monospace(11.0),
            Color32::from_gray(180),
        );
    }

    if !has_ohlc {
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "Waiting for Candle Data in current window...\n(Ensure broker UTC offset is verified)",
            egui::FontId::proportional(13.0),
            Color32::from_gray(140),
        );
    }

    // Draw one candle band per broker inside each time slot. Keep a small
    // explicit gap so adjacent broker candles remain visually distinct.
    let broker_count = broker_ids.len() as f32;
    let candle_gap = 1.0_f32;
    let total_gap = candle_gap * (broker_count - 1.0).max(0.0);
    let bar_width = ((slot_width * 0.82 - total_gap) / broker_count).clamp(1.5, 12.0);
    let candle_group_width = bar_width * broker_count + total_gap;

    for (i, _) in view.slot_starts.iter().enumerate() {
        let slot_center_x = plot_rect.left() + (i as f32 + 0.5) * slot_width;

        for (broker_index, broker_id) in broker_ids.iter().enumerate() {
            if let Some(s) = view
                .slots_by_broker
                .get(broker_id)
                .and_then(|slots| slots.get(i))
            {
                if s.state != SlotState::Empty {
                    if let Some(ohlc) = &s.ohlc {
                        let group_left = slot_center_x - candle_group_width * 0.5;
                        let cx = group_left
                            + broker_index as f32 * (bar_width + candle_gap)
                            + bar_width * 0.5;
                        let color = broker_color_for(theme, broker_index);
                        draw_single_candle(
                            painter,
                            cx,
                            bar_width,
                            ohlc,
                            price_to_y,
                            color,
                            dim_candle_color(color),
                        );
                    }
                }
            }
        }
    }

    // Keep the mapping visible when multiple brokers overlap at the same
    // price. The legend uses broker identity colors only.
    let mut legend_x = rect.left() + 8.0;
    let mut legend_y = rect.top() + 6.0;
    let mut hidden_legends = 0;
    for (broker_index, broker_id) in broker_ids.iter().enumerate() {
        let color = broker_color_for(theme, broker_index);
        let name = broker_overviews
            .iter()
            .find(|b| b.broker_id == *broker_id)
            .map(|b| b.name.as_str())
            .unwrap_or("Broker");
        let label = format!("{} [{}]", name, broker_id);
        let width = 8.0 + label.len() as f32 * 7.0 + 12.0;
        if legend_x + width > rect.right() - 8.0 {
            legend_x = rect.left() + 8.0;
            legend_y += 14.0;
        }
        if legend_y + 12.0 > plot_rect.top() {
            hidden_legends += 1;
            continue;
        }
        painter.rect_filled(
            Rect::from_min_size(
                Pos2::new(legend_x, legend_y + 2.0),
                egui::Vec2::new(7.0, 7.0),
            ),
            1.0,
            color,
        );
        painter.text(
            Pos2::new(legend_x + 11.0, legend_y),
            egui::Align2::LEFT_TOP,
            label,
            egui::FontId::monospace(10.0),
            color,
        );
        legend_x += width;
    }
    if hidden_legends > 0 {
        painter.text(
            Pos2::new(rect.right() - PRICE_AXIS_WIDTH - 6.0, rect.top() + 20.0),
            egui::Align2::RIGHT_TOP,
            format!("+{} brokers", hidden_legends),
            egui::FontId::monospace(11.0),
            Color32::from_gray(180),
        );
    }
}

fn dim_candle_color(color: Color32) -> Color32 {
    Color32::from_rgba_unmultiplied(color.r() / 2, color.g() / 2, color.b() / 2, color.a())
}

fn draw_single_candle<F>(
    painter: &egui::Painter,
    cx: f32,
    bar_width: f32,
    ohlc: &Ohlc,
    price_to_y: F,
    up_color: Color32,
    down_color: Color32,
) where
    F: Fn(f64) -> f32,
{
    let y_open = price_to_y(ohlc.open);
    let y_close = price_to_y(ohlc.close);
    let y_high = price_to_y(ohlc.high);
    let y_low = price_to_y(ohlc.low);

    let is_up = ohlc.close >= ohlc.open;
    let color = if is_up { up_color } else { down_color };

    // Wick
    painter.line_segment(
        [Pos2::new(cx, y_high), Pos2::new(cx, y_low)],
        Stroke::new(1.0_f32, color),
    );

    // Body
    let top_body = y_open.min(y_close);
    let bottom_body = y_open.max(y_close).max(top_body + 1.0);
    let body_rect = Rect::from_min_max(
        Pos2::new(cx - bar_width * 0.5, top_body),
        Pos2::new(cx + bar_width * 0.5, bottom_body),
    );
    painter.rect_filled(body_rect, 1.0, color);
}

pub fn draw_difference_chart(
    painter: &egui::Painter,
    rect: Rect,
    series: &[DiffPoint],
    x_axis_mode: ChartXAxisMode,
    now_mono: MonoNs,
    visible_seconds: u64,
    visible_ticks: usize,
    theme: &ChartTheme,
) {
    draw_mid_diff_chart(
        painter,
        rect,
        series,
        x_axis_mode,
        now_mono,
        visible_seconds,
        visible_ticks,
        theme,
    );
}

fn visible_diff_extreme<F>(
    series: &[DiffPoint],
    x_axis_mode: ChartXAxisMode,
    plot_rect: Rect,
    now_mono: MonoNs,
    visible_seconds: u64,
    visible_ticks: usize,
    minimum: f64,
    value: F,
) -> f64
where
    F: Fn(&DiffPoint) -> f64,
{
    series
        .iter()
        .enumerate()
        .filter(|(index, point)| {
            x_axis_coordinate(
                x_axis_mode,
                plot_rect,
                *index,
                series.len(),
                point.mono_ns,
                now_mono,
                visible_seconds,
                visible_ticks,
            )
            .is_some()
        })
        .map(|(_, point)| value(point).abs())
        .filter(|value| value.is_finite())
        .fold(minimum, f64::max)
}

fn draw_diff_scale(
    painter: &egui::Painter,
    rect: Rect,
    plot_rect: Rect,
    extreme: f64,
    theme: &ChartTheme,
) {
    for value in [extreme, 0.0, -extreme] {
        let y = plot_rect.top() + ((extreme - value) / (2.0 * extreme)) as f32 * plot_rect.height();
        let stroke = if value == 0.0 {
            Stroke::new(1.0_f32, theme.zero_line)
        } else {
            Stroke::new(1.0_f32, theme.grid_color)
        };
        painter.line_segment(
            [
                Pos2::new(plot_rect.left(), y),
                Pos2::new(plot_rect.right(), y),
            ],
            stroke,
        );
        let label = if value == 0.0 {
            "0.00000 price".to_owned()
        } else {
            format!("{} price", format_price_delta(value))
        };
        painter.text(
            Pos2::new(rect.right() - 4.0, y),
            egui::Align2::RIGHT_CENTER,
            label,
            egui::FontId::monospace(11.0),
            Color32::from_gray(185),
        );
    }
}

pub fn draw_mid_diff_chart(
    painter: &egui::Painter,
    rect: Rect,
    series: &[DiffPoint],
    x_axis_mode: ChartXAxisMode,
    now_mono: MonoNs,
    visible_seconds: u64,
    visible_ticks: usize,
    theme: &ChartTheme,
) {
    painter.rect_filled(rect, 4.0, theme.bg_color);

    if series.is_empty() {
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "No difference data yet",
            egui::FontId::proportional(13.0),
            Color32::GRAY,
        );
        return;
    }

    let plot_rect = chart_plot_rect(rect, 26.0);
    let extreme = visible_diff_extreme(
        series,
        x_axis_mode,
        plot_rect,
        now_mono,
        visible_seconds,
        visible_ticks,
        0.005,
        |point| point.mid_diff,
    );
    let chart_min = -extreme;
    let chart_max = extreme;
    let range = chart_max - chart_min;

    let diff_to_y = |d: f64| -> f32 {
        let norm = (chart_max - d) / range;
        plot_rect.top() + (norm as f32) * plot_rect.height()
    };
    draw_diff_scale(painter, rect, plot_rect, extreme, theme);

    // Latest badge
    if let Some(last) = series.last() {
        let label = format!("Mid A - B: {} price", format_price_delta(last.mid_diff));
        painter.text(
            Pos2::new(plot_rect.right() - 4.0, rect.top() + 6.0),
            egui::Align2::RIGHT_TOP,
            label,
            egui::FontId::monospace(12.0),
            theme.diff_line,
        );
    }

    draw_x_axis_caption(painter, rect, x_axis_mode, visible_seconds, visible_ticks);
    let mut previous = None;
    for (index, pt) in series.iter().enumerate() {
        if let Some(x) = x_axis_coordinate(
            x_axis_mode,
            plot_rect,
            index,
            series.len(),
            pt.mono_ns,
            now_mono,
            visible_seconds,
            visible_ticks,
        ) {
            let position = Pos2::new(x, diff_to_y(pt.mid_diff));
            if let Some(previous) = previous {
                painter.line_segment([previous, position], Stroke::new(1.5_f32, theme.diff_line));
            }
            previous = Some(position);
        }
    }
}

pub fn draw_bid_ask_diff_chart(
    painter: &egui::Painter,
    rect: Rect,
    series: &[DiffPoint],
    x_axis_mode: ChartXAxisMode,
    now_mono: MonoNs,
    visible_seconds: u64,
    visible_ticks: usize,
    theme: &ChartTheme,
) {
    painter.rect_filled(rect, 4.0, theme.bg_color);

    if series.is_empty() {
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "No difference data yet",
            egui::FontId::proportional(13.0),
            Color32::GRAY,
        );
        return;
    }

    let plot_rect = chart_plot_rect(rect, 26.0);
    let extreme = visible_diff_extreme(
        series,
        x_axis_mode,
        plot_rect,
        now_mono,
        visible_seconds,
        visible_ticks,
        0.005,
        |point| point.bid_diff.abs().max(point.ask_diff.abs()),
    );
    let chart_min = -extreme;
    let chart_max = extreme;
    let range = chart_max - chart_min;

    let diff_to_y = |d: f64| -> f32 {
        let norm = (chart_max - d) / range;
        plot_rect.top() + (norm as f32) * plot_rect.height()
    };
    draw_diff_scale(painter, rect, plot_rect, extreme, theme);

    // Legend & latest values
    if let Some(last) = series.last() {
        let text = format!(
            "Bid A - B: {}  Ask A - B: {} price",
            format_price_delta(last.bid_diff),
            format_price_delta(last.ask_diff)
        );
        painter.text(
            Pos2::new(plot_rect.right() - 4.0, rect.top() + 6.0),
            egui::Align2::RIGHT_TOP,
            text,
            egui::FontId::monospace(12.0),
            Color32::WHITE,
        );
    }

    draw_x_axis_caption(painter, rect, x_axis_mode, visible_seconds, visible_ticks);
    let mut previous_bid = None;
    let mut previous_ask = None;
    for (index, pt) in series.iter().enumerate() {
        if let Some(x) = x_axis_coordinate(
            x_axis_mode,
            plot_rect,
            index,
            series.len(),
            pt.mono_ns,
            now_mono,
            visible_seconds,
            visible_ticks,
        ) {
            let position = Pos2::new(x, diff_to_y(pt.bid_diff));
            if let Some(previous) = previous_bid {
                painter.line_segment(
                    [previous, position],
                    Stroke::new(1.5_f32, theme.bid_diff_line),
                );
            }
            previous_bid = Some(position);
        }
        if let Some(x) = x_axis_coordinate(
            x_axis_mode,
            plot_rect,
            index,
            series.len(),
            pt.mono_ns,
            now_mono,
            visible_seconds,
            visible_ticks,
        ) {
            let position = Pos2::new(x, diff_to_y(pt.ask_diff));
            if let Some(previous) = previous_ask {
                painter.line_segment(
                    [previous, position],
                    Stroke::new(1.5_f32, theme.ask_diff_line),
                );
            }
            previous_ask = Some(position);
        }
    }
}

pub fn draw_spread_diff_chart(
    painter: &egui::Painter,
    rect: Rect,
    series: &[DiffPoint],
    x_axis_mode: ChartXAxisMode,
    now_mono: MonoNs,
    visible_seconds: u64,
    visible_ticks: usize,
    theme: &ChartTheme,
) {
    painter.rect_filled(rect, 4.0, theme.bg_color);

    if series.is_empty() {
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "No spread difference data yet",
            egui::FontId::proportional(13.0),
            Color32::GRAY,
        );
        return;
    }

    let plot_rect = chart_plot_rect(rect, 26.0);
    let extreme = visible_diff_extreme(
        series,
        x_axis_mode,
        plot_rect,
        now_mono,
        visible_seconds,
        visible_ticks,
        0.002,
        |point| point.spread_diff,
    );
    let chart_min = -extreme;
    let chart_max = extreme;
    let range = chart_max - chart_min;

    let diff_to_y = |d: f64| -> f32 {
        let norm = (chart_max - d) / range;
        plot_rect.top() + (norm as f32) * plot_rect.height()
    };
    draw_diff_scale(painter, rect, plot_rect, extreme, theme);

    // Latest badge
    if let Some(last) = series.last() {
        let status = if last.spread_diff > 0.0001 {
            "(A is wider)"
        } else if last.spread_diff < -0.0001 {
            "(B is wider)"
        } else {
            "(Equal)"
        };
        let label = format!(
            "Spread A - B: {} price {}",
            format_price_delta(last.spread_diff),
            status
        );
        painter.text(
            Pos2::new(plot_rect.right() - 4.0, rect.top() + 6.0),
            egui::Align2::RIGHT_TOP,
            label,
            egui::FontId::monospace(12.0),
            theme.spread_diff_line,
        );
    }

    draw_x_axis_caption(painter, rect, x_axis_mode, visible_seconds, visible_ticks);
    let mut previous = None;
    for (index, pt) in series.iter().enumerate() {
        if let Some(x) = x_axis_coordinate(
            x_axis_mode,
            plot_rect,
            index,
            series.len(),
            pt.mono_ns,
            now_mono,
            visible_seconds,
            visible_ticks,
        ) {
            let position = Pos2::new(x, diff_to_y(pt.spread_diff));
            if let Some(previous) = previous {
                painter.line_segment(
                    [previous, position],
                    Stroke::new(1.5_f32, theme.spread_diff_line),
                );
            }
            previous = Some(position);
        }
    }
}

fn fixed_time_window(now_mono: MonoNs, visible_seconds: u64) -> (u64, u64) {
    let span_ns = visible_seconds.max(1).saturating_mul(1_000_000_000);
    (now_mono.0.saturating_sub(span_ns), now_mono.0)
}

fn x_axis_coordinate(
    mode: ChartXAxisMode,
    rect: Rect,
    sample_index: usize,
    sample_count: usize,
    mono_ns: MonoNs,
    now_mono: MonoNs,
    visible_seconds: u64,
    visible_ticks: usize,
) -> Option<f32> {
    match mode {
        ChartXAxisMode::ReceiveTime => {
            let (start_ns, end_ns) = fixed_time_window(now_mono, visible_seconds);
            if mono_ns.0 < start_ns || mono_ns.0 > end_ns {
                return None;
            }
            let span_ns = end_ns.saturating_sub(start_ns).max(1);
            Some(
                rect.left()
                    + (mono_ns.0.saturating_sub(start_ns) as f64 / span_ns as f64) as f32
                        * rect.width(),
            )
        }
        ChartXAxisMode::TickCount => {
            let slots = visible_ticks.max(1);
            let first_visible = sample_count.saturating_sub(slots);
            if sample_index < first_visible {
                return None;
            }
            if slots == 1 {
                return Some(rect.right());
            }
            let shown_count = sample_count - first_visible;
            let slot_index = slots - shown_count + sample_index - first_visible;
            Some(rect.left() + slot_index as f32 / (slots - 1) as f32 * rect.width())
        }
    }
}

fn draw_x_axis_caption(
    painter: &egui::Painter,
    rect: Rect,
    mode: ChartXAxisMode,
    visible_seconds: u64,
    visible_ticks: usize,
) {
    let caption = match mode {
        ChartXAxisMode::ReceiveTime => {
            format!("Receive time · fixed {} s window", visible_seconds.max(1))
        }
        ChartXAxisMode::TickCount => format!("Tick count · fixed {} updates", visible_ticks.max(1)),
    };
    painter.text(
        Pos2::new(rect.left() + 6.0, rect.bottom() - 3.0),
        egui::Align2::LEFT_BOTTOM,
        caption,
        egui::FontId::monospace(11.0),
        Color32::from_gray(170),
    );
}

pub fn draw_lead_lag_view(
    painter: &egui::Painter,
    rect: Rect,
    comparison: Option<&PairComparison>,
    broker_overviews: &[BrokerOverview],
    theme: &ChartTheme,
) {
    painter.rect_filled(rect, 4.0, theme.bg_color);

    let comp = match comparison {
        Some(c) => c,
        None => {
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "No active pair comparison available",
                egui::FontId::proportional(13.0),
                Color32::GRAY,
            );
            return;
        }
    };

    let name_of = |bid: BrokerId| -> String {
        broker_overviews
            .iter()
            .find(|b| b.broker_id == bid)
            .map(|b| b.name.clone())
            .unwrap_or_else(|| format!("Broker #{}", bid))
    };

    let broker_a_name = name_of(comp.broker_a);
    let broker_b_name = name_of(comp.broker_b);

    if let Some(m) = &comp.latest_match {
        let leader_name = name_of(m.leader);
        let follower_name = name_of(m.follower);

        let ema_text = comp
            .ema_lead_lag_ms
            .map(|e| format!("  |  EMA Lead: {:+.1} ms", e))
            .unwrap_or_default();

        let header_text = format!(
            "First observed on this PC: {} ({:.1} ms{})",
            leader_name,
            m.raw_delta_ms.abs(),
            ema_text
        );

        painter.text(
            Pos2::new(rect.left() + 16.0, rect.top() + 14.0),
            egui::Align2::LEFT_TOP,
            header_text,
            egui::FontId::proportional(16.0),
            Color32::from_rgb(255, 215, 0),
        );

        // Visual bar showing relative lead direction
        let bar_center_y = rect.top() + 50.0;
        let bar_center_x = rect.center().x;
        let max_bar_half_width = rect.width() * 0.35;

        // Base line
        painter.line_segment(
            [
                Pos2::new(bar_center_x - max_bar_half_width, bar_center_y),
                Pos2::new(bar_center_x + max_bar_half_width, bar_center_y),
            ],
            Stroke::new(2.0_f32, Color32::from_gray(60)),
        );

        // Center tick
        painter.line_segment(
            [
                Pos2::new(bar_center_x, bar_center_y - 8.0),
                Pos2::new(bar_center_x, bar_center_y + 8.0),
            ],
            Stroke::new(2.0_f32, Color32::from_gray(140)),
        );

        // Labels for A (Left) and B (Right)
        painter.text(
            Pos2::new(bar_center_x - max_bar_half_width - 8.0, bar_center_y),
            egui::Align2::RIGHT_CENTER,
            format!("{} (A)", broker_a_name),
            egui::FontId::proportional(12.0),
            if m.leader == comp.broker_a {
                theme.candle_up_a
            } else {
                Color32::GRAY
            },
        );

        painter.text(
            Pos2::new(bar_center_x + max_bar_half_width + 8.0, bar_center_y),
            egui::Align2::LEFT_CENTER,
            format!("{} (B)", broker_b_name),
            egui::FontId::proportional(12.0),
            if m.leader == comp.broker_b {
                theme.candle_up_b
            } else {
                Color32::GRAY
            },
        );

        // Bar indicator
        let bar_len = ((m.raw_delta_ms.abs() / 100.0) as f32 * max_bar_half_width)
            .clamp(8.0, max_bar_half_width);
        let (bar_rect, bar_color) = if m.leader == comp.broker_a {
            (
                Rect::from_min_max(
                    Pos2::new(bar_center_x - bar_len, bar_center_y - 6.0),
                    Pos2::new(bar_center_x, bar_center_y + 6.0),
                ),
                theme.candle_up_a,
            )
        } else {
            (
                Rect::from_min_max(
                    Pos2::new(bar_center_x, bar_center_y - 6.0),
                    Pos2::new(bar_center_x + bar_len, bar_center_y + 6.0),
                ),
                theme.candle_up_b,
            )
        };
        painter.rect_filled(bar_rect, 2.0, bar_color);

        // Event detail section
        let dir_str = match m.leader_event.direction {
            MoveDirection::Up => "UP",
            MoveDirection::Down => "DOWN",
        };
        let quality_str = match m.leader_event.quality {
            MoveQuality::BothSides => "BothSides",
            MoveQuality::BidOnly => "BidOnly",
            MoveQuality::AskOnly => "AskOnly",
            MoveQuality::SpreadDriven => "SpreadDriven",
        };

        let details = format!(
            "Match #{}: {} followed {} | Delta: {:.1} ms | Dir: {} | Quality: {} | ΔMid: {:.1} pts",
            m.match_id,
            follower_name,
            leader_name,
            m.raw_delta_ms.abs(),
            dir_str,
            quality_str,
            m.leader_event.mid_delta_points
        );

        painter.text(
            Pos2::new(rect.left() + 16.0, rect.top() + 75.0),
            egui::Align2::LEFT_TOP,
            details,
            egui::FontId::monospace(12.0),
            Color32::from_gray(180),
        );
    } else {
        let ema_info = comp
            .ema_lead_lag_ms
            .map(|e| format!("Current EMA Lead/Lag: {:+.1} ms\n", e))
            .unwrap_or_default();

        let msg = format!(
            "{}Waiting for significant price moves between {} and {}...",
            ema_info, broker_a_name, broker_b_name
        );

        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            msg,
            egui::FontId::proportional(13.0),
            Color32::GRAY,
        );
    }
}

/// Helper: get broker color from theme by index
pub fn broker_color_for(theme: &ChartTheme, index: usize) -> Color32 {
    theme.broker_colors[index % theme.broker_colors.len()]
}

fn advance_chart_anchor(
    chart_anchor: &mut Option<f64>,
    center_price: f64,
    half_span: f64,
    deadzone_pct: f64,
) -> f64 {
    match chart_anchor {
        Some(anchor) => {
            let delta = center_price - *anchor;
            let deadzone_half = half_span * (1.0 - deadzone_pct.clamp(0.0, 0.95));
            let required_shift = (delta.abs() - deadzone_half).max(0.0) * delta.signum();
            // Move at most 12% of the visible range per repaint. This keeps a
            // large price move visible without replacing the whole chart at once.
            let max_step = half_span * 0.12;
            *anchor += required_shift.clamp(-max_step, max_step);
            *anchor
        }
        None => {
            *chart_anchor = Some(center_price);
            center_price
        }
    }
}

fn price_decimals_for_pip(pip_size: f64) -> usize {
    if !pip_size.is_finite() || pip_size <= 0.0 {
        return 3;
    }
    ((-pip_size.log10()).round() as i32 + 1).clamp(3, 6) as usize
}

/// Realtime Quote Path Chart (RFC §23, §24, §60)
pub fn draw_realtime_quote_path_chart(
    painter: &egui::Painter,
    rect: Rect,
    quote_points: &[RealtimeQuotePoint],
    broker_overviews: &[BrokerOverview],
    selected_pair: (BrokerId, BrokerId),
    x_axis_mode: ChartXAxisMode,
    now_mono: MonoNs,
    visible_seconds: u64,
    visible_ticks: usize,
    pip_size: f64,
    fixed_follow_span_pips: f64,
    deadzone_pct: f64,
    chart_anchor: &mut Option<f64>,
    theme: &ChartTheme,
) {
    painter.rect_filled(rect, 4.0, theme.bg_color);

    if quote_points.is_empty() {
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "Waiting for Realtime Quote Data...",
            egui::FontId::proportional(14.0),
            Color32::GRAY,
        );
        return;
    }

    let latest_mid = quote_points.last().and_then(|p| {
        p.consensus_mid.or_else(|| {
            broker_overviews
                .iter()
                .find_map(|b| p.broker_mids.get(&b.broker_id).copied())
        })
    });

    let center_price = match latest_mid {
        Some(m) => m,
        None => return,
    };

    let pip_size = pip_size.max(f64::EPSILON);
    let half_span = (fixed_follow_span_pips * pip_size / 2.0).max(f64::EPSILON);
    let anchor = advance_chart_anchor(chart_anchor, center_price, half_span, deadzone_pct);

    let chart_min = anchor - half_span;
    let chart_max = anchor + half_span;
    let price_range = chart_max - chart_min;
    let plot_rect = chart_plot_rect(rect, CHART_HEADER_HEIGHT);
    let price_decimals = price_decimals_for_pip(pip_size);

    let price_to_y = |p: f64| -> f32 {
        let normalized = (chart_max - p) / price_range;
        plot_rect.top() + (normalized as f32) * plot_rect.height()
    };

    // Grid lines
    let grid_steps = 5;
    for i in 0..=grid_steps {
        let p = chart_min + (price_range / grid_steps as f64) * i as f64;
        let y = price_to_y(p);
        painter.line_segment(
            [
                Pos2::new(plot_rect.left(), y),
                Pos2::new(plot_rect.right(), y),
            ],
            Stroke::new(1.0_f32, theme.grid_color),
        );
        painter.text(
            Pos2::new(rect.right() - 4.0, y - 2.0),
            egui::Align2::RIGHT_BOTTOM,
            format!("{:.*}", price_decimals, p),
            egui::FontId::monospace(11.0),
            Color32::from_gray(185),
        );
    }

    // Header and plot have separate rectangles, so neither the legend nor the
    // price axis obscures the latest part of the line.
    painter.text(
        Pos2::new(rect.left() + 6.0, rect.top() + 4.0),
        egui::Align2::LEFT_TOP,
        format!("Follow: ±{:.1} pip", price_range / pip_size / 2.0),
        egui::FontId::monospace(11.0),
        Color32::from_gray(190),
    );

    draw_x_axis_caption(painter, rect, x_axis_mode, visible_seconds, visible_ticks);

    let mut ordered_brokers: Vec<(usize, &BrokerOverview)> =
        broker_overviews.iter().enumerate().collect();
    ordered_brokers.sort_by_key(|(_, broker)| {
        if broker.broker_id == selected_pair.0 || broker.broker_id == selected_pair.1 {
            1
        } else {
            0
        }
    });

    let mut legend_x = rect.left() + 172.0;
    let mut legend_y = rect.top() + 5.0;
    let mut hidden_legends = 0;
    for (index, broker) in &ordered_brokers {
        let selected = broker.broker_id == selected_pair.0 || broker.broker_id == selected_pair.1;
        let role = if broker.broker_id == selected_pair.0 {
            "A"
        } else if broker.broker_id == selected_pair.1 {
            "B"
        } else {
            "·"
        };
        let quote = broker
            .latest_quote
            .as_ref()
            .map(|quote| format!("{:.*}", price_decimals, quote.mid))
            .unwrap_or_else(|| "--".to_owned());
        let availability = match broker.health.connection {
            ConnectionState::Disconnected => " DISCONNECTED",
            ConnectionState::Connecting => " CONNECTING",
            ConnectionState::Connected => match broker.health.data_freshness {
                FreshnessState::Live => "",
                FreshnessState::Stale => " STALE",
                FreshnessState::Unknown => " WARMING",
            },
        };
        let label = format!("{} {} {}{}", role, broker.name, quote, availability);
        let width = 10.0 + label.chars().count() as f32 * 7.0;
        if legend_x + width > plot_rect.right() - 4.0 {
            legend_x = rect.left() + 172.0;
            legend_y += 15.0;
        }
        if legend_y + 12.0 > plot_rect.top() {
            hidden_legends += 1;
            continue;
        }
        let base_color = broker_color_for(theme, *index);
        painter.line_segment(
            [
                Pos2::new(legend_x, legend_y + 6.0),
                Pos2::new(legend_x + 8.0, legend_y + 6.0),
            ],
            Stroke::new(
                if selected { 2.5_f32 } else { 1.0_f32 },
                if selected {
                    base_color
                } else {
                    dim_color(base_color, 130)
                },
            ),
        );
        painter.text(
            Pos2::new(legend_x + 11.0, legend_y),
            egui::Align2::LEFT_TOP,
            label,
            egui::FontId::monospace(if selected { 11.0 } else { 10.0 }),
            if selected {
                base_color
            } else {
                dim_color(base_color, 150)
            },
        );
        legend_x += width;
    }
    if hidden_legends > 0 {
        painter.text(
            Pos2::new(plot_rect.right() - 4.0, rect.top() + 21.0),
            egui::Align2::RIGHT_TOP,
            format!("+{} brokers", hidden_legends),
            egui::FontId::monospace(11.0),
            Color32::from_gray(180),
        );
    }

    // Draw background brokers first. The selected pair is drawn last with a
    // stronger stroke, so it remains readable where prices overlap.
    for (index, broker) in &ordered_brokers {
        let selected = broker.broker_id == selected_pair.0 || broker.broker_id == selected_pair.1;
        let color = broker_color_for(theme, *index);
        let line_color = if selected {
            color
        } else {
            dim_color(color, if is_live(broker) { 105 } else { 55 })
        };
        let stroke_width = if selected { 2.5_f32 } else { 1.0_f32 };
        let mut previous = None;
        for (sample_index, pt) in quote_points.iter().enumerate() {
            let current = pt
                .broker_mids
                .get(&broker.broker_id)
                .filter(|m| m.is_finite())
                .and_then(|&mid| {
                    x_axis_coordinate(
                        x_axis_mode,
                        plot_rect,
                        sample_index,
                        quote_points.len(),
                        pt.mono_ns,
                        now_mono,
                        visible_seconds,
                        visible_ticks,
                    )
                    .map(|x| Pos2::new(x, price_to_y(mid)))
                });
            if let (Some(a), Some(b)) = (previous, current) {
                painter.line_segment([a, b], Stroke::new(stroke_width, line_color));
            }
            previous = current;
        }
        if let Some(point) = previous {
            painter.circle_filled(point, if selected { 3.5 } else { 2.0 }, line_color);
        }
    }

    // Draw Broker Median (consensus_mid) as one continuous line.
    //
    // Do not derive a dash pattern from the sample index here. The realtime
    // history is a bounded deque, so once it is full, dropping the oldest
    // sample shifts every remaining index and makes the median appear to
    // flicker even when the data itself is unchanged.
    {
        let mut previous = None;
        for (index, pt) in quote_points.iter().enumerate() {
            let current = pt.consensus_mid.filter(|m| m.is_finite()).and_then(|mid| {
                x_axis_coordinate(
                    x_axis_mode,
                    plot_rect,
                    index,
                    quote_points.len(),
                    pt.mono_ns,
                    now_mono,
                    visible_seconds,
                    visible_ticks,
                )
                .map(|x| Pos2::new(x, price_to_y(mid)))
            });
            if let (Some(a), Some(b)) = (previous, current) {
                painter.line_segment(
                    [a, b],
                    Stroke::new(1.5_f32, dim_color(theme.median_line, 145)),
                );
            }
            previous = current;
        }
    }
}

/// One-Line State Ribbon (RFC §57)
pub fn draw_state_ribbon(
    ui: &mut egui::Ui,
    brokers: &[BrokerOverview],
    consensus: &Option<ObservedBrokerConsensus>,
    clusters: &[EventCluster],
    breadth: &Option<MoveBreadth>,
) {
    ui.horizontal(|ui| {
        let live_count = brokers
            .iter()
            .filter(|b| {
                b.health.connection == ConnectionState::Connected
                    && b.health.data_freshness == FreshnessState::Live
            })
            .count();
        let overloaded = brokers.iter().any(|b| {
            let flags = b.health.overload;
            flags.receiver || flags.engine || flags.logger || flags.analysis
        });
        let (global_status, global_color) = if overloaded {
            ("OVERLOAD", Color32::RED)
        } else if !brokers.is_empty() && live_count == brokers.len() {
            ("SYSTEM_OK", Color32::GREEN)
        } else if live_count > 0 {
            ("PARTIAL", Color32::YELLOW)
        } else {
            ("DEGRADED", Color32::RED)
        };
        ui.colored_label(global_color, global_status);
        ui.separator();
        if let Some(c) = consensus {
            let fresh_color = if c.fresh_count == c.total_count {
                Color32::from_rgb(0, 200, 160)
            } else {
                Color32::from_rgb(255, 200, 80)
            };
            ui.colored_label(
                fresh_color,
                format!("Fresh {}/{}", c.fresh_count, c.total_count),
            );
            ui.separator();
            if let Some(median) = c.consensus_mid {
                ui.label(format!("Observed Broker Median {:.3}", median));
                ui.separator();
            }
            if let Some(range) = c.mid_range {
                ui.label(format!("Range {:.3}", range));
            }
            ui.separator();
        }
        if let Some(cluster) = clusters.last() {
            let dir = match cluster.direction {
                MoveDirection::Up => "UP",
                MoveDirection::Down => "DOWN",
            };
            ui.label(format!(
                "{} Cluster {}/{} {:.0}ms",
                dir,
                cluster.participating_brokers.len(),
                cluster.total_brokers,
                cluster.observed_span_ms
            ));
            ui.separator();
        }
        if let Some(b) = breadth {
            if b.up_count > 0 || b.down_count > 0 {
                ui.label(format!(
                    "Breadth UP {} DN {}",
                    b.up_ratio_str(),
                    b.down_ratio_str()
                ));
            } else {
                ui.label("Breadth: Quiet");
            }
        }
    });
}

/// Mid Dispersion View (RFC §26, §61)
pub fn draw_mid_dispersion_view(
    painter: &egui::Painter,
    rect: Rect,
    consensus: &Option<ObservedBrokerConsensus>,
    broker_overviews: &[BrokerOverview],
    theme: &ChartTheme,
) {
    painter.rect_filled(rect, 4.0, theme.bg_color);
    let cons = match consensus {
        Some(c) => c,
        None => {
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "No consensus data yet",
                egui::FontId::proportional(13.0),
                Color32::GRAY,
            );
            return;
        }
    };
    let median = match cons.consensus_mid {
        Some(m) => m,
        None => {
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "Insufficient fresh brokers",
                egui::FontId::proportional(13.0),
                Color32::GRAY,
            );
            return;
        }
    };

    painter.text(
        Pos2::new(rect.left() + 8.0, rect.top() + 6.0),
        egui::Align2::LEFT_TOP,
        format!(
            "Observed Broker Median: {:.3}  |  Fresh: {}/{}  |  Range: {:.1}pt",
            median,
            cons.fresh_count,
            cons.total_count,
            cons.mid_range.unwrap_or(0.0) * 1000.0
        ),
        egui::FontId::monospace(11.0),
        Color32::WHITE,
    );

    if let Some(mad) = cons.median_abs_deviation {
        painter.text(
            Pos2::new(rect.right() - 8.0, rect.top() + 6.0),
            egui::Align2::RIGHT_TOP,
            format!("MAD: {:.2}pt", mad * 1000.0),
            egui::FontId::monospace(11.0),
            Color32::from_rgb(200, 180, 255),
        );
    }

    let bar_top = rect.top() + 26.0;
    let bar_height = (rect.height() - 32.0).max(20.0);
    let n = broker_overviews.len().max(1);
    let row_h = (bar_height / n as f32).min(22.0);
    let cx = rect.center().x;
    let max_half = rect.width() * 0.35;
    let max_dev = broker_overviews
        .iter()
        .filter(|b| {
            b.health.connection == ConnectionState::Connected
                && b.health.data_freshness == FreshnessState::Live
        })
        .filter_map(|b| b.latest_quote.as_ref().map(|q| (q.mid - median).abs()))
        .fold(0.001_f64, f64::max);

    painter.line_segment(
        [Pos2::new(cx, bar_top), Pos2::new(cx, bar_top + bar_height)],
        Stroke::new(1.0_f32, theme.zero_line),
    );

    for (i, b) in broker_overviews.iter().enumerate() {
        let y = bar_top + (i as f32) * row_h + row_h * 0.5;
        let color = broker_color_for(theme, i);
        painter.text(
            Pos2::new(rect.left() + 8.0, y),
            egui::Align2::LEFT_CENTER,
            &b.name,
            egui::FontId::monospace(11.0),
            color,
        );

        if b.health.connection != ConnectionState::Connected
            || b.health.data_freshness != FreshnessState::Live
        {
            painter.text(
                Pos2::new(rect.right() - 8.0, y),
                egui::Align2::RIGHT_CENTER,
                if b.health.connection == ConnectionState::Disconnected {
                    "DISCONNECTED"
                } else {
                    "STALE / WARMING"
                },
                egui::FontId::monospace(10.0),
                Color32::GRAY,
            );
            continue;
        }
        if let Some(q) = &b.latest_quote {
            let dev = q.mid - median;
            let bar_len = ((dev.abs() / max_dev) as f32 * max_half).clamp(2.0, max_half);
            let br = if dev >= 0.0 {
                Rect::from_min_max(Pos2::new(cx, y - 4.0), Pos2::new(cx + bar_len, y + 4.0))
            } else {
                Rect::from_min_max(Pos2::new(cx - bar_len, y - 4.0), Pos2::new(cx, y + 4.0))
            };
            let is_outlier = cons.outliers.iter().any(|o| o.broker_id == b.broker_id);
            painter.rect_filled(
                br,
                2.0,
                if is_outlier {
                    Color32::from_rgb(255, 100, 100)
                } else {
                    color
                },
            );
            let (lx, al) = if dev >= 0.0 {
                (br.right() + 4.0, egui::Align2::LEFT_CENTER)
            } else {
                (br.left() - 4.0, egui::Align2::RIGHT_CENTER)
            };
            painter.text(
                Pos2::new(lx, y),
                al,
                format!("{:+.1}pt", dev * 1000.0),
                egui::FontId::monospace(10.0),
                Color32::from_gray(180),
            );
            if is_outlier {
                painter.text(
                    Pos2::new(rect.right() - 8.0, y),
                    egui::Align2::RIGHT_CENTER,
                    "Large Deviation",
                    egui::FontId::monospace(9.0),
                    Color32::from_rgb(255, 140, 100),
                );
            }
        }
    }
}

/// Move Breadth View (RFC §42, §61)
pub fn draw_move_breadth_view(
    painter: &egui::Painter,
    rect: Rect,
    breadth: &Option<MoveBreadth>,
    clusters: &[EventCluster],
    broker_overviews: &[BrokerOverview],
    theme: &ChartTheme,
) {
    painter.rect_filled(rect, 4.0, theme.bg_color);
    painter.text(
        Pos2::new(rect.left() + 8.0, rect.top() + 6.0),
        egui::Align2::LEFT_TOP,
        "Directional Move Breadth",
        egui::FontId::monospace(12.0),
        Color32::WHITE,
    );

    let y0 = rect.top() + 28.0;
    if let Some(b) = breadth {
        let total = b.total_brokers.max(1);
        let bw = rect.width() * 0.6;
        let bl = rect.left() + rect.width() * 0.25;

        let up_f = b.up_count as f32 / total as f32;
        painter.text(
            Pos2::new(bl - 8.0, y0 + 4.0),
            egui::Align2::RIGHT_CENTER,
            format!("UP {}", b.up_ratio_str()),
            egui::FontId::monospace(11.0),
            Color32::from_rgb(80, 200, 220),
        );
        painter.rect_filled(
            Rect::from_min_size(Pos2::new(bl, y0), egui::Vec2::new(bw * up_f, 12.0)),
            2.0,
            Color32::from_rgb(80, 200, 220),
        );

        let dn_f = b.down_count as f32 / total as f32;
        let dy = y0 + 20.0;
        painter.text(
            Pos2::new(bl - 8.0, dy + 4.0),
            egui::Align2::RIGHT_CENTER,
            format!("DN {}", b.down_ratio_str()),
            egui::FontId::monospace(11.0),
            Color32::from_rgb(255, 160, 80),
        );
        painter.rect_filled(
            Rect::from_min_size(Pos2::new(bl, dy), egui::Vec2::new(bw * dn_f, 12.0)),
            2.0,
            Color32::from_rgb(255, 160, 80),
        );
    } else {
        painter.text(
            Pos2::new(rect.center().x, y0 + 10.0),
            egui::Align2::CENTER_TOP,
            "No breadth data yet",
            egui::FontId::proportional(13.0),
            Color32::GRAY,
        );
    }

    let cy = y0 + 55.0;
    if !clusters.is_empty() {
        painter.text(
            Pos2::new(rect.left() + 8.0, cy),
            egui::Align2::LEFT_TOP,
            "Recent Clusters:",
            egui::FontId::monospace(11.0),
            Color32::from_gray(160),
        );
        let name_of = |bid: BrokerId| -> String {
            broker_overviews
                .iter()
                .find(|b| b.broker_id == bid)
                .map(|b| b.name.clone())
                .unwrap_or_else(|| format!("#{}", bid))
        };
        for (i, cl) in clusters.iter().rev().take(3).enumerate() {
            let d = match cl.direction {
                MoveDirection::Up => "UP",
                MoveDirection::Down => "DN",
            };
            painter.text(
                Pos2::new(rect.left() + 16.0, cy + 16.0 + (i as f32) * 14.0),
                egui::Align2::LEFT_TOP,
                format!(
                    "{} {}/{} {:.0}ms First:{} Last:{}",
                    d,
                    cl.participating_brokers.len(),
                    cl.total_brokers,
                    cl.observed_span_ms,
                    name_of(cl.first_observed),
                    name_of(cl.last_observed)
                ),
                egui::FontId::monospace(10.0),
                Color32::from_gray(140),
            );
        }
    }
}

/// Quote Persistence View (RFC §45, §61)
pub fn draw_quote_persistence_view(
    painter: &egui::Painter,
    rect: Rect,
    broker_overviews: &[BrokerOverview],
    theme: &ChartTheme,
) {
    painter.rect_filled(rect, 4.0, theme.bg_color);
    painter.text(
        Pos2::new(rect.left() + 8.0, rect.top() + 6.0),
        egui::Align2::LEFT_TOP,
        "Quote Freshness / Tick Rate per Broker",
        egui::FontId::monospace(12.0),
        Color32::WHITE,
    );

    if broker_overviews.is_empty() {
        return;
    }

    let y0 = rect.top() + 28.0;
    let rh = ((rect.height() - 36.0) / broker_overviews.len() as f32).min(24.0);
    let bl = rect.left() + rect.width() * 0.2;
    let bw = rect.width() * 0.55;
    let max_rate = broker_overviews
        .iter()
        .map(|b| b.tick_rate_1s)
        .fold(1.0_f64, f64::max);

    for (i, b) in broker_overviews.iter().enumerate() {
        let y = y0 + (i as f32) * rh + rh * 0.5;
        let color = broker_color_for(theme, i);
        painter.text(
            Pos2::new(rect.left() + 8.0, y),
            egui::Align2::LEFT_CENTER,
            &b.name,
            egui::FontId::monospace(11.0),
            color,
        );
        let frac = (b.tick_rate_1s / max_rate) as f32;
        let br = Rect::from_min_size(Pos2::new(bl, y - 5.0), egui::Vec2::new(bw * frac, 10.0));
        painter.rect_filled(br, 2.0, color);
        painter.text(
            Pos2::new(br.right() + 6.0, y),
            egui::Align2::LEFT_CENTER,
            format!("{:.0} t/s", b.tick_rate_1s),
            egui::FontId::monospace(10.0),
            Color32::from_gray(180),
        );
        if let Some(q) = &b.latest_quote {
            painter.text(
                Pos2::new(rect.right() - 8.0, y),
                egui::Align2::RIGHT_CENTER,
                format!("Spread: {:.3}", q.spread),
                egui::FontId::monospace(10.0),
                Color32::from_gray(140),
            );
        }
    }
}

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
            ui.label(format!("  {}: No samples", name));
        }
    };
    draw_stage(ui, "Tick→Engine", &summary.tick_to_engine);
    draw_stage(ui, "Engine→Proj", &summary.engine_to_projection);
    draw_stage(ui, "Proj→Snap", &summary.projection_to_snapshot);
    draw_stage(ui, "Snap→UI", &summary.snapshot_to_ui);
    draw_stage(ui, "Total", &summary.total_pipeline);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn follow_anchor_limits_a_large_recenter_to_one_small_step() {
        let mut anchor = Some(100.0);
        let next = advance_chart_anchor(&mut anchor, 120.0, 10.0, 0.4);

        assert!((next - 101.2).abs() < f64::EPSILON);
        assert_eq!(anchor, Some(next));
    }

    #[test]
    fn visible_difference_scale_ignores_points_outside_the_time_window() {
        let series = [
            DiffPoint {
                mono_ns: MonoNs(1),
                bid_diff: 5.0,
                ask_diff: 5.0,
                mid_diff: 5.0,
                spread_diff: 5.0,
            },
            DiffPoint {
                mono_ns: MonoNs(60_000_000_000),
                bid_diff: 0.01,
                ask_diff: 0.01,
                mid_diff: 0.01,
                spread_diff: 0.01,
            },
        ];
        let plot_rect = Rect::from_min_size(Pos2::ZERO, egui::Vec2::new(400.0, 200.0));
        let extreme = visible_diff_extreme(
            &series,
            ChartXAxisMode::ReceiveTime,
            plot_rect,
            MonoNs(60_000_000_000),
            10,
            100,
            0.005,
            |point| point.mid_diff,
        );

        assert_eq!(extreme, 0.01);
    }
}
