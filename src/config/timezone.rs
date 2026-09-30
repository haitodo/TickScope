//! Timezone and Daylight Saving Time (DST) resolution for broker time alignment.

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

/// Converts Unix timestamp in seconds to (year, month, day).
pub fn unix_sec_to_ymd(unix_sec: i64) -> (i32, u32, u32) {
    let days = if unix_sec >= 0 {
        unix_sec / 86400
    } else {
        (unix_sec - 86399) / 86400
    };
    days_to_ymd(days)
}

/// Converts days since Unix epoch (1970-01-01) to (year, month, day).
/// Uses Howard Hinnant's civil date algorithm.
pub fn days_to_ymd(days: i64) -> (i32, u32, u32) {
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u32;
    let yoe = (doe - doe / 1024 + doe / 1461 - doe / 36524) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if m <= 2 { y + 1 } else { y };
    (year as i32, m, d)
}

/// Converts (year, month, day) to days since Unix epoch (1970-01-01).
pub fn ymd_to_days(year: i32, month: u32, day: u32) -> i64 {
    let y = if month <= 2 { year as i64 - 1 } else { year as i64 };
    let m = if month <= 2 { month as i64 + 9 } else { month as i64 - 3 };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = (y - era * 400) as u32;
    let doy = (153 * m + 2) / 5 + day as i64 - 1;
    let doe = yoe as i64 * 365 + (yoe / 4) as i64 - (yoe / 100) as i64 + doy;
    era * 146097 + doe - 719468
}

/// Converts (year, month, day, hour, min, sec) to Unix timestamp in seconds.
pub fn ymd_hms_to_unix_sec(year: i32, month: u32, day: u32, hour: u32, min: u32, sec: u32) -> i64 {
    let days = ymd_to_days(year, month, day);
    days * 86400 + (hour as i64 * 3600) + (min as i64 * 60) + sec as i64
}

/// Returns the day of week for a given date (0 = Sunday, 1 = Monday, ..., 6 = Saturday).
pub fn day_of_week(year: i32, month: u32, day: u32) -> u32 {
    let days = ymd_to_days(year, month, day);
    let rem = (days + 4) % 7;
    if rem < 0 {
        (rem + 7) as u32
    } else {
        rem as u32
    }
}

/// Returns the day of month for the nth Sunday of a given month (n >= 1).
pub fn nth_sunday_of_month(year: i32, month: u32, n: u32) -> u32 {
    let first_day_dow = day_of_week(year, month, 1);
    let first_sunday = 1 + (7 - first_day_dow) % 7;
    first_sunday + (n - 1) * 7
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calendar_roundtrip() {
        let (y, m, d) = days_to_ymd(0);
        assert_eq!((y, m, d), (1970, 1, 1));
        assert_eq!(ymd_to_days(1970, 1, 1), 0);
        assert_eq!(day_of_week(1970, 1, 1), 4); // Thursday

        // Today test (2026-09-30, Wednesday)
        let days_2026_09_30 = ymd_to_days(2026, 9, 30);
        let (y2, m2, d2) = days_to_ymd(days_2026_09_30);
        assert_eq!((y2, m2, d2), (2026, 9, 30));
        assert_eq!(day_of_week(2026, 9, 30), 3); // Wednesday
    }

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
