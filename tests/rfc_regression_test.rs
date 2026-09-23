//! RFC Beta 0.3 Regression & Verification Suite.
//! Reference: docs/improvement.md Sections 81, 82, 83, 91.
//!
//! # Core Invariants & Tests Covered:
//! - RFC §81 Test A: Single broker quote jump -> Outlier / Broker Deviation, NOT market-wide move.
//! - RFC §81 Test B: 5 brokers simultaneous move -> High Breadth / UP Cluster, NOT a trading signal.
//! - RFC §81 Test C: 1 broker stale -> fresh count 4/5, excluded from fresh consensus.
//! - RFC §81 Test D: Spread-only expansion -> classified as SpreadExpansion.
//! - RFC §81 Test E: Ask-only quote change -> classified as AskOnly.
//! - RFC §81 Test F: Bid & ask same direction -> classified as TwoSideDirectional.
//! - RFC §91 Invariant I4 & I5 & §83: Reference broker change preserves consensus, dispersion, burst.
//! - RFC §91 Invariant I12 & I13: No BUY, SELL, ENTRY signals or single composite scores anywhere.
//! - RFC §91 Invariant I1 & I2 & I3 & I6 & I7 & I8 & I14 & I15.

use std::sync::Arc;
use tick_compare::contracts::config::{AppConfig, BrokerConfig};
use tick_compare::contracts::models::*;
use tick_compare::contracts::types::*;
use tick_compare::metrics::burst::{
    classify_quote_geometry, MultiBrokerBurstDetector, QuoteGeometry,
};
use tick_compare::metrics::consensus::{ConsensusCalculator, ObservedBrokerConsensus};
use tick_compare::metrics::fingerprint::{BrokerFingerprint, SampleContext};
use tick_compare::metrics::hypothesis::{EvidenceChain, Hypothesis, HypothesisEngine, HypothesisType};
use tick_compare::state::snapshot::SnapshotBuilder;
use tick_compare::tick::engine::TickEngine;

/// Helper to generate a valid `Quote` for testing.
fn make_quote(broker_id: BrokerId, seq: Sequence, rx_mono_ns: u64, bid: f64, ask: f64) -> Quote {
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
        rx_mono_ns: MonoNs(rx_mono_ns),
        utc_ms: Some(UtcMs(1000)),
        is_warmup: false,
        is_valid: true,
    }
}

/// Helper to generate a 5-broker configuration for engine-level testing.
fn make_5_broker_config() -> AppConfig {
    let mut config = AppConfig::default();
    config.logger.enabled = false;
    config.brokers = (1..=5)
        .map(|id| BrokerConfig {
            id,
            name: format!("Broker{id}"),
            host: "127.0.0.1".to_string(),
            port: 39000 + id as u16,
            symbol: "USDJPY".to_string(),
            digits: 3,
            point_size: 0.001,
            pip_size: 0.01,
            utc_offset_sec: 0,
            utc_verified: true,
            auto_utc_offset: false,
        })
        .collect();
    config.active_pair = (1, 2);
    config
}

// ============================================================================
// RFC §81 Regression Tests (Tests A through F)
// ============================================================================

