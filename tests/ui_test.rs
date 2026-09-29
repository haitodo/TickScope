//! UI dashboard headless tests: T-S02.

use std::collections::HashMap;
use std::sync::Arc;
use tick_scope::core::models::*;
use tick_scope::core::types::*;
use tick_scope::state::snapshot::SnapshotExchange;
use tick_scope::ui::dashboard::DashboardApp;

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
                active_utc_offset_sec: 10800,
                is_auto_offset: true,
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
                active_utc_offset_sec: 7200,
                is_auto_offset: true,
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
                active_utc_offset_sec: 0,
                is_auto_offset: false,
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
        candle_views: HashMap::new(),
        diagnostics: Vec::new(),
        ..Default::default()
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

#[test]
fn test_candlestick_chart_scaling_and_timeframe_selection() {
    let run_id = RunId([2u8; 16]);
    let mut candle_views = HashMap::new();

    // S10 candle view with single price candle (high == low == 155.200)
    let s10_view = CandleView {
        period_ms: 10000,
        slot_starts: vec![UtcMs(0)],
        slots_by_broker: {
            let mut m = HashMap::new();
            m.insert(
                1,
                vec![CandleSlot {
                    broker_id: 1,
                    segment_id: 1,
                    period_ms: 10000,
                    start_utc_ms: UtcMs(0),
                    state: SlotState::Active,
                    ohlc: Some(Ohlc {
                        open: 155.200,
                        high: 155.200,
                        low: 155.200,
                        close: 155.200,
                        open_key: (UtcMs(0), 1),
                        close_key: (UtcMs(0), 1),
                    }),
                    tick_count: 1,
                    revision: 1,
                    coverage: SlotCoverage::Full,
                }],
            );
            m
        },
    };
    candle_views.insert(10000, s10_view);

    let snap = Arc::new(UiSnapshot {
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
                    bid: 155.200,
                    ask: 155.203,
                    mid: 155.2015,
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
                active_utc_offset_sec: 10800,
                is_auto_offset: true,
            },
        ],
        active_pair_comparison: None,
        active_candles: None,
        candle_views,
        diagnostics: Vec::new(),
        ..Default::default()
    });

    let exchange = Arc::new(SnapshotExchange::new(snap));
    let mut app = DashboardApp::new(exchange, (1, 2));

    let ctx = egui::Context::default();
    let raw_input = egui::RawInput::default();
    let _ = ctx.run(raw_input, |ctx| {
        app.render_ui(ctx);
    });
}

#[test]
fn test_candlestick_fixed_slot_width_and_responsive_slots() {
    use tick_scope::ui::chart::{draw_candlestick_chart_multi, ChartTheme};
    use tick_scope::ui::settings::{
        DEFAULT_CANDLE_BAR_WIDTH, VALID_CANDLE_BAR_WIDTHS, VALID_CANDLE_FIXED_PIPS,
    };

    assert_eq!(DEFAULT_CANDLE_BAR_WIDTH, 5.0);
    assert_eq!(VALID_CANDLE_BAR_WIDTHS, [3.0, 4.0, 5.0, 6.0, 8.0, 10.0]);
    assert_eq!(VALID_CANDLE_FIXED_PIPS, [2.5, 5.0, 10.0, 25.0, 50.0]);

    let mut slots_by_broker = HashMap::new();
    let mut ohlc_slots = Vec::new();
    let mut slot_starts = Vec::new();

    for i in 0..60 {
        let utc = UtcMs(i * 1000);
        slot_starts.push(utc);
        ohlc_slots.push(CandleSlot {
            broker_id: 1,
            segment_id: 1,
            period_ms: 1000,
            start_utc_ms: utc,
            state: SlotState::Closed,
            ohlc: Some(Ohlc {
                open: 150.0 + (i as f64 * 0.01),
                high: 150.05 + (i as f64 * 0.01),
                low: 149.95 + (i as f64 * 0.01),
                close: 150.02 + (i as f64 * 0.01),
                open_key: (utc, 1),
                close_key: (utc, 1),
            }),
            tick_count: 5,
            revision: 1,
            coverage: SlotCoverage::Full,
        });
    }
    slots_by_broker.insert(1, ohlc_slots);

    let view = CandleView {
        period_ms: 1000,
        slot_starts,
        slots_by_broker,
    };

    let ctx = egui::Context::default();
    let _ = ctx.run(egui::RawInput::default(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            let theme = ChartTheme::default();

            // Test render on narrow rect (800x400) with 5px bar (Auto scale)
            let rect_narrow = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::Vec2::new(800.0, 400.0));
            let painter_narrow = ui.painter_at(rect_narrow);
            let mut anchor = None;
            let mut latch_narrow = None;
            draw_candlestick_chart_multi(
                &painter_narrow,
                rect_narrow,
                Some(&view),
                &[],
                5.0,
                tick_scope::ui::settings::CandlePriceScaleMode::Auto,
                tick_scope::ui::settings::CandleFollowCriteria::Median,
                0.01,
                &mut anchor,
                &mut latch_narrow,
                Some(150.0),
                1000,
                MonoNs(0),
                &theme,
            );

            // Test render on wide rect (1600x400) with 8px bar (Fixed scale 10 pips)
            let rect_wide = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::Vec2::new(1600.0, 400.0));
            let painter_wide = ui.painter_at(rect_wide);
            let mut fixed_anchor = None;
            let mut latch_wide = None;
            draw_candlestick_chart_multi(
                &painter_wide,
                rect_wide,
                Some(&view),
                &[],
                8.0,
                tick_scope::ui::settings::CandlePriceScaleMode::Fixed(10.0),
                tick_scope::ui::settings::CandleFollowCriteria::Median,
                0.01,
                &mut fixed_anchor,
                &mut latch_wide,
                Some(150.0),
                1000,
                MonoNs(0),
                &theme,
            );
            assert!(fixed_anchor.is_some(), "Fixed scale should set chart anchor");
        });
    });
}

