//! Shared helpers for headless UI unit tests.
//!
//! `#[cfg(test)]`-only: unit tests inside `src/ui/**` cannot use `tests/support`
//! because that is a separate crate, so the few setups that genuinely repeat are
//! collected here instead.

use crate::core::models::UiSnapshot;
use crate::state::snapshot::SnapshotExchange;
use crate::ui::dashboard::DashboardApp;
use std::sync::Arc;

/// A dashboard app wired to an empty snapshot and the `(1, 2)` broker pair.
///
/// Tests that only exercise dashboard state (pair selection, panel toggles,
/// visibility flags) do not care about the snapshot contents, so they should use
/// this instead of rebuilding the exchange by hand.
pub(crate) fn headless_app() -> DashboardApp {
    DashboardApp::new(
        Arc::new(SnapshotExchange::new(Arc::new(UiSnapshot::default()))),
        (1, 2),
    )
}
