//! UTC Normalizer for Observed Ticks.
//! Reference: docs/blueprint/semantics-candle.md

use crate::contracts::types::*;

pub fn normalize_tick(
    observed: &ObservedTick,
    utc_offset_sec: i32,
    verified: bool,
    normalization_epoch: u64,
) -> Result<NormalizedTick, String> {
    if !verified {
        return Err(format!(
            "UTC offset not verified for broker {}",
            observed.tick_id.broker_id
        ));
    }

    let offset_ms = (utc_offset_sec as i64)
        .checked_mul(1000)
        .ok_or_else(|| "UTC offset arithmetic overflow".to_string())?;

    let utc_ms = observed
        .record
        .broker_time_msc
        .checked_sub(offset_ms)
        .ok_or_else(|| "Broker time UTC normalization arithmetic underflow".to_string())?;

    Ok(NormalizedTick {
        observed: observed.clone(),
        utc_ms: UtcMs(utc_ms),
        normalization_epoch,
    })
}

/// Rounds raw seconds difference to the nearest hourly offset (multiple of 3600 seconds).
/// This eliminates millisecond jitter, network delays, and minor clock drift, reliably
/// identifying standard FX timezone offsets such as GMT+0, GMT+2 (7200), GMT+3 (10800), GMT+9 (32400).
pub fn round_to_hourly_offset(raw_sec: f64) -> i32 {
    let hours = (raw_sec / 3600.0).round() as i32;
    hours * 3600
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_round_to_hourly_offset() {
        assert_eq!(round_to_hourly_offset(0.0), 0);
        assert_eq!(round_to_hourly_offset(5.4), 0);
        assert_eq!(round_to_hourly_offset(-12.0), 0);

        // Winter time GMT+2 (7200 sec) with jitter
        assert_eq!(round_to_hourly_offset(7201.5), 7200);
        assert_eq!(round_to_hourly_offset(7190.0), 7200);

        // Summer time GMT+3 (10800 sec) with jitter
        assert_eq!(round_to_hourly_offset(10798.2), 10800);
        assert_eq!(round_to_hourly_offset(10815.0), 10800);

        // JST GMT+9 (32400 sec)
        assert_eq!(round_to_hourly_offset(32400.0), 32400);
    }

    #[test]
    fn test_normalization_checked_math() {
        let obs = ObservedTick {
            tick_id: TickId {
                broker_id: 1,
                session_id: 1,
                sequence: 0,
            },
            record: TickRecord {
                sequence: 0,
                broker_time_msc: 10_000,
                ea_elapsed_us: 100,
                bid: 150.0,
                ask: 150.02,
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
        };

        let norm = normalize_tick(&obs, 2, true, 1).unwrap();
        assert_eq!(norm.utc_ms, UtcMs(8_000)); // 10,000 - 2,000 = 8,000

        let unverified = normalize_tick(&obs, 2, false, 1);
        assert!(unverified.is_err());
    }
}
