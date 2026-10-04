//! UTC time as seconds since the Unix epoch, without a date-time crate.

use std::time::{SystemTime, UNIX_EPOCH};

const SECONDS_PER_DAY: i64 = 86_400;

pub fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs() as i64)
}

/// Days from 1970-01-01 to a Gregorian date (Howard Hinnant's algorithm).
fn days_from_civil(year: i32, month: u32, day: u32) -> i64 {
    let year = i64::from(year) - i64::from(month <= 2);
    let era = year.div_euclid(400);
    let year_of_era = year.rem_euclid(400);
    let month_from_march = (i64::from(month) + 9) % 12;
    let day_of_year = (153 * month_from_march + 2) / 5 + i64::from(day) - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// The Gregorian date `days` after 1970-01-01, as (year, month, day).
fn civil_from_days(days: i64) -> (i32, u32, u32) {
    let days = days + 719_468;
    let era = days.div_euclid(146_097);
    let day_of_era = days.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_from_march = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_from_march + 2) / 5 + 1;
    let month = if month_from_march < 10 { month_from_march + 3 } else { month_from_march - 9 };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year as i32, month as u32, day as u32)
}

pub fn from_utc(year: i32, month: u32, day: u32, hour: u32, minute: u32) -> i64 {
    days_from_civil(year, month, day) * SECONDS_PER_DAY
        + i64::from(hour) * 3600
        + i64::from(minute) * 60
}

/// The UTC date of a time, as `YYYY-MM-DD`.
pub fn date_string(unix_seconds: i64) -> String {
    let (year, month, day) = civil_from_days(unix_seconds.div_euclid(SECONDS_PER_DAY));
    format!("{year:04}-{month:02}-{day:02}")
}

/// Month number from an English name or three-letter abbreviation.
pub fn month_number(name: &str) -> Option<u32> {
    const MONTHS: [&str; 12] =
        ["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"];
    let prefix = name.get(..3)?.to_ascii_lowercase();
    MONTHS.iter().position(|m| *m == prefix).map(|i| i as u32 + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_dates_both_ways() {
        assert_eq!(from_utc(1970, 1, 1, 0, 0), 0);
        assert_eq!(from_utc(2000, 3, 1, 0, 0), 951_868_800);
        assert_eq!(from_utc(2026, 10, 4, 15, 5), 1_791_126_300);
        assert_eq!(date_string(0), "1970-01-01");
        assert_eq!(date_string(951_868_800 - 1), "2000-02-29");
        assert_eq!(date_string(1_791_126_300), "2026-10-04");
    }

    #[test]
    fn reads_month_names() {
        assert_eq!(month_number("Oct"), Some(10));
        assert_eq!(month_number("October"), Some(10));
        assert_eq!(month_number("SEP"), Some(9));
        assert_eq!(month_number("Xyz"), None);
        assert_eq!(month_number("O"), None);
    }
}