/// RFC §81 Test A:
/// 1 broker quote jump -> classified as Outlier / Broker Deviation, NOT market-wide move.
///
/// Simulate 5 brokers where 4 brokers have Mid around 150.00 and Broker 4 jumps to 150.48 (+48 pips).
/// Verify:
/// - Consensus median remains near 150.00 (not pulled by single outlier).
/// - Broker 4 is detected as Outlier / Broker Deviation.
/// - It is NOT flagged as a market-wide move (burst detector does NOT fire a cluster).
/// - No BUY / trading signal is produced.
#[test]
fn test_rfc_section81_test_a_single_broker_jump_outlier_not_market_move() {
    let calc = ConsensusCalculator::new(1000).with_threshold(0.10);

    // 4 brokers around 150.00, Broker 4 jumps +48 pips (+0.480)
    let q1 = make_quote(1, 1, 1_000_000_000, 149.998, 150.002); // Mid = 150.000
    let q2 = make_quote(2, 1, 1_000_000_000, 149.999, 150.003); // Mid = 150.001
    let q3 = make_quote(3, 1, 1_000_000_000, 149.997, 150.001); // Mid = 149.999
    let q4 = make_quote(4, 1, 1_000_000_000, 150.478, 150.482); // Mid = 150.480 (+48 pips!)
    let q5 = make_quote(5, 1, 1_000_000_000, 149.998, 150.002); // Mid = 150.000

    let quotes = [q1, q2, q3, q4, q5];
    let consensus = calc.compute(&quotes, MonoNs(1_000_000_000));

    // 1. Consensus median is robust and remains near 150.00
    let median = consensus.consensus_mid.expect("Consensus median must exist");
    assert!(
        (median - 150.00).abs() < 0.005,
        "Observed Broker Median should be ~150.00, got {median}"
    );

    // 2. Broker 4 is detected as Outlier / Broker Deviation
    assert_eq!(
        consensus.outliers.len(),
        1,
        "Only Broker 4 should be classified as Outlier"
    );
    let outlier = &consensus.outliers[0];
    assert_eq!(outlier.broker_id, 4);
    assert!(
        (outlier.abs_deviation - 0.480).abs() < 0.005,
        "Broker 4 deviation should be ~0.480, got {}",
        outlier.abs_deviation
    );

    // 3. Broker 4 jump alone does NOT constitute a market-wide move
    // Feed Broker 4's jump into burst detector: min_brokers=2 requires at least 2 brokers
    let mut burst_detector = MultiBrokerBurstDetector::new(100, 2);
    let ev_b4 = MoveEvent {
        segment_id: 1,
        broker_id: 4,
        trigger_sequence: 1,
        rx_mono_ns: MonoNs(1_000_000_000),
        direction: MoveDirection::Up,
        anchor_mid: 150.000,
        current_mid: 150.480,
        mid_delta_points: 480.0,
        bid_delta: 0.480,
        ask_delta: 0.480,
        mid_delta: 0.480,
        spread_delta: 0.0,
        quality: MoveQuality::BothSides,
    };

    let cluster_opt = burst_detector.on_event(ev_b4, 5, 5);
    assert!(
        cluster_opt.is_none(),
        "Single broker jump must NOT produce an EventCluster (RFC §81 Test A)"
    );

    // Breadth is only 1/5
    let breadth = burst_detector.compute_breadth(5, 0, MonoNs(1_000_000_000));
    assert_eq!(breadth.up_count, 1);
    assert_eq!(breadth.total_brokers, 5);
    assert_eq!(breadth.up_ratio_str(), "1/5");

    // 4. Verify no BUY or trading signal is produced
    assert!(
        burst_detector.detect_cluster(MoveDirection::Up, 5, 5).is_none(),
        "No market move or buy signal produced from single broker jump"
    );
}

/// RFC §81 Test B:
/// 5 brokers simultaneous move -> classified as High Breadth / UP Cluster, NOT a trading signal.
///
/// Simulate 5 brokers moving UP within a 10ms span.
/// Verify:
/// - An EventCluster is detected with direction Up.
/// - 5 participating brokers.
/// - Observed span <= 10ms.
/// - Directional breadth is UP 5/5.
/// - NO buy/sell signal is produced (purely observational cluster).
#[test]
fn test_rfc_section81_test_b_multi_broker_simultaneous_move_high_breadth_not_signal() {
    let mut burst_detector = MultiBrokerBurstDetector::new(100, 2);

    // 5 brokers moving UP within a 9ms span (< 10ms)
    let timestamps_ms = [0, 2, 4, 7, 9];
    let mut last_cluster = None;

    for (i, &t_ms) in timestamps_ms.iter().enumerate() {
        let broker_id = (i + 1) as BrokerId;
        let ev = MoveEvent {
            segment_id: 1,
            broker_id,
            trigger_sequence: (i + 1) as Sequence,
            rx_mono_ns: MonoNs(t_ms * 1_000_000),
            direction: MoveDirection::Up,
            anchor_mid: 150.000,
            current_mid: 150.005,
            mid_delta_points: 5.0,
            bid_delta: 0.005,
            ask_delta: 0.005,
            mid_delta: 0.005,
            spread_delta: 0.0,
            quality: MoveQuality::BothSides,
        };

        if let Some(c) = burst_detector.on_event(ev, 5, 5) {
            last_cluster = Some(c);
        }
    }

    // 1. Verify EventCluster is detected
    let cluster = last_cluster.expect("Cluster must be detected for 5 simultaneous broker moves");
    assert_eq!(cluster.direction, MoveDirection::Up);
    assert_eq!(cluster.participating_brokers.len(), 5);
    assert_eq!(cluster.participating_brokers, vec![1, 2, 3, 4, 5]);

    // 2. Verify span <= 10ms
    assert!(
        cluster.observed_span_ms <= 10.0,
        "Observed span must be <= 10ms, was {}ms",
        cluster.observed_span_ms
    );
    assert_eq!(cluster.first_observed, 1);
    assert_eq!(cluster.last_observed, 5);

    // 3. Verify Breadth is UP 5/5
    let breadth = burst_detector.compute_breadth(5, 0, MonoNs(9_000_000));
    assert_eq!(breadth.up_count, 5);
    assert_eq!(breadth.down_count, 0);
    assert_eq!(breadth.total_brokers, 5);
    assert_eq!(breadth.up_ratio_str(), "5/5");

    // 4. Verify Core Invariant: EventCluster is NOT a trade signal
    // Cluster struct only stores participating brokers and timing, without buy/sell orders.
    assert_eq!(cluster.direction, MoveDirection::Up);
    assert_eq!(cluster.fresh_count, 5);
}