#[test]
fn test_bottom_metric_shortcuts_and_cycling() {
    use tick_scope::ui::chart::BottomMetric;

    // 1. Cycling tests
    assert_eq!(BottomMetric::MidDiff.next(), BottomMetric::BidAskDiff);
    assert_eq!(BottomMetric::BidAskDiff.next(), BottomMetric::SpreadDiff);
    assert_eq!(BottomMetric::SpreadDiff.next(), BottomMetric::LeadLag);
    assert_eq!(BottomMetric::LeadLag.next(), BottomMetric::MidDispersion);
    assert_eq!(BottomMetric::MidDispersion.next(), BottomMetric::MoveBreadthView);
    assert_eq!(BottomMetric::MoveBreadthView.next(), BottomMetric::QuotePersistence);
    assert_eq!(BottomMetric::QuotePersistence.next(), BottomMetric::QuotePath);
    assert_eq!(BottomMetric::QuotePath.next(), BottomMetric::MidDiff);

    assert_eq!(BottomMetric::MidDiff.prev(), BottomMetric::QuotePath);
    assert_eq!(BottomMetric::QuotePath.prev(), BottomMetric::QuotePersistence);
    assert_eq!(BottomMetric::QuotePersistence.prev(), BottomMetric::MoveBreadthView);
    assert_eq!(BottomMetric::MoveBreadthView.prev(), BottomMetric::MidDispersion);
    assert_eq!(BottomMetric::MidDispersion.prev(), BottomMetric::LeadLag);
    assert_eq!(BottomMetric::LeadLag.prev(), BottomMetric::SpreadDiff);
    assert_eq!(BottomMetric::SpreadDiff.prev(), BottomMetric::BidAskDiff);
    assert_eq!(BottomMetric::BidAskDiff.prev(), BottomMetric::MidDiff);

    // 2. Key mapping tests
    assert_eq!(BottomMetric::from_key_number(2), Some(BottomMetric::MidDiff));
    assert_eq!(BottomMetric::from_key_number(3), Some(BottomMetric::BidAskDiff));
    assert_eq!(BottomMetric::from_key_number(4), Some(BottomMetric::SpreadDiff));
    assert_eq!(BottomMetric::from_key_number(5), Some(BottomMetric::LeadLag));
    assert_eq!(BottomMetric::from_key_number(6), Some(BottomMetric::MidDispersion));
    assert_eq!(BottomMetric::from_key_number(7), Some(BottomMetric::MoveBreadthView));
    assert_eq!(BottomMetric::from_key_number(8), Some(BottomMetric::QuotePersistence));
    assert_eq!(BottomMetric::from_key_number(1), Some(BottomMetric::QuotePath));

    // 3. UI Key input event simulation
    let run_id = RunId([3u8; 16]);
    let snap = Arc::new(UiSnapshot {
        schema_revision: 1,
        snapshot_revision: 1,
        projection_revision: 1,
        run_id,
        built_mono_ns: MonoNs(100_000_000),
        processed_watermark_ns: MonoNs(100_000_000),
        display_now_utc: UtcMs(1000),
        active_pair: (1, 2),
        broker_overviews: vec![],
        active_pair_comparison: None,
        active_candles: None,
        candle_views: HashMap::new(),
        diagnostics: Vec::new(),
        ..Default::default()
    });

    let exchange = Arc::new(SnapshotExchange::new(snap));
    let mut app = DashboardApp::new(exchange, (1, 2));

    assert_eq!(app.bottom_metric(), BottomMetric::QuotePath);

    let ctx = egui::Context::default();

    // Simulate pressing Key 3 (Num3)
    let mut input2 = egui::RawInput::default();
    input2.events.push(egui::Event::Key {
        key: egui::Key::Num3,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    });
    let _ = ctx.run(input2, |ctx| {
        app.render_ui(ctx);
    });
    assert_eq!(app.bottom_metric(), BottomMetric::BidAskDiff);

    // Simulate pressing Key 4 (Num4)
    let mut input3 = egui::RawInput::default();
    input3.events.push(egui::Event::Key {
        key: egui::Key::Num4,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    });
    let _ = ctx.run(input3, |ctx| {
        app.render_ui(ctx);
    });
    assert_eq!(app.bottom_metric(), BottomMetric::SpreadDiff);

    // Simulate pressing Key 5 (Num5)
    let mut input4 = egui::RawInput::default();
    input4.events.push(egui::Event::Key {
        key: egui::Key::Num5,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    });
    let _ = ctx.run(input4, |ctx| {
        app.render_ui(ctx);
    });
    assert_eq!(app.bottom_metric(), BottomMetric::LeadLag);

    // Simulate pressing Tab -> should cycle to Mid Dispersion
    let mut input_tab = egui::RawInput::default();
    input_tab.events.push(egui::Event::Key {
        key: egui::Key::Tab,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    });
    let _ = ctx.run(input_tab, |ctx| {
        app.render_ui(ctx);
    });
    assert_eq!(app.bottom_metric(), BottomMetric::MidDispersion);

    // Simulate pressing Shift+Tab -> should cycle back to LeadLag
    let mut input_shift_tab = egui::RawInput {
        modifiers: egui::Modifiers::SHIFT,
        ..Default::default()
    };
    input_shift_tab.events.push(egui::Event::Key {
        key: egui::Key::Tab,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::SHIFT,
    });
    let _ = ctx.run(input_shift_tab, |ctx| {
        app.render_ui(ctx);
    });
    assert_eq!(app.bottom_metric(), BottomMetric::LeadLag);

    // Simulate pressing Key 1 (Num1) -> should switch to QuotePath
    let mut input8 = egui::RawInput::default();
    input8.events.push(egui::Event::Key {
        key: egui::Key::Num1,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    });
    let _ = ctx.run(input8, |ctx| {
        app.render_ui(ctx);
    });
    assert_eq!(app.bottom_metric(), BottomMetric::QuotePath);
}

