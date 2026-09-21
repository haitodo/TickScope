//! Integration test verifying P0 fixtures and contracts.

#[path = "fixtures/mod.rs"]
mod fixtures;
#[path = "support/mod.rs"]
mod support;

use fixtures::*;
use support::*;
use tick_compare::config::load_config_from_file;
use tick_compare::contracts::*;

#[test]
fn test_golden_wire_fixtures_match() {
    assert_eq!(GOLDEN_HEADER_BYTES.len(), 40);
    assert_eq!(GOLDEN_TICK_RECORD_BYTES.len(), 72);

    let magic = u32::from_le_bytes(GOLDEN_HEADER_BYTES[0..4].try_into().unwrap());
    assert_eq!(magic, MAGIC_TICK);

    let version = u16::from_le_bytes(GOLDEN_HEADER_BYTES[4..6].try_into().unwrap());
    assert_eq!(version, PROTOCOL_VERSION);

    let msg_type = u16::from_le_bytes(GOLDEN_HEADER_BYTES[6..8].try_into().unwrap());
    assert_eq!(msg_type, MSG_TYPE_TICK_BATCH);

    let header_len = u16::from_le_bytes(GOLDEN_HEADER_BYTES[8..10].try_into().unwrap());
    assert_eq!(header_len, HEADER_LENGTH);

    let tick_count = u32::from_le_bytes(GOLDEN_HEADER_BYTES[32..36].try_into().unwrap());
    assert_eq!(tick_count, 1);

    let payload_len = u32::from_le_bytes(GOLDEN_HEADER_BYTES[36..40].try_into().unwrap());
    assert_eq!(payload_len, 72);
}

#[test]
fn test_storage_crc32c_fixture() {
    let crc = crc32c(CRC32C_INPUT_BYTES);
    assert_eq!(crc, CRC32C_EXPECTED_U32);
}

#[test]
fn test_fake_clock_and_sink() {
    let clock = FakeClock::new(100_000_000, 1_700_000_000_000_000_000);
    let sample1 = clock.sample();
    assert_eq!(sample1.mono_ns, MonoNs(100_000_000));

    clock.advance_mono_ms(50);
    let sample2 = clock.sample();
    assert_eq!(sample2.mono_ns, MonoNs(150_000_000));

    let sink = FakeIngressSink::new(2);
    let item = IngressItem::Progress {
        broker_id: 1,
        watermark_ns: MonoNs(150_000_000),
    };
    assert_eq!(sink.try_submit(item.clone()), SubmitResult::Accepted);
    assert_eq!(sink.try_submit(item.clone()), SubmitResult::Accepted);
    assert_eq!(sink.try_submit(item.clone()), SubmitResult::Full(item));
}

#[test]
fn test_load_default_config() {
    let config = load_config_from_file("config/default.toml").expect("Default config must load and validate");
    assert!(config.brokers.len() >= 2, "Default config has at least 2 brokers");
    assert_eq!(config.active_pair, (1, 2));
}
