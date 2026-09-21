//! UI dashboard headless tests: T-S02.

use std::collections::HashMap;
use std::sync::Arc;
use tick_compare::contracts::models::*;
use tick_compare::contracts::types::*;
use tick_compare::state::snapshot::SnapshotExchange;
use tick_compare::ui::dashboard::DashboardApp;

#[test]
fn test_ts02_ui_headless_render() {
    let run_id = RunId([1u8; 16]);
    let initial_snap = Arc::new(UiSnapshot {
        schema_revision: 1,
        snapshot_revision: 1,
        projection_revision: 1,
        run_id,
        built_mono_ns: MonoNs(100_000_000),
        processed_watermark_ns: MonoNs(100_000_000),
        display_now_utc: UtcMs(1000),
        active_pair: (1, 2),
        broker_overviews: vec![
            BrokerOverview {
                broker_id: 1,
                name: "OANDA".to_string(),
                symbol: "USDJPY".to_string(),
                latest_quote: Some(Quote {
                    tick_id: TickId { broker_id: 1, session_id: 1, sequence: 1 },
                    bid: 155.000,
                    ask: 155.003,
                    mid: 155.0015,
                    spread: 0.003,
                    rx_mono_ns: MonoNs(100_000_000),
                    utc_ms: Some(UtcMs(1000)),
                    is_warmup: false,
                    is_valid: true,
                }),
                min_spread: Some(0.002),
                max_spread: Some(0.005),
                health: HealthState::default(),
                tick_rate_1s: 10.0,
            },
            BrokerOverview {
                broker_id: 2,
                name: "Axiory".to_string(),
                symbol: "USDJPY.pro".to_string(),
                latest_quote: Some(Quote {
                    tick_id: TickId { broker_id: 2, session_id: 1, sequence: 1 },
                    bid: 154.998,
                    ask: 155.001,
                    mid: 154.9995,
                    spread: 0.003,
                    rx_mono_ns: MonoNs(100_000_000),
                    utc_ms: Some(UtcMs(1000)),
                    is_warmup: false,
                    is_valid: true,
                }),
                min_spread: Some(0.001),
                max_spread: Some(0.004),
                health: HealthState::default(),
                tick_rate_1s: 12.0,
            },
            BrokerOverview {
                broker_id: 3,
                name: "XM".to_string(),
                symbol: "USDJPY#".to_string(),
                latest_quote: None,
                min_spread: None,
                max_spread: None,
                health: HealthState::default(),
                tick_rate_1s: 0.0,
            },
        ],
        active_pair_comparison: Some(PairComparison {
            broker_a: 1,
            broker_b: 2,
            as_of_mono_ns: MonoNs(100_000_000),
            bid_diff: Some(0.002),
            ask_diff: Some(0.002),
            mid_diff: Some(0.002),
            spread_diff: Some(0.0),
            recent_diff_series: vec![DiffPoint {
                mono_ns: MonoNs(100_000_000),
                bid_diff: 0.002,
                ask_diff: 0.002,
                mid_diff: 0.002,
                spread_diff: 0.0,
            }],
            latest_match: None,
            ema_lead_lag_ms: None,
        }),
        active_candles: Some(CandleView {
            period_ms: 60000,
            slot_starts: vec![UtcMs(0), UtcMs(60000)],
            slots_by_broker: HashMap::new(),
        }),
        diagnostics: Vec::new(),
    });

    let exchange = Arc::new(SnapshotExchange::new(initial_snap));
    let mut app = DashboardApp::new(exchange, (1, 2));

    // Headless egui Context
    let ctx = egui::Context::default();
    let raw_input = egui::RawInput::default();
    let _full_output = ctx.run(raw_input, |ctx| {
        app.render_ui(ctx);
    });

    assert_eq!(app.selected_pair(), (1, 2));
    app.set_selected_pair(1, 3);
    assert_eq!(app.selected_pair(), (1, 3));
}