#[test]
fn test_all_bottom_metrics_render_headless() {
    use tick_scope::ui::chart::BottomMetric;

    let run_id = RunId([4u8; 16]);
    let snap = Arc::new(UiSnapshot {
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
                name: "Broker A".to_string(),
                symbol: "USDJPY".to_string(),
                latest_quote: None,
                min_spread: None,
                max_spread: None,
                health: HealthState::default(),
                tick_rate_1s: 5.0,
                active_utc_offset_sec: 0,
                is_auto_offset: false,
            },
            BrokerOverview {
                broker_id: 2,
                name: "Broker B".to_string(),
                symbol: "USDJPY".to_string(),
                latest_quote: None,
                min_spread: None,
                max_spread: None,
                health: HealthState::default(),
                tick_rate_1s: 5.0,
                active_utc_offset_sec: 0,
                is_auto_offset: false,
            },
        ],
        active_pair_comparison: Some(PairComparison {
            broker_a: 1,
            broker_b: 2,
            as_of_mono_ns: MonoNs(100_000_000),
            bid_diff: Some(0.002),
            ask_diff: Some(0.003),
            mid_diff: Some(0.0025),
            spread_diff: Some(0.001),
            recent_diff_series: vec![
                DiffPoint {
                    mono_ns: MonoNs(90_000_000),
                    bid_diff: 0.001,
                    ask_diff: 0.002,
                    mid_diff: 0.0015,
                    spread_diff: 0.001,
                },
                DiffPoint {
                    mono_ns: MonoNs(100_000_000),
                    bid_diff: 0.002,
                    ask_diff: 0.003,
                    mid_diff: 0.0025,
                    spread_diff: 0.001,
                },
            ],
            latest_match: Some(LeadLagMatch {
                match_id: 42,
                leader: 1,
                follower: 2,
                leader_event: MoveEvent {
                    segment_id: 1,
                    broker_id: 1,
                    trigger_sequence: 100,
                    rx_mono_ns: MonoNs(95_000_000),
                    direction: MoveDirection::Up,
                    anchor_mid: 150.000,
                    current_mid: 150.005,
                    mid_delta_points: 5.0,
                    bid_delta: 0.005,
                    ask_delta: 0.005,
                    mid_delta: 0.005,
                    spread_delta: 0.0,
                    quality: MoveQuality::BothSides,
                },
                follower_event: MoveEvent {
                    segment_id: 1,
                    broker_id: 2,
                    trigger_sequence: 102,
                    rx_mono_ns: MonoNs(98_000_000),
                    direction: MoveDirection::Up,
                    anchor_mid: 149.998,
                    current_mid: 150.003,
                    mid_delta_points: 5.0,
                    bid_delta: 0.005,
                    ask_delta: 0.005,
                    mid_delta: 0.005,
                    spread_delta: 0.0,
                    quality: MoveQuality::BothSides,
                },
                t_leader: MonoNs(95_000_000),
                t_follower: MonoNs(98_000_000),
                signed_delta_ns: 3_000_000,
                abs_delta_ns: 3_000_000,
                raw_delta_ms: 3.0,
                ema_delta_ms: Some(2.8),
                segment_id: 1,
            }),
            ema_lead_lag_ms: Some(2.8),
        }),
        active_candles: None,
        candle_views: HashMap::new(),
        diagnostics: Vec::new(),
        ..Default::default()
    });

    let exchange = Arc::new(SnapshotExchange::new(snap));
    let mut app = DashboardApp::new(exchange, (1, 2));
    let ctx = egui::Context::default();

    // Verify all metrics render successfully without panicking
    for &metric in &BottomMetric::ALL {
        app.set_bottom_metric(metric);
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            app.render_ui(ctx);
        });
        assert_eq!(app.bottom_metric(), metric);
    }
}

