//! Timestamps and calendar dates.
//!
//! * Timestamps are stored and returned as ISO-8601 UTC with milliseconds
//!   (`2026-10-14T05:30:00.000Z`), which is what the reference's JSON shows for `Date`s.
//! * "Today" is the calendar date in the main PC's own time zone (Windows settings, daylight
//!   saving included) — approved deviation D5, widened by D25 so the app works in any
//!   country; the reference used the UTC date. On a PC set to Sri Lanka time this is the
//!   Sri Lanka date, as before.

use chrono::{DateTime, Local, NaiveDate, SecondsFormat, Utc};

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
    t.with_timezone(&Local).date_naive()
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
    fn local_date_follows_this_pcs_time_zone() {
        let t = Utc.with_ymd_and_hms(2026, 4, 30, 18, 29, 59).single();
        let expected = t.map(|t| t.with_timezone(&Local).date_naive());
        assert_eq!(t.map(local_date), expected);
        assert_eq!(today_local(), Local::now().date_naive());
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