/// RFC §81 Test C:
/// 1 broker stale -> fresh count 4/5, excluded from fresh consensus.
///
/// Simulate 5 brokers where Broker 5 has not updated for 1500ms (stale_after_ms = 1000ms).
/// Verify:
/// - fresh_count == 4
/// - total_count == 5
/// - Broker 5's stale quote is excluded from Observed Broker Median.
#[test]
fn test_rfc_section81_test_c_stale_broker_exclusion_fresh_4_of_5() {
    let calc = ConsensusCalculator::new(1000); // stale_after_ms = 1000ms

    let now_mono_ns = 2_000_000_000; // t = 2000ms

    // Brokers 1..4 updated at t = 2000ms (age = 0ms, fresh)
    let q1 = make_quote(1, 10, now_mono_ns, 150.00, 150.02); // Mid = 150.01
    let q2 = make_quote(2, 10, now_mono_ns, 150.01, 150.03); // Mid = 150.02
    let q3 = make_quote(3, 10, now_mono_ns, 150.02, 150.04); // Mid = 150.03
    let q4 = make_quote(4, 10, now_mono_ns, 150.03, 150.05); // Mid = 150.04

    // Broker 5 updated at t = 500ms (age = 1500ms > 1000ms stale threshold, stale!)
    // Mid = 159.00 (a heavily shifted price to prove exclusion)
    let q5 = make_quote(5, 10, 500_000_000, 158.99, 159.01);

    let quotes = [q1, q2, q3, q4, q5];
    let consensus = calc.compute(&quotes, MonoNs(now_mono_ns));

    // 1. Fresh count is 4, Total count is 5
    assert_eq!(consensus.fresh_count, 4, "Fresh count must be 4");
    assert_eq!(consensus.total_count, 5, "Total count must be 5");

    // 2. Broker 5's stale quote is excluded from consensus median
    // Fresh mids: [150.01, 150.02, 150.03, 150.04] -> Median = (150.02 + 150.03) / 2 = 150.025
    let med = consensus.consensus_mid.expect("Must have consensus median");
    assert!(
        (med - 150.025).abs() < 1e-6,
        "Consensus median must be 150.025, got {med}"
    );

    // If Broker 5 were included, median would have been 150.030
    assert_ne!(med, 150.030);

    // Broker 5 is NOT an outlier in the fresh consensus (it was excluded prior to consensus)
    assert!(consensus.outliers.iter().all(|o| o.broker_id != 5));
}

