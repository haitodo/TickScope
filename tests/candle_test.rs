//! Candle tests: T-C01, T-C02, T-C03.

use tick_compare::contracts::models::*;
use tick_compare::contracts::types::*;
use tick_compare::tick::candle::{calculate_slot_start, CandleBook};

fn make_test_tick(broker_id: BrokerId, seq: Sequence, utc_ms: i64, bid: f64, ask: f64) -> NormalizedTick {
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
        assert!(slot.ohlc.is_none(), "Empty slot must not have synthetic OHLC");
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
    use tick_compare::contracts::config::SlotRetention;

    let retentions = vec![SlotRetention { period_ms: 1000, slots: 60 }];
    let mut book = CandleBook::with_retentions(&retentions);

    // Feed 70 slots of ticks (from 1000ms to 70000ms)
    for i in 1..=70 {
        let utc_ms = i * 1000;
        book.on_tick(
            &make_test_tick(1, i as u64, utc_ms, 150.0 + (i as f64 * 0.01), 150.02 + (i as f64 * 0.01)),
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
    use tick_compare::contracts::config::AppConfig;
    use tick_compare::tick::engine::TickEngine;

    let mut config = AppConfig::default();
    config.history.retentions = vec![
        tick_compare::contracts::config::SlotRetention { period_ms: 1000, slots: 60 },
        tick_compare::contracts::config::SlotRetention { period_ms: 60000, slots: 60 },
    ];
    let engine = TickEngine::new(config);
    let proj = engine.make_projection(UtcMs(1_000_000));

    let cv_s1 = proj.candle_views.get(&1000).expect("S1 candle view exists");
    assert_eq!(cv_s1.slot_starts.len(), 60, "S1 candle view should have 60 slots as configured");

    let cv_m1 = proj.candle_views.get(&60000).expect("M1 candle view exists");
    assert_eq!(cv_m1.slot_starts.len(), 60, "M1 candle view should have 60 slots as configured");
}


