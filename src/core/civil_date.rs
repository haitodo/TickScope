//! Calendar conversions shared by every timezone-sensitive part of the app
//! (logger timestamps, tlog records, config timezone rules, chart overlays).
//!
//! This is deliberately the only place where the crate converts between a Unix
//! day number and a civil date: the algorithm used to be duplicated in four
//! places, and one of those copies silently drifted.

/// Howard Hinnant's civil-date algorithm: converts days since the Unix epoch
/// (1970-01-01) into `(year, month, day)`.
///
/// The three leap-year correction divisors are 1460 (4 years), 36524 (100 years)
/// and 146096 (400 years). They must stay in this exact order — shifting them by
/// one position still compiles and still round-trips, but returns invalid dates
/// such as `2026-3-0` for the last days of February.
#[must_use]
pub const fn civil_from_days(days_since_unix_epoch: i64) -> (i64, u32, u32) {
    let z = days_since_unix_epoch + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    if month <= 2 {
        year += 1;
    }
    (year, month as u32, day as u32)
}

/// Inverse of [`civil_from_days`]: converts `(year, month, day)` into days since
/// the Unix epoch (1970-01-01).
#[must_use]
pub fn ymd_to_days(year: i32, month: u32, day: u32) -> i64 {
    let y = if month <= 2 {
        i64::from(year) - 1
    } else {
        i64::from(year)
    };
    let m = if month <= 2 {
        i64::from(month) + 9
    } else {
        i64::from(month) - 3
    };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = (y - era * 400) as u32;
    let doy = (153 * m + 2) / 5 + i64::from(day) - 1;
    let doe = i64::from(yoe) * 365 + i64::from(yoe / 4) - i64::from(yoe / 100) + doy;
    era * 146_097 + doe - 719_468
}

/// Converts a Unix timestamp in seconds to `(year, month, day)`.
///
/// Days are floored rather than truncated, so timestamps before 1970-01-01 map to
/// the correct date.
#[must_use]
pub const fn unix_sec_to_ymd(unix_sec: i64) -> (i32, u32, u32) {
    let (year, month, day) = civil_from_days(unix_sec.div_euclid(86_400));
    (year as i32, month, day)
}

/// Converts `(year, month, day, hour, min, sec)` to a Unix timestamp in seconds.
#[must_use]
pub fn ymd_hms_to_unix_sec(year: i32, month: u32, day: u32, hour: u32, min: u32, sec: u32) -> i64 {
    let days = ymd_to_days(year, month, day);
    days * 86_400 + (i64::from(hour) * 3_600) + (i64::from(min) * 60) + i64::from(sec)
}

/// Returns the day of week for a given date (0 = Sunday, 1 = Monday, ..., 6 = Saturday).
#[must_use]
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
#[must_use]
pub fn nth_sunday_of_month(year: i32, month: u32, n: u32) -> u32 {
    let first_day_dow = day_of_week(year, month, 1);
    let first_sunday = 1 + (7 - first_day_dow) % 7;
    first_sunday + (n - 1) * 7
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_civil_from_days_leap_years() {
        // 2000-02-29 is day 11016, 2024-02-29 is day 19782.
        assert_eq!(civil_from_days(11_016), (2000, 2, 29));
        assert_eq!(civil_from_days(19_782), (2024, 2, 29));
    }

    /// Regression: the `year_of_era` divisors used to be `doe / 1024 + doe / 1461`,
    /// which produced invalid dates (for example `2026-3-0`) for the last days of
    /// February every year, and a wrong month on ~6% of all days.
    #[test]
    fn test_civil_from_days_known_dates() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(12), (1970, 1, 13));
        assert_eq!(civil_from_days(12_112), (2003, 3, 1));
        assert_eq!(civil_from_days(19_782), (2024, 2, 29)); // was (2024, 3, 0)
        assert_eq!(civil_from_days(20_147), (2025, 2, 28)); // was (2025, 3, 0)
        assert_eq!(civil_from_days(20_512), (2026, 2, 28)); // was (2026, 3, 0)
        assert_eq!(civil_from_days(20_877), (2027, 2, 28)); // was (2027, 3, 0)
                                                            // Negative days exercise the `z - 146_096` era branch.
        assert_eq!(civil_from_days(-1), (1969, 12, 31));
        assert_eq!(civil_from_days(-25_567), (1900, 1, 1));
    }

    #[test]
    fn test_unix_sec_to_ymd_floors_negative_timestamps() {
        assert_eq!(unix_sec_to_ymd(0), (1970, 1, 1));
        assert_eq!(unix_sec_to_ymd(-1), (1969, 12, 31));
        assert_eq!(unix_sec_to_ymd(86_399), (1970, 1, 1));
        assert_eq!(unix_sec_to_ymd(1_772_323_199), (2026, 2, 28));
    }

    #[test]
    fn test_calendar_roundtrip_and_day_of_week() {
        assert_eq!(ymd_to_days(1970, 1, 1), 0);
        assert_eq!(day_of_week(1970, 1, 1), 4); // Thursday
        assert_eq!(day_of_week(2026, 9, 30), 3); // Wednesday
        assert_eq!(day_of_week(2024, 2, 29), 4); // Thursday
        assert_eq!(day_of_week(2026, 2, 28), 6); // Saturday

        let days_2026_09_30 = ymd_to_days(2026, 9, 30);
        assert_eq!(civil_from_days(days_2026_09_30), (2026, 9, 30));
    }

    fn is_leap_year(year: i64) -> bool {
        (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
    }

    /// Independent, table-based successor of a calendar date.
    fn next_day((year, month, day): (i64, u32, u32)) -> (i64, u32, u32) {
        let last_day = match month {
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            4 | 6 | 9 | 11 => 30,
            _ => {
                if is_leap_year(year) {
                    29
                } else {
                    28
                }
            }
        };
        if day < last_day {
            (year, month, day + 1)
        } else if month < 12 {
            (year, month + 1, 1)
        } else {
            (year + 1, 1, 1)
        }
    }

    /// Walks 1900-01-01 .. 2200-01-01 one day at a time and checks that the result is a
    /// dense, consecutive calendar. The reference here is a plain days-in-month table,
    /// so unlike a round-trip check it cannot drift together with the implementation.
    #[test]
    fn test_civil_from_days_is_a_dense_consecutive_calendar() {
        let mut previous = civil_from_days(-25_567);
        assert_eq!(previous, (1900, 1, 1));
        for days in -25_566_i64..=84_005 {
            let current = civil_from_days(days);
            assert_eq!(
                current,
                next_day(previous),
                "days={days} is not consecutive"
            );
            previous = current;
        }
    }
}
