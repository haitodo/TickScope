//! Calendar conversion shared by every timezone-sensitive part of the app
//! (logger timestamps, tlog records, config timezone rules, chart overlays).
//!
//! This is deliberately the only copy of the algorithm in the crate: it used to
//! be duplicated in four places, and one of those copies silently drifted.

/// Howard Hinnant's civil-date algorithm: converts days since the Unix epoch
/// (1970-01-01) into `(year, month, day)`.
///
/// The three leap-year correction divisors are 1460 (4 years), 36524 (100 years)
/// and 146096 (400 years). They must stay in this exact order — shifting them by
/// one position still compiles and still round-trips, but returns invalid dates
/// such as `2026-3-0` for the last days of February.
#[must_use]
pub fn civil_from_days(days_since_unix_epoch: i64) -> (i64, u32, u32) {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_civil_from_days_leap_years() {
        // 2000-02-29 is day 11016, 2024-02-29 is day 19782.
        assert_eq!(civil_from_days(11_016), (2000, 2, 29));
        assert_eq!(civil_from_days(19_782), (2024, 2, 29));
    }

    #[test]
    fn test_civil_from_days_known_dates() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(12), (1970, 1, 13));
        // 2026-02-28: one of the dates the old diverged copy used to break.
        assert_eq!(civil_from_days(20_512), (2026, 2, 28));
        // Negative days exercise the `z - 146_096` era branch.
        assert_eq!(civil_from_days(-1), (1969, 12, 31));
        assert_eq!(civil_from_days(-25_567), (1900, 1, 1));
    }
}
