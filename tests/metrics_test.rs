//! Metrics and Matcher tests: T-M01, T-L01, T-L02, T-L03, T-L04.

use tick_compare::contracts::models::*;
use tick_compare::contracts::types::*;
use tick_compare::metrics::lead_lag::SignificantMidMoveDetector;
use tick_compare::metrics::price_diff::PairDifferenceTracker;
use tick_compare::metrics::spread::SpreadTracker;
use tick_compare::tick::matcher::OneToOneEventMatcher;

fn make_quote(broker_id: BrokerId, seq: Sequence, rx_mono: u64, bid: f64, ask: f64) -> Quote {
    Quote {
        tick_id: TickId {
            broker_id,
            session_id: 1,
            sequence: seq,
        },
        bid,
        ask,
        mid: (bid + ask) / 2.0,
        spread: ask - bid,
        rx_mono_ns: MonoNs(rx_mono),
        utc_ms: Some(UtcMs(1000)),
        is_warmup: false,
        is_valid: true,
    }
}

#[test]
fn test_tm01_spread_and_pair_diff() {
    let mut spread_tracker = SpreadTracker::new(1);
    spread_tracker.on_quote(0.002, MonoNs(100_000_000));
    spread_tracker.on_quote(0.004, MonoNs(200_000_000));
    spread_tracker.on_quote(0.001, MonoNs(300_000_000));

    assert_eq!(spread_tracker.min_spread(), Some(0.001));
    assert_eq!(spread_tracker.max_spread(), Some(0.004));
    assert_eq!(spread_tracker.tick_rate_1s(), 3.0);

    let mut diff_tracker = PairDifferenceTracker::new(1, 2, 60);
    let q_a = make_quote(1, 1, 100, 155.000, 155.002);
    let q_b = make_quote(2, 1, 105, 154.995, 155.000);

    let (bid_d, ask_d, mid_d, sp_d) =
        diff_tracker.compute_and_record(Some(&q_a), Some(&q_b), MonoNs(110));

    assert!((bid_d.unwrap() - 0.005).abs() < 1e-9);
    assert!((ask_d.unwrap() - 0.002).abs() < 1e-9);
    assert!((mid_d.unwrap() - 0.0035).abs() < 1e-9);
    assert!((sp_d.unwrap() - (-0.003)).abs() < 1e-9);
    assert_eq!(diff_tracker.series().len(), 1);
}

#[test]
fn test_tl02_mid_move_detector_anchor_and_cooldown() {
    let mut detector = SignificantMidMoveDetector::new(1, 0.001, 2.0, 20, 1);
    // threshold = 2 * 0.001 = 0.002

    // Quote 1 at t=0: Sets initial anchor at Mid=150.001, no event
    let q1 = make_quote(1, 1, 0, 150.000, 150.002);
    assert!(detector.on_quote(&q1).is_none());

    // Quote 2 at t=10ms: Mid=150.002 (diff = 0.001 < 0.002) -> No event
    let q2 = make_quote(1, 2, 10_000_000, 150.001, 150.003);
    assert!(detector.on_quote(&q2).is_none());

    // Quote 3 at t=25ms: Mid=150.004 (diff = 150.004 - 150.001 = 0.003 >= 0.002) -> Fires Up event!
    let q3 = make_quote(1, 3, 25_000_000, 150.003, 150.005);
    let ev3 = detector.on_quote(&q3).expect("Must fire move event");
    assert_eq!(ev3.direction, MoveDirection::Up);
    assert!((ev3.mid_delta_points - 3.0).abs() < 1e-6);
    assert!((ev3.current_mid - 150.004).abs() < 1e-6);

    // Quote 4 at t=30ms (within 20ms cooldown from t=25ms -> cooldown until 45ms):
    // Even though Mid moves 150.010, it must be suppressed by cooldown!
    let q4 = make_quote(1, 4, 30_000_000, 150.009, 150.011);
    assert!(detector.on_quote(&q4).is_none(), "Suppressed during cooldown");

    // Quote 5 at t=46ms (after cooldown): Mid=150.008 (anchor was re-anchored at 150.004, diff=0.004 >= 0.002) -> Fires!
    let q5 = make_quote(1, 5, 46_000_000, 150.007, 150.009);
    let ev5 = detector.on_quote(&q5).expect("Must fire after cooldown");
    assert_eq!(ev5.direction, MoveDirection::Up);
}