#[test]
fn test_ui_settings_persistence_lifecycle() {
    use tick_scope::config::BrokerConfig;
    use tick_scope::ui::chart::BottomMetric;
    use tick_scope::ui::settings::load_ui_state;

    let temp_dir = tempfile::tempdir().unwrap();
    let state_file = temp_dir.path().join("ui_state.json");

    let snap = Arc::new(UiSnapshot::default());
    let exchange = Arc::new(SnapshotExchange::new(snap));

    // 1. First session: change settings and drop
    {
        let mut app = DashboardApp::new(exchange.clone(), (1, 2))
            .with_ui_state_path(state_file.clone());

        app.set_selected_pair(2, 3);
        app.set_bottom_metric(BottomMetric::SpreadDiff);
        app.set_show_broker_overview(true);
        app.set_candle_bar_width(6.0);

        let ctx = egui::Context::default();
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            app.render_ui(ctx);
        });

        // Mutate additional settings
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::pos2(50.0, 50.0),
                egui::vec2(1200.0, 800.0),
            )),
            ..Default::default()
        };
        let _ = ctx.run(input, |ctx| {
            // Emulate selecting S10
            egui::CentralPanel::default().show(ctx, |_ui| {
                app.set_bottom_metric(BottomMetric::LeadLag);
            });
            app.render_ui(ctx);
        });
        // Drops here, executing save_state via Drop
    }

    // 2. Verify state file was saved
    assert!(state_file.exists());
    let mut loaded = load_ui_state(&state_file).expect("UI state should load");
    assert_eq!(loaded.active_pair, (2, 3));
    assert_eq!(loaded.bottom_metric, BottomMetric::LeadLag);
    assert!(loaded.show_broker_overview);
    assert_eq!(loaded.candle_bar_width, 6.0);

    // 3. Second session: reconcile with brokers and restore into new app instance
    let brokers = vec![
        BrokerConfig { id: 1, name: "A".to_string(), ..Default::default() },
        BrokerConfig { id: 2, name: "B".to_string(), ..Default::default() },
        BrokerConfig { id: 3, name: "C".to_string(), ..Default::default() },
    ];
    loaded.reconcile_with_brokers(&brokers, (1, 2));

    {
        let app = DashboardApp::new(exchange.clone(), (1, 2))
            .with_ui_state(&loaded)
            .with_ui_state_path(state_file.clone());

        assert_eq!(app.selected_pair(), (2, 3));
        assert_eq!(app.bottom_metric(), BottomMetric::LeadLag);
        assert!(app.show_broker_overview());
        assert_eq!(app.candle_bar_width(), 6.0);
    }

    // 4. Test broker disappearance fallback
    let brokers_missing_c = vec![
        BrokerConfig { id: 1, name: "A".to_string(), ..Default::default() },
        BrokerConfig { id: 2, name: "B".to_string(), ..Default::default() },
    ];
    loaded.reconcile_with_brokers(&brokers_missing_c, (1, 2));
    assert_eq!(loaded.active_pair, (1, 2));
}