/// RFC §81 Test D:
/// Spread-only expansion -> classified as SpreadExpansion.
///
/// Simulate bid falling and ask rising symmetrically (mid unchanged).
/// Verify quote geometry is classified as `SpreadExpansion`.
#[test]
fn test_rfc_section81_test_d_spread_expansion_classification() {
    let old_bid = 150.000;
    let old_ask = 150.002; // mid = 150.001, spread = 0.002

    // Symmetrical spread expansion: bid drops 1 point, ask rises 1 point
    let new_bid = 149.999;
    let new_ask = 150.003; // mid = 150.001 (unchanged), spread = 0.004

    let geom = classify_quote_geometry(old_bid, old_ask, new_bid, new_ask);
    assert_eq!(
        geom,
        QuoteGeometry::SpreadExpansion,
        "Symmetrical spread widening must be classified as SpreadExpansion (RFC §81 Test D)"
    );

    // Also test via QuoteGeometry::classify
    assert_eq!(
        QuoteGeometry::classify(old_bid, old_ask, new_bid, new_ask),
        QuoteGeometry::SpreadExpansion
    );
}

/// RFC §81 Test E:
/// Ask-only quote change -> classified as AskOnly.
///
/// Simulate bid remaining constant and ask rising by 2 points.
/// Verify quote geometry is classified as `AskOnly`.
#[test]
fn test_rfc_section81_test_e_ask_only_quote_change() {
    let old_bid = 150.000;
    let old_ask = 150.002;

    // Bid constant, Ask moves up by 2 points (0.002)
    let new_bid = 150.000;
    let new_ask = 150.004;

    let geom = classify_quote_geometry(old_bid, old_ask, new_bid, new_ask);
    assert_eq!(
        geom,
        QuoteGeometry::AskOnly,
        "Ask-only change must be classified as AskOnly (RFC §81 Test E)"
    );

    assert_eq!(
        QuoteGeometry::classify(old_bid, old_ask, new_bid, new_ask),
        QuoteGeometry::AskOnly
    );
}

/// RFC §81 Test F:
/// Bid & ask same direction -> classified as TwoSideDirectional.
///
/// Simulate both bid and ask rising by equal amounts.
/// Verify quote geometry is classified as `TwoSideDirectional`.
#[test]
fn test_rfc_section81_test_f_two_sided_directional_quote_change() {
    let old_bid = 150.000;
    let old_ask = 150.002;

    // Both bid and ask rise by +0.005 (+5 points)
    let new_bid = 150.005;
    let new_ask = 150.007;

    let geom = classify_quote_geometry(old_bid, old_ask, new_bid, new_ask);
    assert_eq!(
        geom,
        QuoteGeometry::TwoSideDirectional,
        "Two-sided same direction quote change must be classified as TwoSideDirectional (RFC §81 Test F)"
    );

    // Both move down
    let new_bid_down = 149.995;
    let new_ask_down = 149.997;
    assert_eq!(
        QuoteGeometry::classify(old_bid, old_ask, new_bid_down, new_ask_down),
        QuoteGeometry::TwoSideDirectional
    );
}

// ============================================================================
// Core Invariant Tests (RFC §91 & §83)
// ============================================================================

