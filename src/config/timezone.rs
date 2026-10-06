//! Timezone and Daylight Saving Time (DST) resolution for broker time alignment.

use crate::core::civil_date::{nth_sunday_of_month, unix_sec_to_ymd, ymd_hms_to_unix_sec};
use serde::{Deserialize, Serialize};

/// Broker timezone specification.
///
/// Almost all MT4/MT5 brokers globally align their server clocks to New York Close
/// (US Eastern DST: UTC+3 in summer, UTC+2 in winter) so that daily charts form
/// exactly 5 candles per week without a Sunday candle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TimezoneRule {
    /// Standard MetaTrader New York Close server time.
    /// Automatically resolves to UTC+3 (US DST / Summer) or UTC+2 (Standard Time / Winter).
    #[default]
    #[serde(alias = "NYClose", alias = "nyclose", alias = "US/Eastern", alias = "ny_close")]
    NyClose,
    /// Fixed UTC offset in seconds (uses configured `utc_offset_sec`).
    #[serde(alias = "Fixed", alias = "fixed")]
    Fixed,
    /// Japan Standard Time (UTC+9, 32400s, no DST).
    #[serde(alias = "JST", alias = "jst", alias = "Japan")]
    Jst,
    /// Coordinated Universal Time (UTC+0, 0s, no DST).
    #[serde(alias = "UTC", alias = "utc", alias = "GMT")]
    Utc,
}

impl TimezoneRule {
    /// Resolves the UTC offset in seconds for the given Unix timestamp (in seconds).
    pub fn resolve_offset(&self, unix_sec: i64, fallback_offset: i32) -> i32 {
        match self {
            TimezoneRule::NyClose => {
                if is_us_dst(unix_sec) {
                    10800 // +3h (US Summer Time)
                } else {
                    7200 // +2h (US Winter / Standard Time)
                }
            }
            TimezoneRule::Fixed => fallback_offset,
            TimezoneRule::Jst => 32400,
            TimezoneRule::Utc => 0,
        }
    }

    /// Display label for UI and logs.
    pub fn label(&self) -> &'static str {
        match self {
            TimezoneRule::NyClose => "NY Close (Auto DST)",
            TimezoneRule::Fixed => "Fixed",
            TimezoneRule::Jst => "JST (UTC+9)",
            TimezoneRule::Utc => "UTC (UTC+0)",
        }
    }
}

/// Returns true if the given Unix timestamp (in seconds) falls within
/// United States Daylight Saving Time (DST).
///
/// Rules (Energy Policy Act of 2005):
/// - Starts: Second Sunday in March at 02:00 local standard time (07:00 UTC)
/// - Ends: First Sunday in November at 02:00 local daylight time (06:00 UTC)
pub fn is_us_dst(unix_sec: i64) -> bool {
    let (year, month, _) = unix_sec_to_ymd(unix_sec);

    if month < 3 || month > 11 {
        return false;
    }
    if month > 3 && month < 11 {
        return true;
    }

    // In March: DST starts on the 2nd Sunday at 07:00 UTC
    if month == 3 {
        let second_sunday = nth_sunday_of_month(year, 3, 2);
        let dst_start_unix = ymd_hms_to_unix_sec(year, 3, second_sunday, 7, 0, 0);
        return unix_sec >= dst_start_unix;
    }

    // In November: DST ends on the 1st Sunday at 06:00 UTC
    if month == 11 {
        let first_sunday = nth_sunday_of_month(year, 11, 1);
        let dst_end_unix = ymd_hms_to_unix_sec(year, 11, first_sunday, 6, 0, 0);
        return unix_sec < dst_end_unix;
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_us_dst_transitions_2026() {
        // 2026: March 8 (2nd Sunday) at 07:00 UTC -> DST begins
        let t_before_march = ymd_hms_to_unix_sec(2026, 3, 8, 6, 59, 59);
        let t_at_march = ymd_hms_to_unix_sec(2026, 3, 8, 7, 0, 0);
        assert!(!is_us_dst(t_before_march));
        assert!(is_us_dst(t_at_march));

        // Summer time (e.g. today 2026-09-30)
        let t_today = ymd_hms_to_unix_sec(2026, 9, 30, 12, 0, 0);
        assert!(is_us_dst(t_today));
        assert_eq!(TimezoneRule::NyClose.resolve_offset(t_today, 0), 10800);

        // 2026: November 1 (1st Sunday) at 06:00 UTC -> DST ends
        let t_before_nov = ymd_hms_to_unix_sec(2026, 11, 1, 5, 59, 59);
        let t_at_nov = ymd_hms_to_unix_sec(2026, 11, 1, 6, 0, 0);
        assert!(is_us_dst(t_before_nov));
        assert!(!is_us_dst(t_at_nov));
        assert_eq!(TimezoneRule::NyClose.resolve_offset(t_at_nov, 0), 7200);

        // Winter (December 2026)
        let t_dec = ymd_hms_to_unix_sec(2026, 12, 25, 0, 0, 0);
        assert!(!is_us_dst(t_dec));
        assert_eq!(TimezoneRule::NyClose.resolve_offset(t_dec, 0), 7200);
    }
}