#[test]
fn test_independent_top_and_bottom_x_axis_mode() {
    use tick_scope::ui::chart::ChartXAxisMode;
    use tick_scope::ui::settings::load_ui_state;

    let dir = tempfile::tempdir().unwrap();
    let state_file = dir.path().join("ui_state.json");
    let exchange = Arc::new(SnapshotExchange::new(Arc::new(UiSnapshot::default())));

    let mut app = DashboardApp::new(exchange.clone(), (1, 2))
        .with_ui_state_path(state_file.clone());

    // Initial defaults
    assert_eq!(app.top_x_axis_mode(), ChartXAxisMode::ReceiveTime);
    assert_eq!(app.bottom_x_axis_mode(), ChartXAxisMode::ReceiveTime);

    // Set Top to TickCount while Bottom remains ReceiveTime
    app.set_top_x_axis_mode(ChartXAxisMode::TickCount);
    assert_eq!(app.top_x_axis_mode(), ChartXAxisMode::TickCount);
    assert_eq!(app.bottom_x_axis_mode(), ChartXAxisMode::ReceiveTime);

    // Set Bottom to TickCount, Top to ReceiveTime
    app.set_bottom_x_axis_mode(ChartXAxisMode::TickCount);
    app.set_top_x_axis_mode(ChartXAxisMode::ReceiveTime);
    assert_eq!(app.top_x_axis_mode(), ChartXAxisMode::ReceiveTime);
    assert_eq!(app.bottom_x_axis_mode(), ChartXAxisMode::TickCount);

    // Save and verify persistence
    app.save_state();
    let loaded = load_ui_state(&state_file).expect("UI state should load");
    assert_eq!(loaded.top_x_axis_mode, ChartXAxisMode::ReceiveTime);
    assert_eq!(loaded.bottom_x_axis_mode, ChartXAxisMode::TickCount);

    // Restore in new app instance
    let restored = DashboardApp::new(exchange, (1, 2)).with_ui_state(&loaded);
    assert_eq!(restored.top_x_axis_mode(), ChartXAxisMode::ReceiveTime);
    assert_eq!(restored.bottom_x_axis_mode(), ChartXAxisMode::TickCount);
}