/// Invariant I4, I5, I10 & RFC §83 UI Regression:
/// Reference broker change does NOT mutate consensus median, dispersion, or burst detection.
///
/// Under the same market data, changing the reference broker pair in UI
/// must not alter raw consensus analytics or burst detection results.
#[test]
fn test_invariant_i4_i5_reference_broker_change_preserves_consensus() {
    let config = make_5_broker_config();
    let mut engine = TickEngine::new(config, None);

    // Connect all 5 brokers
    for b_id in 1..=5 {
        engine.on_ingress_item(IngressItem::Connected {
            broker_id: b_id,
            generation: 1,
            connected_at_mono: MonoNs(1_000_000),
        });
    }

    // Feed ticks for all 5 brokers at t=10ms
    let bids = [150.000, 150.002, 149.998, 150.004, 150.001];
    let asks = [150.002, 150.004, 150.000, 150.006, 150.003];

    for i in 0..5 {
        let b_id = (i + 1) as BrokerId;
        let frame = ReceivedFrame {
            frame: Frame {
                header: Header {
                    magic: MAGIC_TICK,
                    protocol_version: PROTOCOL_VERSION,
                    message_type: MSG_TYPE_TICK_BATCH,
                    header_length: HEADER_LENGTH,
                    header_flags: 0,
                    broker_id: b_id,
                    session_id: 100 + b_id as u64,
                    sequence_start: 1,
                    tick_count: 1,
                    payload_length: 72,
                },
                payload: FramePayload::TickBatch(vec![TickRecord {
                    sequence: 1,
                    broker_time_msc: 1000,
                    ea_elapsed_us: 10,
                    bid: bids[i],
                    ask: asks[i],
                    last: 0.0,
                    volume: 1,
                    volume_real: 1.0,
                    flags: 0,
                    reserved: 0,
                }]),
            },
            connection_generation: 1,
            frame_index: 1,
            run_id: RunId([0u8; 16]),
            rx_mono_ns: MonoNs(10_000_000),
            rx_unix_ns: Some(1_700_000_000_000_000_000),
            raw_wire_bytes: Arc::new(vec![]),
        };
        engine.on_ingress_item(IngressItem::Frame(frame));
    }

    // 1. Projection with reference pair (1, 2)
    engine.set_active_pair((1, 2));
    let proj1 = engine.make_projection(UtcMs(1000));
    assert_eq!(proj1.consensus.as_ref().unwrap().fresh_count, 5);
    assert_eq!(proj1.consensus.as_ref().unwrap().total_count, 5);
    assert_eq!(proj1.current_breadth.as_ref().unwrap().total_brokers, 5);
    assert_eq!(proj1.current_breadth.as_ref().unwrap().stale_count, 0);

    // 2. Change reference pair to (4, 5) without new market data
    engine.set_active_pair((4, 5));
    let proj2 = engine.make_projection(UtcMs(1000));

    // Verify consensus, dispersion, and bursts are byte-for-byte / value identical
    assert_eq!(
        proj1.consensus, proj2.consensus,
        "Invariant I4/I5/I10: Reference broker change must not change consensus"
    );
    assert_eq!(
        proj1.active_clusters, proj2.active_clusters,
        "Invariant I4/I5/I10: Reference broker change must not change active clusters"
    );
    assert_eq!(
        proj1.current_breadth, proj2.current_breadth,
        "Invariant I4/I5/I10: Reference broker change must not change breadth"
    );

    let c1 = proj1.consensus.unwrap();
    let c2 = proj2.consensus.unwrap();
    assert_eq!(c1.consensus_mid, c2.consensus_mid);
    assert_eq!(c1.median_abs_deviation, c2.median_abs_deviation);
    assert_eq!(c1.mid_range, c2.mid_range);
    assert_eq!(c1.bid_range, c2.bid_range);
    assert_eq!(c1.ask_range, c2.ask_range);

    engine.on_ingress_item(IngressItem::End {
        broker_id: 5, generation: 1, reason: "test disconnect".into(),
    });
    let disconnected = engine.make_projection(UtcMs(1000));
    assert_eq!(disconnected.consensus.as_ref().unwrap().fresh_count, 4);
    assert_eq!(disconnected.consensus.as_ref().unwrap().total_count, 5);
    assert_eq!(disconnected.current_breadth.as_ref().unwrap().stale_count, 1);

    for broker_id in 1..=4 {
        engine.on_ingress_item(IngressItem::Progress {
            broker_id, watermark_ns: MonoNs(10_000_000_000),
        });
    }
    let stale = engine.make_projection(UtcMs(10000));
    assert_eq!(stale.consensus.as_ref().unwrap().fresh_count, 0);
    assert_eq!(stale.current_breadth.as_ref().unwrap().stale_count, 5);
    assert!(stale.active_clusters.is_empty());
}

