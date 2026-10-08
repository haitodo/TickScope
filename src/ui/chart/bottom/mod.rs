//! Bottom panel microstructure indicators, diagnostics, and metrics.

pub mod breadth;
pub mod dispersion;
pub mod lead_lag;
pub mod persistence;
pub mod tick_candle;

pub use breadth::*;
pub use dispersion::*;
pub use lead_lag::*;
pub use persistence::*;
pub use tick_candle::*;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum BottomMetric {
    MidDiff,
    BidAskDiff,
    SpreadDiff,
    LeadLag,
    MidDispersion,
    MoveBreadthView,
    QuotePersistence,
    #[default]
    QuotePath,
    TickCandle,
}

impl BottomMetric {
    pub const ALL: [Self; 9] = [
        Self::QuotePath,
        Self::TickCandle,
        Self::MidDiff,
        Self::BidAskDiff,
        Self::SpreadDiff,
        Self::LeadLag,
        Self::MidDispersion,
        Self::MoveBreadthView,
        Self::QuotePersistence,
    ];

    #[must_use]
    pub const fn key_number(&self) -> u32 {
        match self {
            Self::QuotePath => 1,
            Self::TickCandle => 2,
            Self::MidDiff => 3,
            Self::BidAskDiff => 4,
            Self::SpreadDiff => 5,
            Self::LeadLag => 6,
            Self::MidDispersion => 7,
            Self::MoveBreadthView => 8,
            Self::QuotePersistence => 9,
        }
    }

    #[must_use]
    pub const fn label(&self) -> &'static str {
        match self {
            Self::QuotePath => "1: Quote Path",
            Self::TickCandle => "2: Tick Candle",
            Self::MidDiff => "3: Mid Diff",
            Self::BidAskDiff => "4: Bid/Ask Diff",
            Self::SpreadDiff => "5: Spread Diff",
            Self::LeadLag => "6: Lead/Lag",
            Self::MidDispersion => "7: Dispersion",
            Self::MoveBreadthView => "8: Breadth",
            Self::QuotePersistence => "9: Persistence",
        }
    }

    #[must_use]
    pub const fn title(&self) -> &'static str {
        match self {
            Self::QuotePath => "Realtime Multi-Broker Quote Path",
            Self::TickCandle => "0.2p Noise-Cancelled Tick Candle",
            Self::MidDiff => "Mid Price Difference (A - B)",
            Self::BidAskDiff => "Bid & Ask Difference (A - B)",
            Self::SpreadDiff => "Spread Difference (A - B)",
            Self::LeadLag => "Lead / Lag Diagnostics",
            Self::MidDispersion => "Mid Dispersion (Deviation from Broker Median)",
            Self::MoveBreadthView => "Directional Move Breadth",
            Self::QuotePersistence => "Quote Freshness / Age per Broker",
        }
    }

    #[must_use]
    pub const fn next(&self) -> Self {
        match self {
            Self::QuotePath => Self::TickCandle,
            Self::TickCandle => Self::MidDiff,
            Self::MidDiff => Self::BidAskDiff,
            Self::BidAskDiff => Self::SpreadDiff,
            Self::SpreadDiff => Self::LeadLag,
            Self::LeadLag => Self::MidDispersion,
            Self::MidDispersion => Self::MoveBreadthView,
            Self::MoveBreadthView => Self::QuotePersistence,
            Self::QuotePersistence => Self::QuotePath,
        }
    }

    #[must_use]
    pub const fn prev(&self) -> Self {
        match self {
            Self::QuotePath => Self::QuotePersistence,
            Self::TickCandle => Self::QuotePath,
            Self::MidDiff => Self::TickCandle,
            Self::BidAskDiff => Self::MidDiff,
            Self::SpreadDiff => Self::BidAskDiff,
            Self::LeadLag => Self::SpreadDiff,
            Self::MidDispersion => Self::LeadLag,
            Self::MoveBreadthView => Self::MidDispersion,
            Self::QuotePersistence => Self::MoveBreadthView,
        }
    }

    #[must_use]
    pub const fn from_key_number(n: u32) -> Option<Self> {
        match n {
            1 => Some(Self::QuotePath),
            2 => Some(Self::TickCandle),
            3 => Some(Self::MidDiff),
            4 => Some(Self::BidAskDiff),
            5 => Some(Self::SpreadDiff),
            6 => Some(Self::LeadLag),
            7 => Some(Self::MidDispersion),
            8 => Some(Self::MoveBreadthView),
            9 => Some(Self::QuotePersistence),
            _ => None,
        }
    }

    #[must_use]
    pub const fn category(&self) -> BottomMetricCategory {
        match self {
            Self::QuotePath | Self::TickCandle => BottomMetricCategory::RawQuotes,
            Self::MidDiff | Self::BidAskDiff | Self::SpreadDiff | Self::LeadLag => {
                BottomMetricCategory::PairDiff
            }
            Self::MidDispersion | Self::MoveBreadthView | Self::QuotePersistence => {
                BottomMetricCategory::MarketConsensus
            }
        }
    }

    #[must_use]
    pub const fn is_pair_metric(&self) -> bool {
        matches!(self.category(), BottomMetricCategory::PairDiff)
    }

    #[must_use]
    pub const fn is_single_broker_metric(&self) -> bool {
        matches!(self, Self::TickCandle)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BottomMetricCategory {
    PairDiff,
    MarketConsensus,
    RawQuotes,
}

impl BottomMetricCategory {
    pub const ALL: [Self; 3] = [Self::RawQuotes, Self::PairDiff, Self::MarketConsensus];

    #[must_use]
    pub const fn title(&self) -> &'static str {
        match self {
            Self::RawQuotes => "Raw Quotes & Price Action (リアルタイム価格・足)",
            Self::PairDiff => "Pair Differentials (2社比較)",
            Self::MarketConsensus => "Market Consensus (市場統計)",
        }
    }

    #[must_use]
    pub const fn metrics(&self) -> &'static [BottomMetric] {
        match self {
            Self::RawQuotes => &[BottomMetric::QuotePath, BottomMetric::TickCandle],
            Self::PairDiff => &[
                BottomMetric::MidDiff,
                BottomMetric::BidAskDiff,
                BottomMetric::SpreadDiff,
                BottomMetric::LeadLag,
            ],
            Self::MarketConsensus => &[
                BottomMetric::MidDispersion,
                BottomMetric::MoveBreadthView,
                BottomMetric::QuotePersistence,
            ],
        }
    }
}
