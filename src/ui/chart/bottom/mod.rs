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
    pub const ALL: [BottomMetric; 8] = [
        BottomMetric::QuotePath,
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
            BottomMetric::MidDiff => 2,
            BottomMetric::BidAskDiff => 3,
            BottomMetric::SpreadDiff => 4,
            BottomMetric::LeadLag => 5,
            BottomMetric::MidDispersion => 6,
            BottomMetric::MoveBreadthView => 7,
            BottomMetric::QuotePersistence => 8,
            BottomMetric::QuotePath => 1,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            BottomMetric::MidDiff => "2: Mid Diff",
            BottomMetric::BidAskDiff => "3: Bid/Ask Diff",
            BottomMetric::SpreadDiff => "4: Spread Diff",
            BottomMetric::LeadLag => "5: Lead/Lag",
            BottomMetric::MidDispersion => "6: Dispersion",
            BottomMetric::MoveBreadthView => "7: Breadth",
            BottomMetric::QuotePersistence => "8: Persistence",
            BottomMetric::QuotePath => "1: Quote Path",
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
            BottomMetric::QuotePath => "Realtime Multi-Broker Quote Path",
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
            BottomMetric::QuotePersistence => BottomMetric::QuotePath,
            BottomMetric::QuotePath => BottomMetric::MidDiff,
        }
    }

    pub fn prev(&self) -> Self {
        match self {
            BottomMetric::MidDiff => BottomMetric::QuotePath,
            BottomMetric::BidAskDiff => BottomMetric::MidDiff,
            BottomMetric::SpreadDiff => BottomMetric::BidAskDiff,
            BottomMetric::LeadLag => BottomMetric::SpreadDiff,
            BottomMetric::MidDispersion => BottomMetric::LeadLag,
            BottomMetric::MoveBreadthView => BottomMetric::MidDispersion,
            BottomMetric::QuotePersistence => BottomMetric::MoveBreadthView,
            BottomMetric::QuotePath => BottomMetric::QuotePersistence,
        }
    }

    pub fn from_key_number(n: u32) -> Option<Self> {
        match n {
            2 => Some(BottomMetric::MidDiff),
            3 => Some(BottomMetric::BidAskDiff),
            4 => Some(BottomMetric::SpreadDiff),
            5 => Some(BottomMetric::LeadLag),
            6 => Some(BottomMetric::MidDispersion),
            7 => Some(BottomMetric::MoveBreadthView),
            8 => Some(BottomMetric::QuotePersistence),
            1 => Some(BottomMetric::QuotePath),
            _ => None,
        }
    }

    pub fn category(&self) -> BottomMetricCategory {
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

    pub fn is_pair_metric(&self) -> bool {
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
    pub const ALL: [BottomMetricCategory; 3] = [
        BottomMetricCategory::RawQuotes,
        BottomMetricCategory::PairDiff,
        BottomMetricCategory::MarketConsensus,
    ];

    pub fn title(&self) -> &'static str {
        match self {
            Self::PairDiff => "Pair Differentials (2社比較)",
            Self::MarketConsensus => "Market Consensus (市場統計)",
            Self::RawQuotes => "Raw Quotes / Ticks (リアルタイム価格)",
        }
    }

    pub fn metrics(&self) -> &'static [BottomMetric] {
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
