//! Comprehensive verification of NY Close calendar-based timezone resolution.

use tick_scope::config::{load_config_from_file, TimezoneRule};
use tick_scope::tick::engine::TickEngine;

#[test]
fn test_default_config_all_brokers_use_ny_close() {
    let config = load_config_from_file("config/default.toml").expect("default.toml must parse");
    assert_eq!(config.brokers.len(), 5);

    for broker in &config.brokers {
        assert_eq!(
            broker.timezone_rule,
            TimezoneRule::NyClose,
            "Broker {} ({}) must use TimezoneRule::NyClose",
            broker.id,
            broker.name
        );
        assert!(
            !broker.auto_utc_offset,
            "Broker {} ({}) should disable per-tick auto_utc_offset",
            broker.id, broker.name
        );
    }
}

#[test]
fn test_engine_initializes_verified_offsets_at_startup() {
    use tick_scope::core::types::{NormalizationState, UtcMs};

    let config = load_config_from_file("config/default.toml").expect("default.toml must parse");
    let engine = TickEngine::new(config);
    let proj = engine.make_projection(UtcMs(0));

    assert_eq!(proj.broker_overviews.len(), 5);

    // All brokers use NyClose rule, so offset must match resolution of current time
    let now_sec = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs()) as i64;
    let expected_offset = TimezoneRule::NyClose.resolve_offset(now_sec, 0);

    // Engine must have all 5 brokers initialized with verified = true
    for b in &proj.broker_overviews {
        assert_eq!(
            b.health.normalization,
            NormalizationState::Verified,
            "Broker {} ({}) must be verified at startup (0ms)",
            b.broker_id,
            b.name
        );
        assert_eq!(
            b.active_utc_offset_sec, expected_offset,
            "Broker {} ({}) must resolve to current season offset ({expected_offset}s)",
            b.broker_id, b.name
        );
    }
}

#[test]
fn test_ny_close_dst_calendar_seasons() {
    // 2026 Summer (September): +10800s (+3h)
    let t_summer = 1_790_779_200; // 2026-09-30 12:00:00 UTC
    assert_eq!(TimezoneRule::NyClose.resolve_offset(t_summer, 0), 10800);

    // 2026 Winter (December): +7200s (+2h)
    let t_winter = 1_798_156_800; // 2026-12-25 00:00:00 UTC
    assert_eq!(TimezoneRule::NyClose.resolve_offset(t_winter, 0), 7200);

    // 2027 Summer (June): +10800s (+3h)
    let t_summer_2027 = 1_813_150_800; // 2027-06-15 13:00:00 UTC
    assert_eq!(
        TimezoneRule::NyClose.resolve_offset(t_summer_2027, 0),
        10800
    );
}
