use tick_scope::config::AppConfig;
use tick_scope::core::models::*;
use tick_scope::core::types::*;
use tick_scope::tick::candle::{calculate_slot_start, CandleBook};

const fn make_test_tick(
    broker_id: BrokerId,
    seq: Sequence,
    utc_ms: i64,
    bid: f64,
    ask: f64,
) -> NormalizedTick {
    NormalizedTick {
        observed: ObservedTick {
            tick_id: TickId {
                broker_id,
                session_id: 1,
                sequence: seq,
            },
            record: TickRecord {
                sequence: seq,
                broker_time_msc: utc_ms,
                ea_elapsed_us: 10,
                bid,
                ask,
                last: 0.0,
                volume: 1,
                volume_real: 1.0,
                flags: 0,
                reserved: 0,
            },
            rx_mono_ns: MonoNs(100),
            rx_unix_ns: None,
            connection_generation: 1,
            frame_index: 1,
            is_warmup: false,
            segment_id: 1,
            disposition: SequenceDisposition::New,
        },
        utc_ms: UtcMs(utc_ms),
        normalization_epoch: 1,
    }
}

#[test]
fn test_tc01_slot_math_and_ohlc_revisions() {
    // Mathematical floor
    assert_eq!(calculate_slot_start(UtcMs(999), 1000), UtcMs(0));
    assert_eq!(calculate_slot_start(UtcMs(1000), 1000), UtcMs(1000));
    assert_eq!(calculate_slot_start(UtcMs(-1), 1000), UtcMs(-1000));

    let mut book = CandleBook::new(vec![1000]);

    // Tick 1 at 1200ms: Open=150.0, High=150.0, Low=150.0, Close=150.0
    book.on_tick(
        &make_test_tick(1, 1, 1200, 150.0, 150.02),
        PriceMode::Bid,
        UtcMs(1500),
    );

    // Tick 2 at 1800ms: High=150.5, Close=150.5
    book.on_tick(
        &make_test_tick(1, 2, 1800, 150.5, 150.52),
        PriceMode::Bid,
        UtcMs(1900),
    );

    // Tick 3 (Late arrival!) at 1100ms: Should become the new Open!
    book.on_tick(
        &make_test_tick(1, 0, 1100, 149.5, 149.52),
        PriceMode::Bid,
        UtcMs(1950),
    );

    let view = book.get_candle_view(1000, &[1], 1, UtcMs(1950));
    let slot = &view.slots_by_broker[&1][0];
    assert_eq!(slot.state, SlotState::Active);
    assert_eq!(slot.tick_count, 3);
    assert_eq!(slot.revision, 3);

    let ohlc = slot.ohlc.unwrap();
    assert_eq!(ohlc.open, 149.5, "Late tick at 1100ms must update Open");
    assert_eq!(ohlc.high, 150.5);
    assert_eq!(ohlc.low, 149.5);
    assert_eq!(ohlc.close, 150.5);
}

#[test]
fn test_tc02_empty_slots_and_aligned_x_axis() {
    let mut book = CandleBook::new(vec![1000]);

    // Broker 1 has a tick in slot 2000
    book.on_tick(
        &make_test_tick(1, 1, 2500, 150.0, 150.02),
        PriceMode::Bid,
        UtcMs(2900),
    );

    // Broker 2 has NO ticks at all
    let view = book.get_candle_view(1000, &[1, 2], 3, UtcMs(2900));

    assert_eq!(view.slot_starts.len(), 3);
    assert_eq!(view.slot_starts, vec![UtcMs(0), UtcMs(1000), UtcMs(2000)]);

    // Broker 2 slots must all be Empty with None OHLC
    let b2_slots = &view.slots_by_broker[&2];
    assert_eq!(b2_slots.len(), 3);
    for slot in b2_slots {
        assert!(
            slot.ohlc.is_none(),
            "Empty slot must not have synthetic OHLC"
        );
        assert_eq!(slot.tick_count, 0);
    }
}

#[test]
fn test_tc03_slot_active_to_closed_advancement() {
    let mut book = CandleBook::new(vec![1000]);

    // Tick at 1500 in slot [1000, 2000)
    book.on_tick(
        &make_test_tick(1, 1, 1500, 150.0, 150.02),
        PriceMode::Bid,
        UtcMs(1600),
    );

    let view1 = book.get_candle_view(1000, &[1], 1, UtcMs(1600));
    assert_eq!(view1.slots_by_broker[&1][0].state, SlotState::Active);

    // Advance UTC past 2000ms
    book.advance_utc(UtcMs(2100));

    let view2 = book.get_candle_view(1000, &[1], 2, UtcMs(2100));
    // Slot 0 is [1000, 2000), which is now Closed
    assert_eq!(view2.slots_by_broker[&1][0].start_utc_ms, UtcMs(1000));
    assert_eq!(view2.slots_by_broker[&1][0].state, SlotState::Closed);
    // Slot 1 is [2000, 3000), which is current Active
    assert_eq!(view2.slots_by_broker[&1][1].start_utc_ms, UtcMs(2000));
    assert_eq!(view2.slots_by_broker[&1][1].state, SlotState::Active);
}

