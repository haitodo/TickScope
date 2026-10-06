//! Bottom panel microstructure indicators, diagnostics, and metrics.

pub mod breadth;
pub mod dispersion;
pub mod lead_lag;
pub mod persistence;

pub use breadth::*;
pub use dispersion::*;
pub use lead_lag::*;
pub use persistence::*;

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
}

impl BottomMetric {
    pub const ALL: [Self; 8] = [
        Self::QuotePath,
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
            Self::MidDiff => 2,
            Self::BidAskDiff => 3,
            Self::SpreadDiff => 4,
            Self::LeadLag => 5,
            Self::MidDispersion => 6,
            Self::MoveBreadthView => 7,
            Self::QuotePersistence => 8,
            Self::QuotePath => 1,
        }
    }

    #[must_use]
    pub const fn label(&self) -> &'static str {
        match self {
            Self::MidDiff => "2: Mid Diff",
            Self::BidAskDiff => "3: Bid/Ask Diff",
            Self::SpreadDiff => "4: Spread Diff",
            Self::LeadLag => "5: Lead/Lag",
            Self::MidDispersion => "6: Dispersion",
            Self::MoveBreadthView => "7: Breadth",
            Self::QuotePersistence => "8: Persistence",
            Self::QuotePath => "1: Quote Path",
        }
    }

    #[must_use]
    pub const fn title(&self) -> &'static str {
        match self {
            Self::MidDiff => "Mid Price Difference (A - B)",
            Self::BidAskDiff => "Bid & Ask Difference (A - B)",
            Self::SpreadDiff => "Spread Difference (A - B)",
            Self::LeadLag => "Lead / Lag Diagnostics",
            Self::MidDispersion => "Mid Dispersion (Deviation from Broker Median)",
            Self::MoveBreadthView => "Directional Move Breadth",
            Self::QuotePersistence => "Quote Freshness / Age per Broker",
            Self::QuotePath => "Realtime Multi-Broker Quote Path",
        }
    }

    #[must_use]
    pub const fn next(&self) -> Self {
        match self {
            Self::MidDiff => Self::BidAskDiff,
            Self::BidAskDiff => Self::SpreadDiff,
            Self::SpreadDiff => Self::LeadLag,
            Self::LeadLag => Self::MidDispersion,
            Self::MidDispersion => Self::MoveBreadthView,
            Self::MoveBreadthView => Self::QuotePersistence,
            Self::QuotePersistence => Self::QuotePath,
            Self::QuotePath => Self::MidDiff,
        }
    }

    #[must_use]
    pub const fn prev(&self) -> Self {
        match self {
            Self::MidDiff => Self::QuotePath,
            Self::BidAskDiff => Self::MidDiff,
            Self::SpreadDiff => Self::BidAskDiff,
            Self::LeadLag => Self::SpreadDiff,
            Self::MidDispersion => Self::LeadLag,
            Self::MoveBreadthView => Self::MidDispersion,
            Self::QuotePersistence => Self::MoveBreadthView,
            Self::QuotePath => Self::QuotePersistence,
        }
    }

    #[must_use]
    pub const fn from_key_number(n: u32) -> Option<Self> {
        match n {
            2 => Some(Self::MidDiff),
            3 => Some(Self::BidAskDiff),
            4 => Some(Self::SpreadDiff),
            5 => Some(Self::LeadLag),
            6 => Some(Self::MidDispersion),
            7 => Some(Self::MoveBreadthView),
            8 => Some(Self::QuotePersistence),
            1 => Some(Self::QuotePath),
            _ => None,
        }
    }

    #[must_use]
    pub const fn category(&self) -> BottomMetricCategory {
        match self {
            Self::MidDiff | Self::BidAskDiff | Self::SpreadDiff | Self::LeadLag => {
                BottomMetricCategory::PairDiff
            }
            Self::MidDispersion | Self::MoveBreadthView | Self::QuotePersistence => {
                BottomMetricCategory::MarketConsensus
            }
            Self::QuotePath => BottomMetricCategory::RawQuotes,
        }
    }

    #[must_use]
    pub const fn is_pair_metric(&self) -> bool {
        matches!(self.category(), BottomMetricCategory::PairDiff)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BottomMetricCategory {
    PairDiff,
    MarketConsensus,
    RawQuotes,
}

impl BottomMetricCategory {
    pub const ALL: [Self; 3] = [
        Self::RawQuotes,
        Self::PairDiff,
        Self::MarketConsensus,
    ];

    #[must_use]
    pub const fn title(&self) -> &'static str {
        match self {
            Self::PairDiff => "Pair Differentials (2社比較)",
            Self::MarketConsensus => "Market Consensus (市場統計)",
            Self::RawQuotes => "Raw Quotes / Ticks (リアルタイム価格)",
        }
    }

    #[must_use]
    pub const fn metrics(&self) -> &'static [BottomMetric] {
        match self {
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
            Self::RawQuotes => &[BottomMetric::QuotePath],
        }
    }
}