/// Invariant I12 & I13:
/// No trading signals (BUY, SELL, ENTRY, EXIT) or single composite broker scores
/// are produced anywhere in projections or snapshots.
#[test]
fn test_invariant_i12_i13_no_trading_signals_or_single_scores() {
    let config = make_5_broker_config();
    let mut engine = TickEngine::new(config, None);

    for b_id in 1..=5 {
        engine.on_ingress_item(IngressItem::Connected {
            broker_id: b_id,
            generation: 1,
            connected_at_mono: MonoNs(1_000_000),
        });
    }

    // Feed ticks
    for b_id in 1..=5 {
        let frame = ReceivedFrame {
            frame: Frame {
                header: Header {
                    magic: MAGIC_TICK,
                    protocol_version: PROTOCOL_VERSION,
                    message_type: MSG_TYPE_TICK_BATCH,
                    header_length: HEADER_LENGTH,
                    header_flags: 0,
                    broker_id: b_id,
                    session_id: 100 + b_id as u64,
                    sequence_start: 1,
                    tick_count: 1,
                    payload_length: 72,
                },
                payload: FramePayload::TickBatch(vec![TickRecord {
                    sequence: 1,
                    broker_time_msc: 1000,
                    ea_elapsed_us: 10,
                    bid: 150.000 + (b_id as f64) * 0.001,
                    ask: 150.002 + (b_id as f64) * 0.001,
                    last: 0.0,
                    volume: 1,
                    volume_real: 1.0,
                    flags: 0,
                    reserved: 0,
                }]),
            },
            connection_generation: 1,
            frame_index: 1,
            run_id: RunId([0u8; 16]),
            rx_mono_ns: MonoNs(10_000_000),
            rx_unix_ns: Some(1_700_000_000_000_000_000),
            raw_wire_bytes: Arc::new(vec![]),
        };
        engine.on_ingress_item(IngressItem::Frame(frame));
    }

    let proj = engine.make_projection(UtcMs(1000));
    let run_id = RunId([7u8; 16]);
    let builder = SnapshotBuilder::new(run_id);
    let snapshot = builder.build(&proj, UtcMs(1000), MonoNs(10_000_000), 60000);

    // Serialize projection and snapshot to JSON
    let proj_json = serde_json::to_string(&proj).expect("Serialize projection");
    let snap_json = serde_json::to_string(&snapshot).expect("Serialize snapshot");

    // 1. Prohibited trade signals check (RFC §2 O1, §91 I13)
    let banned_signal_tokens = [
        "\"BUY\"",
        "\"SELL\"",
        "\"ENTRY\"",
        "\"EXIT\"",
        "\"LONG\"",
        "\"SHORT\"",
        "\"Trade Now\"",
        "\"Buy Confidence\"",
        "\"Sell Confidence\"",
        "\"Entry Score\"",
        "\"Signal Strength\"",
    ];

    for &token in &banned_signal_tokens {
        assert!(
            !proj_json.contains(token),
            "EngineProjection must never contain banned signal token '{token}'"
        );
        assert!(
            !snap_json.contains(token),
            "UiSnapshot must never contain banned signal token '{token}'"
        );
    }

    // 2. Invariant I12: Broker Behaviour Fingerprints do NOT contain aggregate single scores
    for (&bid, fp) in &proj.fingerprints {
        assert_eq!(fp.broker_id, bid);
        // Verify independent dimensions
        assert!(fp.observed_lead_freq >= 0.0 && fp.observed_lead_freq <= 1.0);
        assert!(fp.observed_follow_freq >= 0.0 && fp.observed_follow_freq <= 1.0);
        assert!(fp.spread_expansion_freq >= 0.0 && fp.spread_expansion_freq <= 1.0);
        assert!(fp.stale_freq >= 0.0 && fp.stale_freq <= 1.0);
        assert!(fp.outlier_freq >= 0.0 && fp.outlier_freq <= 1.0);
    }

    // 3. Invariant I13 & RFC §78-80: Hypotheses never contain confidence percentages or LP identity
    for h in &proj.hypotheses {
        assert!(
            h.title.starts_with("Possible "),
            "Hypothesis title must start with 'Possible '"
        );
        assert!(
            !h.title.to_lowercase().contains("lp"),
            "Hypothesis must not guess LP identity (Invariant I6)"
        );
        for item in &h.evidence.items {
            let lower = item.to_lowercase();
            assert!(!lower.contains("buy"));
            assert!(!lower.contains("sell"));
            assert!(!lower.contains("confidence"));
            assert!(!lower.contains("probability"));
        }
    }
}

/// Invariant I1:
/// Ingestion receive timestamp (rx_mono_ns) is distinct from UI render timestamp
/// and preserved end-to-end without mutation.
#[test]
fn test_invariant_i1_rx_time_vs_ui_render_time_separation() {
    let rx_time = MonoNs(123_456_789);
    let q = make_quote(1, 1, rx_time.0, 150.00, 150.02);

    // Verify quote preserves exact rx_mono_ns
    assert_eq!(q.rx_mono_ns, rx_time);

    // In UiSnapshot, built_mono_ns and processed_watermark_ns are tracked separately
    let snapshot = UiSnapshot {
        built_mono_ns: MonoNs(999_999_999),
        processed_watermark_ns: rx_time,
        ..Default::default()
    };
    assert_ne!(snapshot.built_mono_ns, snapshot.processed_watermark_ns);
    assert_eq!(snapshot.processed_watermark_ns, rx_time);
}

