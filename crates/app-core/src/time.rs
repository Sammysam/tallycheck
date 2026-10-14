//! Timestamps and calendar dates.
//!
//! * Timestamps are stored and returned as ISO-8601 UTC with milliseconds
//!   (`2026-10-14T05:30:00.000Z`), which is what the reference's JSON shows for `Date`s.
//! * "Today" is the Sri Lanka calendar date (UTC+05:30, no daylight saving) — approved
//!   deviation D5; the reference used the UTC date.

use chrono::{DateTime, FixedOffset, NaiveDate, SecondsFormat, Utc};

/// Sri Lanka Standard Time offset in seconds (+05:30).
const SRI_LANKA_OFFSET_SECS: i32 = 5 * 3600 + 30 * 60;

pub fn now_iso() -> String {
    to_iso(Utc::now())
}

pub fn to_iso(t: DateTime<Utc>) -> String {
    t.to_rfc3339_opts(SecondsFormat::Millis, true)
}

pub fn today_local() -> NaiveDate {
    local_date(Utc::now())
}

pub fn local_date(t: DateTime<Utc>) -> NaiveDate {
    match FixedOffset::east_opt(SRI_LANKA_OFFSET_SECS) {
        Some(offset) => t.with_timezone(&offset).date_naive(),
        None => t.date_naive(),
    }
}

/// The reference's `IsDateFormat`: exactly `YYYY-MM-DD` and a real calendar date.
pub fn is_date_format(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 10
        && b[4] == b'-'
        && b[7] == b'-'
        && b.iter().enumerate().all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit())
        && NaiveDate::parse_from_str(s, "%Y-%m-%d").is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn iso_has_millis_and_z() {
        let t = Utc.with_ymd_and_hms(2026, 4, 30, 18, 0, 0).single();
        assert_eq!(t.map(to_iso).as_deref(), Some("2026-04-30T18:00:00.000Z"));
    }

    #[test]
    fn local_date_rolls_over_at_1830_utc() {
        let before = Utc.with_ymd_and_hms(2026, 4, 30, 18, 29, 59).single();
        let after = Utc.with_ymd_and_hms(2026, 4, 30, 18, 30, 0).single();
        assert_eq!(before.map(local_date).map(|d| d.to_string()).as_deref(), Some("2026-04-30"));
        assert_eq!(after.map(local_date).map(|d| d.to_string()).as_deref(), Some("2026-05-01"));
    }

    #[test]
    fn date_format_matches_reference_validator() {
        assert!(is_date_format("2026-04-30"));
        assert!(is_date_format("2024-02-29"));
        assert!(!is_date_format("2026-02-29"));
        assert!(!is_date_format("2026-4-30"));
        assert!(!is_date_format("2026-04-30T00:00:00Z"));
        assert!(!is_date_format("+2026-04-3"));
    }
}