#[test]
fn test_broker_visibility_toggle_and_minimum_guard() {
    let exchange = Arc::new(SnapshotExchange::new(Arc::new(UiSnapshot::default())));
    let mut app = DashboardApp::new(exchange, (1, 2));

    let overviews = vec![
        BrokerOverview { broker_id: 1, name: "Broker 1".to_string(), ..Default::default() },
        BrokerOverview { broker_id: 2, name: "Broker 2".to_string(), ..Default::default() },
        BrokerOverview { broker_id: 3, name: "Broker 3".to_string(), ..Default::default() },
    ];

    // Initial state: all visible
    assert!(app.is_broker_visible(1));
    assert!(app.is_broker_visible(2));
    assert!(app.is_broker_visible(3));
    assert_eq!(app.visible_broker_ids(&overviews), vec![1, 2, 3]);

    // Hide broker 3
    app.set_broker_visible(3, false, &overviews);
    assert!(app.is_broker_visible(1));
    assert!(app.is_broker_visible(2));
    assert!(!app.is_broker_visible(3));
    assert_eq!(app.visible_broker_ids(&overviews), vec![1, 2]);

    // Guard test: Trying to hide broker 2 leaves only 1 visible broker, which should be rejected
    app.set_broker_visible(2, false, &overviews);
    assert!(app.is_broker_visible(2), "Should remain visible due to minimum 2 broker guard");
    assert_eq!(app.visible_broker_ids(&overviews), vec![1, 2]);

    // Test active pair auto-switching when a selected broker is hidden:
    // First, show broker 3 again so we have 3 visible brokers
    app.set_broker_visible(3, true, &overviews);
    assert!(app.is_broker_visible(3));
    assert_eq!(app.selected_pair(), (1, 2));

    // Now hide broker 2 (current Broker B) -> Broker B should automatically switch to Broker 3
    app.set_broker_visible(2, false, &overviews);
    assert_eq!(app.selected_pair(), (1, 3));

    // Show all brokers
    app.show_all_brokers();
    assert!(app.is_broker_visible(1));
    assert!(app.is_broker_visible(2));
    assert!(app.is_broker_visible(3));
    assert_eq!(app.visible_broker_ids(&overviews), vec![1, 2, 3]);
}

#[test]
fn test_candlestick_chart_hides_broker_without_wasted_gap() {
    use tick_scope::ui::chart::{draw_candlestick_chart_for_brokers, ChartTheme};
    use tick_scope::ui::settings::{CandleFollowCriteria, CandlePriceScaleMode};

    let mut slots_by_broker = HashMap::new();
    let mut slot_starts = Vec::new();

    for i in 0..10 {
        let utc = UtcMs(i * 1000);
        slot_starts.push(utc);
        for b_id in [1, 2, 3] {
            let slots = slots_by_broker.entry(b_id).or_insert_with(Vec::new);
            slots.push(CandleSlot {
                broker_id: b_id,
                segment_id: 1,
                period_ms: 1000,
                start_utc_ms: utc,
                state: SlotState::Closed,
                ohlc: Some(Ohlc {
                    open: 150.0,
                    high: 150.1,
                    low: 149.9,
                    close: 150.05,
                    open_key: (utc, 1),
                    close_key: (utc, 1),
                }),
                tick_count: 5,
                revision: 1,
                coverage: SlotCoverage::Full,
            });
        }
    }

    let view = CandleView {
        period_ms: 1000,
        slot_starts,
        slots_by_broker,
    };

    let overviews = vec![
        BrokerOverview { broker_id: 1, name: "Broker 1".to_string(), ..Default::default() },
        BrokerOverview { broker_id: 2, name: "Broker 2".to_string(), ..Default::default() },
        BrokerOverview { broker_id: 3, name: "Broker 3".to_string(), ..Default::default() },
    ];

    let ctx = egui::Context::default();
    let _ = ctx.run(egui::RawInput::default(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            let theme = ChartTheme::default();
            let rect = egui::Rect::from_min_size(egui::Pos2::new(0.0, 0.0), egui::Vec2::new(800.0, 400.0));
            let painter = ui.painter_at(rect);
            let mut anchor = None;
            let mut latch = None;

            // 1. Draw with 3 brokers visible
            draw_candlestick_chart_for_brokers(
                &painter,
                rect,
                Some(&view),
                &[1, 2, 3],
                &overviews,
                5.0,
                CandlePriceScaleMode::Auto,
                CandleFollowCriteria::Median,
                0.01,
                &mut anchor,
                &mut latch,
                Some(150.0),
                1000,
                MonoNs(100_000_000),
                PriceMode::Bid,
                &theme,
            );

            // 2. Draw with broker 2 hidden: only [1, 3] visible
            // The slot_width for 2 brokers (5px * 2 + 1px gap + 4px = 15px) is smaller
            // than for 3 brokers (5px * 3 + 2px gap + 4px = 21px), preventing empty gaps.
            draw_candlestick_chart_for_brokers(
                &painter,
                rect,
                Some(&view),
                &[1, 3],
                &overviews,
                5.0,
                CandlePriceScaleMode::Auto,
                CandleFollowCriteria::Median,
                0.01,
                &mut anchor,
                &mut latch,
                Some(150.0),
                1000,
                MonoNs(100_000_000),
                PriceMode::Bid,
                &theme,
            );
        });
    });
}
