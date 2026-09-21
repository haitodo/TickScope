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
