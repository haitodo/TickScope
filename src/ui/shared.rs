//! Helpers shared by several UI panels: broker lookups and compact value formatting.
//!
//! These exist because the same three-line chains were copy-pasted across the chart
//! and dashboard modules; keeping one copy means a change to the lookup rule (for
//! example "match by name when the id is absent") only has to happen once.

use crate::core::models::BrokerOverview;
use crate::core::types::BrokerId;

/// Name of the broker with `id` in `overviews`, or `fallback` when it is not listed.
#[must_use]
pub fn broker_name<'a>(
    overviews: &'a [BrokerOverview],
    id: BrokerId,
    fallback: &'a str,
) -> &'a str {
    overviews
        .iter()
        .find(|b| b.broker_id == id)
        .map_or(fallback, |b| b.name.as_str())
}

/// Position of the broker with `id` in `overviews`, or `fallback` when it is not listed.
#[must_use]
pub fn broker_index(overviews: &[BrokerOverview], id: BrokerId, fallback: usize) -> usize {
    overviews
        .iter()
        .position(|b| b.broker_id == id)
        .unwrap_or(fallback)
}

/// Formats a pip count with the fewest decimals that keep it exact, followed by `unit`.
///
/// Callers pass `" pips"`, `"p"` or `""`; the numeric part is the same for all of them,
/// so the rounding rule lives here instead of in each label.
#[must_use]
pub fn format_pips(pips: f64, unit: &str) -> String {
    if (pips.fract()).abs() < 1e-4 {
        format!("{pips:.0}{unit}")
    } else {
        format!("{pips:.1}{unit}")
    }
}