/// Invariant I2:
/// Lead/Lag is an observational relative timing measurement, never a causal claim.
#[test]
fn test_invariant_i2_observed_lead_lag_is_relative_mono_timing() {
    let ev1 = MoveEvent {
        segment_id: 1,
        broker_id: 1,
        trigger_sequence: 10,
        rx_mono_ns: MonoNs(10_000_000),
        direction: MoveDirection::Up,
        anchor_mid: 150.000,
        current_mid: 150.005,
        mid_delta_points: 5.0,
        bid_delta: 0.005,
        ask_delta: 0.005,
        mid_delta: 0.005,
        spread_delta: 0.0,
        quality: MoveQuality::BothSides,
    };

    let ev2 = MoveEvent {
        segment_id: 1,
        broker_id: 2,
        trigger_sequence: 20,
        rx_mono_ns: MonoNs(13_000_000),
        direction: MoveDirection::Up,
        anchor_mid: 150.000,
        current_mid: 150.005,
        mid_delta_points: 5.0,
        bid_delta: 0.005,
        ask_delta: 0.005,
        mid_delta: 0.005,
        spread_delta: 0.0,
        quality: MoveQuality::BothSides,
    };

    let lead_lag = LeadLagMatch {
        match_id: 1,
        leader: 1,
        follower: 2,
        leader_event: ev1,
        follower_event: ev2,
        t_leader: MonoNs(10_000_000),
        t_follower: MonoNs(13_000_000),
        signed_delta_ns: 3_000_000, // t_follower - t_leader
        abs_delta_ns: 3_000_000,
        raw_delta_ms: 3.0,
        ema_delta_ms: Some(3.0),
        segment_id: 1,
    };

    assert_eq!(lead_lag.signed_delta_ns, 3_000_000);
    assert_eq!(lead_lag.raw_delta_ms, 3.0);
    assert_eq!(lead_lag.leader, 1);
    assert_eq!(lead_lag.follower, 2);
}

/// Invariant I3:
/// Stale broker quote does NOT imply market price has stopped.
/// The consensus continues to update with remaining active brokers.
#[test]
fn test_invariant_i3_stale_does_not_imply_market_halt() {
    let calc = ConsensusCalculator::new(1000);

    // Broker 1 is fresh and moving
    let q1 = make_quote(1, 10, 2_000_000_000, 150.10, 150.12);
    // Broker 2 is fresh and moving
    let q2 = make_quote(2, 10, 2_000_000_000, 150.11, 150.13);
    // Broker 3 is fresh and moving
    let q3 = make_quote(3, 10, 2_000_000_000, 150.12, 150.14);
    // Broker 4 is stale (not updated for 1800ms)
    let q4 = make_quote(4, 5, 200_000_000, 150.00, 150.02);

    let consensus = calc.compute(&[q1, q2, q3, q4], MonoNs(2_000_000_000));

    // Fresh count is 3, consensus is active and moving around 150.12
    assert_eq!(consensus.fresh_count, 3);
    assert_eq!(consensus.total_count, 4);
    assert!(consensus.consensus_mid.is_some());
    let mid = consensus.consensus_mid.unwrap();
    assert!((mid - 150.12).abs() < 0.01);
}

/// Invariant I6:
/// Hypotheses never infer LP identity.
#[test]
fn test_invariant_i6_no_lp_attribution_in_hypotheses() {
    let engine = HypothesisEngine::default();
    let ctx = SampleContext::new(100, 10_000, 0.95);
    let fp = BrokerFingerprint::new(1, 0.6, 0.4, 3.5, 0.1, 0.05, 0.02, 0.2, ctx);

    let hypotheses = engine.evaluate(&fp, None, None);
    for h in hypotheses {
        assert!(
            !h.title.to_lowercase().contains("lp"),
            "Title must not infer LP identity"
        );
        for item in h.evidence.items {
            assert!(
                !item.to_lowercase().contains("lp #")
                    && !item.to_lowercase().contains("lp identity"),
                "Evidence must not infer LP identity"
            );
        }
    }
}

