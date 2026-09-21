//! Snapshot tests: T-S01, T-S03.

use std::collections::HashMap;
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use tick_compare::contracts::models::*;
use tick_compare::contracts::ports::SnapshotExchangePort;
use tick_compare::contracts::types::*;
use tick_compare::state::snapshot::{SnapshotBuilder, SnapshotExchange};

#[test]
fn test_ts01_snapshot_builder_and_exchange() {
    let run_id = RunId([9u8; 16]);
    let builder = SnapshotBuilder::new(run_id);
    let exchange = Arc::new(SnapshotExchange::new_empty(run_id));

    let proj = EngineProjection {
        revision: 42,
        watermark_ns: MonoNs(1_000_000_000),
        broker_overviews: vec![
            BrokerOverview {
                broker_id: 1,
                name: "OANDA".to_string(),
                symbol: "USDJPY".to_string(),
                latest_quote: None,
                min_spread: Some(0.002),
                max_spread: Some(0.005),
                health: HealthState::default(),
                tick_rate_1s: 15.0,
            },
            BrokerOverview {
                broker_id: 2,
                name: "Axiory".to_string(),
                symbol: "USDJPY.pro".to_string(),
                latest_quote: None,
                min_spread: Some(0.001),
                max_spread: Some(0.003),
                health: HealthState::default(),
                tick_rate_1s: 20.0,
            },
        ],
        active_pair: (1, 2),
        active_pair_comparison: Some(PairComparison {
            broker_a: 1,
            broker_b: 2,
            as_of_mono_ns: MonoNs(1_000_000_000),
            bid_diff: Some(0.002),
            ask_diff: Some(0.002),
            mid_diff: Some(0.002),
            spread_diff: Some(0.001),
            recent_diff_series: Vec::new(),
            latest_match: None,
            ema_lead_lag_ms: Some(3.5),
        }),
        candle_views: HashMap::new(),
        global_diagnostics: Vec::new(),
    };

    let snapshot = builder.build(&proj, UtcMs(5000), MonoNs(1_000_000_000), 60000);
    assert_eq!(snapshot.snapshot_revision, 1);
    assert_eq!(snapshot.projection_revision, 42);
    assert_eq!(snapshot.broker_overviews.len(), 2);
    assert_eq!(snapshot.active_pair, (1, 2));

    exchange.publish(snapshot);
    let loaded = exchange.load_latest();
    assert_eq!(loaded.snapshot_revision, 1);
    assert_eq!(loaded.active_pair_comparison.as_ref().unwrap().mid_diff, Some(0.002));
}

#[test]
fn test_ts03_concurrent_exchange_latest_wins() {
    let run_id = RunId([1u8; 16]);
    let exchange = Arc::new(SnapshotExchange::new_empty(run_id));
    let ex_clone = exchange.clone();

    // Publisher thread
    let pub_handle = thread::spawn(move || {
        let builder = SnapshotBuilder::new(run_id);
        let mut proj = EngineProjection {
            revision: 0,
            watermark_ns: MonoNs::ZERO,
            broker_overviews: Vec::new(),
            active_pair: (1, 2),
            active_pair_comparison: None,
            candle_views: HashMap::new(),
            global_diagnostics: Vec::new(),
        };

        for i in 1..=50 {
            proj.revision = i;
            let snap = builder.build(&proj, UtcMs(i as i64 * 1000), MonoNs(i * 10_000_000), 60000);
            ex_clone.publish(snap);
            thread::sleep(Duration::from_millis(1));
        }
    });

    // Consumer reads
    let initial_read = exchange.load_latest();
    assert_eq!(initial_read.snapshot_revision, 0);

    pub_handle.join().unwrap();

    let final_read = exchange.load_latest();
    assert_eq!(final_read.snapshot_revision, 50);
    assert_eq!(final_read.projection_revision, 50);

    // Initial read held before thread is unchanged (immutable Arc!)
    assert_eq!(initial_read.snapshot_revision, 0);
}