#[test]
fn test_tl03_tl04_one_to_one_matcher() {
    let mut matcher = OneToOneEventMatcher::new(1, 2, 100, 0.1, 100, 1);

    // Case 1: A at 0ms, A at 8ms, B at 10ms -> B matches A at 8ms (closest in time, min diff = 2ms)
    let ev_a0 = MoveEvent {
        segment_id: 1,
        broker_id: 1,
        trigger_sequence: 1,
        rx_mono_ns: MonoNs(0),
        direction: MoveDirection::Up,
        anchor_mid: 150.0,
        current_mid: 150.003,
        mid_delta_points: 3.0,
        bid_delta: 0.003,
        ask_delta: 0.003,
        mid_delta: 0.003,
        spread_delta: 0.0,
        quality: MoveQuality::BothSides,
    };
    let ev_a8 = MoveEvent {
        segment_id: 1,
        broker_id: 1,
        trigger_sequence: 2,
        rx_mono_ns: MonoNs(8_000_000),
        direction: MoveDirection::Up,
        anchor_mid: 150.003,
        current_mid: 150.006,
        mid_delta_points: 3.0,
        bid_delta: 0.003,
        ask_delta: 0.003,
        mid_delta: 0.003,
        spread_delta: 0.0,
        quality: MoveQuality::BothSides,
    };
    let ev_b10 = MoveEvent {
        segment_id: 1,
        broker_id: 2,
        trigger_sequence: 10,
        rx_mono_ns: MonoNs(10_000_000),
        direction: MoveDirection::Up,
        anchor_mid: 150.0,
        current_mid: 150.003,
        mid_delta_points: 3.0,
        bid_delta: 0.003,
        ask_delta: 0.003,
        mid_delta: 0.003,
        spread_delta: 0.0,
        quality: MoveQuality::BothSides,
    };

    assert!(matcher.on_event(ev_a0).is_none());
    assert!(matcher.on_event(ev_a8).is_none());
    let m = matcher.on_event(ev_b10).expect("Must match ev_a8 with ev_b10");

    assert_eq!(m.leader, 1, "Broker 1 leads");
    assert_eq!(m.follower, 2, "Broker 2 follows");
    assert_eq!(m.signed_delta_ns, 2_000_000); // t_B - t_A = 10ms - 8ms = +2ms
    assert_eq!(m.raw_delta_ms, 2.0);
    assert_eq!(m.ema_delta_ms, Some(2.0));

    // Case 2: No reuse! Subsequent B at 11ms cannot reuse ev_a8
    let ev_b11 = MoveEvent {
        segment_id: 1,
        broker_id: 2,
        trigger_sequence: 11,
        rx_mono_ns: MonoNs(11_000_000),
        direction: MoveDirection::Up,
        anchor_mid: 150.003,
        current_mid: 150.006,
        mid_delta_points: 3.0,
        bid_delta: 0.003,
        ask_delta: 0.003,
        mid_delta: 0.003,
        spread_delta: 0.0,
        quality: MoveQuality::BothSides,
    };
    // It can match ev_a0 (delta = 11ms <= 100ms)
    let m2 = matcher.on_event(ev_b11).expect("Matches remaining ev_a0");
    assert_eq!(m2.leader_event.trigger_sequence, 1, "Matched ev_a0");

    // Case 3: Delta 0 is not a match
    let ev_a50 = MoveEvent {
        segment_id: 1,
        broker_id: 1,
        trigger_sequence: 3,
        rx_mono_ns: MonoNs(50_000_000),
        direction: MoveDirection::Down,
        anchor_mid: 150.0,
        current_mid: 149.997,
        mid_delta_points: 3.0,
        bid_delta: -0.003,
        ask_delta: -0.003,
        mid_delta: -0.003,
        spread_delta: 0.0,
        quality: MoveQuality::BothSides,
    };
    let ev_b50 = MoveEvent {
        segment_id: 1,
        broker_id: 2,
        trigger_sequence: 12,
        rx_mono_ns: MonoNs(50_000_000), // Same time!
        direction: MoveDirection::Down,
        anchor_mid: 150.0,
        current_mid: 149.997,
        mid_delta_points: 3.0,
        bid_delta: -0.003,
        ask_delta: -0.003,
        mid_delta: -0.003,
        spread_delta: 0.0,
        quality: MoveQuality::BothSides,
    };
    matcher.on_event(ev_a50);
    assert!(matcher.on_event(ev_b50).is_none(), "Delta 0 must not match");
}