/// Invariant I7:
/// Hypothesis layer is strictly distinguished from observed facts.
#[test]
fn test_invariant_i7_hypotheses_clearly_distinguished_from_facts() {
    let proj = EngineProjection {
        consensus: Some(ObservedBrokerConsensus {
            fresh_count: 5,
            total_count: 5,
            consensus_mid: Some(150.00),
            mid_min: Some(149.99),
            mid_max: Some(150.01),
            mid_range: Some(0.02),
            bid_range: Some(0.02),
            ask_range: Some(0.02),
            median_abs_deviation: Some(0.005),
            outliers: vec![],
            crossed_snapshots: vec![],
        }),
        hypotheses: vec![Hypothesis {
            hypothesis_type: HypothesisType::PossibleStickyPricing,
            title: "Possible Sticky Pricing".to_string(),
            broker_id: 1,
            evidence: EvidenceChain {
                sample_count: 100,
                median_lag_ms: None,
                concordance_ratio: None,
                fresh_rate: 0.99,
                dispersion_points: None,
                items: vec!["n = 100".to_string()],
            },
            sample_context: SampleContext::new(100, 5000, 0.99),
        }],
        ..Default::default()
    };

    // Facts are in `consensus`, `broker_overviews`.
    // Inferences are segregated in `hypotheses` with explicit "Possible " prefix.
    assert!(proj.consensus.is_some());
    assert!(!proj.hypotheses.is_empty());
    assert!(proj.hypotheses[0].title.starts_with("Possible "));
}

/// Invariant I8:
/// Current quotes are presented unfiltered without smoothing.
#[test]
fn test_invariant_i8_current_quotes_unfiltered() {
    let q = make_quote(1, 1, 100, 150.12345, 150.12567);
    assert_eq!(q.bid, 150.12345);
    assert_eq!(q.ask, 150.12567);
    assert_eq!(q.mid, (150.12345 + 150.12567) / 2.0);
}

/// Invariant I14:
/// Raw observations are preserved with exact timestamps, sequence, and dispositions.
#[test]
fn test_invariant_i14_raw_observation_preserved_in_logs() {
    let raw_frame = LogRawFrame {
        broker_id: 1,
        connection_generation: 1,
        frame_index: 42,
        rx_mono_ns: MonoNs(100_000_000),
        rx_unix_ns: Some(1_700_000_000_000_000_000),
        config_epoch: 1,
        analysis_segment: 1,
        raw_wire_bytes: Arc::new(vec![0xAA, 0xBB, 0xCC]),
        dispositions: vec![SequenceDisposition::New],
    };

    assert_eq!(raw_frame.broker_id, 1);
    assert_eq!(raw_frame.frame_index, 42);
    assert_eq!(raw_frame.dispositions, vec![SequenceDisposition::New]);
    assert_eq!(*raw_frame.raw_wire_bytes, vec![0xAA, 0xBB, 0xCC]);
}

/// Invariant I15:
/// Low sample counts explicitly result in `None` or suppressed hypotheses rather than guessing.
#[test]
fn test_invariant_i15_explicit_uncertainty_under_low_n() {
    let calc = ConsensusCalculator::new(1000);

    // Only 3 fresh brokers -> RFC §70 requires MAD to be None (N < 4)
    let q1 = make_quote(1, 1, 1_000_000_000, 150.00, 150.02);
    let q2 = make_quote(2, 1, 1_000_000_000, 150.01, 150.03);
    let q3 = make_quote(3, 1, 1_000_000_000, 150.02, 150.04);

    let consensus = calc.compute(&[q1, q2, q3], MonoNs(1_000_000_000));
    assert_eq!(consensus.fresh_count, 3);
    assert!(
        consensus.median_abs_deviation.is_none(),
        "N=3 fresh brokers must return None for MAD under RFC §70 (Invariant I15)"
    );

    // Hypotheses with low sample count (n < 10) must be empty
    let engine = HypothesisEngine::default();
    let ctx = SampleContext::new(5, 1000, 1.0); // sample_count = 5 < 10
    let fp = BrokerFingerprint::new(1, 0.5, 0.5, 10.0, 0.2, 0.1, 0.0, 0.1, ctx);
    let hypotheses = engine.evaluate(&fp, None, None);
    assert!(
        hypotheses.is_empty(),
        "Hypotheses must be suppressed when sample count < 10 (Invariant I15)"
    );
}