#[test]
fn test_tc04_retention_slots_preserved_to_left_edge() {
    use tick_scope::config::SlotRetention;

    let retentions = vec![SlotRetention {
        period_ms: 1000,
        slots: 60,
    }];
    let mut book = CandleBook::with_retentions(&retentions);

    // Feed 70 slots of ticks (from 1000ms to 70000ms)
    for i in 1..=70 {
        let utc_ms = i * 1000;
        book.on_tick(
            &make_test_tick(
                1,
                i as u64,
                utc_ms,
                150.0 + (i as f64 * 0.01),
                150.02 + (i as f64 * 0.01),
            ),
            PriceMode::Bid,
            UtcMs(utc_ms + 100),
        );
    }

    // Now query 60 slots ending at current_utc_now = 70500ms
    let view = book.get_candle_view(1000, &[1], 60, UtcMs(70500));
    assert_eq!(view.slot_starts.len(), 60);

    let b1_slots = &view.slots_by_broker[&1];
    assert_eq!(b1_slots.len(), 60);

    // Every single slot from index 0 (left edge) to 59 (right edge) must have valid OHLC!
    for (i, slot) in b1_slots.iter().enumerate() {
        assert!(
            slot.ohlc.is_some(),
            "Slot at index {} (start_utc_ms = {:?}) must have OHLC and not disappear at the left edge",
            i,
            slot.start_utc_ms
        );
        assert_ne!(slot.state, SlotState::Empty);
    }
}

#[test]
fn test_tc05_engine_candle_views_use_configured_retention_slots() {
    use tick_scope::config::AppConfig;
    use tick_scope::tick::engine::TickEngine;

    let mut config = AppConfig::default();
    config.history.retentions = vec![
        tick_scope::config::SlotRetention {
            period_ms: 1000,
            slots: 60,
        },
        tick_scope::config::SlotRetention {
            period_ms: 60000,
            slots: 60,
        },
    ];
    let engine = TickEngine::new(config);
    let proj = engine.make_projection(UtcMs(1_000_000));

    let cv_s1 = proj.candle_views.get(&1000).expect("S1 candle view exists");
    assert_eq!(
        cv_s1.slot_starts.len(),
        60,
        "S1 candle view should have 60 slots as configured"
    );

    let cv_m1 = proj
        .candle_views
        .get(&60000)
        .expect("M1 candle view exists");
    assert_eq!(
        cv_m1.slot_starts.len(),
        60,
        "M1 candle view should have 60 slots as configured"
    );
}

#[test]
fn test_future_tick_breaks_subsequent_present_candles() {
    let mut book = CandleBook::new(vec![1000]);

    // Suppose broker sends a tick with offset=0 (so 3 hours in future: 10_800_000 ms)
    book.on_tick(
        &make_test_tick(5, 1, 10_800_000, 150.0, 150.02),
        PriceMode::Bid,
        UtcMs(10_800_000),
    );

    // Later, offset is corrected (+10800), and ticks arrive at true present UTC (e.g. 10_000 ms)
    book.on_tick(
        &make_test_tick(5, 2, 10_000, 150.0, 150.02),
        PriceMode::Bid,
        UtcMs(10_000),
    );

    // Query present candle view
    let view = book.get_candle_view(1000, &[5], 10, UtcMs(10_000));
    let present_slot = &view.slots_by_broker[&5][9];
    assert!(
        present_slot.ohlc.is_some(),
        "Present slot must have OHLC even if a future tick arrived previously"
    );
}

#[test]
fn test_warmup_fills_full_candle_and_tick_candle_viewport() {
    let mut config = AppConfig::default();
    config.history.warmup_seconds = 7200;
    config.history.retentions = vec![
        tick_scope::config::SlotRetention {
            period_ms: 1000,
            slots: 120,
        },
        tick_scope::config::SlotRetention {
            period_ms: 5000,
            slots: 120,
        },
        tick_scope::config::SlotRetention {
            period_ms: 10000,
            slots: 120,
        },
        tick_scope::config::SlotRetention {
            period_ms: 60000,
            slots: 120,
        },
    ];

    let mut book = CandleBook::with_retentions(&config.history.retentions);

    // Simulate 7200 seconds of warmup ticks (e.g. 1 tick every 2 seconds = 3600 ticks)
    let start_time_ms = 10_000_000_i64;
    let end_time_ms = start_time_ms + 7_200_000; // +7200 seconds
    let broker_id = 1;

    let mut tct = tick_scope::tick::tick_candle::TickCandleTracker::new(broker_id, 600, 0.2, 0.01);

    let mut cur_ms = start_time_ms;
    let mut seq = 0u64;
    let mut price = 150.0;
    while cur_ms <= end_time_ms {
        // Price oscillates and drifts
        let delta = if seq % 4 == 0 {
            0.03
        } else if seq % 4 == 1 {
            -0.01
        } else if seq % 4 == 2 {
            0.04
        } else {
            -0.02
        };
        price += delta;

        let mut tick = make_test_tick(broker_id, seq, cur_ms, price, price + 0.003);
        tick.observed.is_warmup = true;

        let rx_utc_now = UtcMs(cur_ms);
        book.on_tick(&tick, PriceMode::Bid, rx_utc_now);

        tct.on_tick(price + 0.0015, cur_ms / 1000);

        cur_ms += 1000; // Every 1 second
        seq += 1;
    }

    // Verify all timeframes (M1, S10, S5, S1) have full 120 slots populated
    for &period in &[1000, 5000, 10000, 60000] {
        let cv = book.get_candle_view(period, &[broker_id], 120, UtcMs(end_time_ms));
        assert_eq!(
            cv.slot_starts.len(),
            120,
            "Period {period}ms must have full 120 visible slots"
        );
        let broker_slots = &cv.slots_by_broker[&broker_id];
        let filled_slots = broker_slots.iter().filter(|s| s.ohlc.is_some()).count();
        assert_eq!(
            filled_slots, 120,
            "Period {period}ms must have all 120 slots populated with OHLC"
        );
    }

    // Verify TickCandle ring buffer reached full capacity (600 candles) across the 7200-second warmup
    assert_eq!(
        tct.candles.len(),
        600,
        "TickCandle ring buffer must be fully saturated across 2-hour warmup"
    );
}
