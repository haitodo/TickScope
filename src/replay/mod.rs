//! Historical tick replay module and WebSocket synchronization.
//!
//! This module provides the `VirtualClock`, multi-broker Hive Parquet reader,
//! 5-broker k-way merge stream, and WebSocket sync with `TickReplay` (`ws://127.0.0.1:49210`).
//!
//! Note: Gated under `#[cfg(feature = "replay")]` to guarantee ZERO overhead
//! in standard production builds.

pub mod arbiter;
pub mod clock;
pub mod coordinator;
pub mod driver;
pub mod merge_stream;
pub mod parquet_source;
pub mod pump;
pub mod rebuilder;
pub mod sync_client;

pub use arbiter::{ArbiterAction, SyncArbiter};
pub use clock::VirtualClock;
pub use coordinator::ReplayCoordinator;
pub use driver::{mt5_to_utc_ms, ReplayDriver, DEFAULT_SYNC_URL};
pub use merge_stream::MergeStream;
pub use parquet_source::{BrokerParquetSource, ReplayTick};
pub use pump::PlaybackPump;
pub use rebuilder::StateRebuilder;
pub use sync_client::WsSyncClient;
