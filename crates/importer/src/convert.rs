//! Converting PostgreSQL values (read as text) to this app's SQLite storage.

use app_core::money::Money;
use chrono::{FixedOffset, NaiveDateTime, TimeZone};

/// A value ready to bind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    Null,
    Int(i64),
    Text(String),
}

#[derive(Debug, thiserror::Error)]
#[error("{table}.{column}: cannot convert {value:?} ({why})")]
pub struct ConvertError {
    pub table: String,
    pub column: String,
    pub value: String,
    pub why: String,
}

/// Parses `+05:30`, `-03:00`, `UTC` or `Z` into a fixed offset.
pub fn parse_offset(text: &str) -> Option<FixedOffset> {
    let t = text.trim();
    if t.eq_ignore_ascii_case("utc") || t == "Z" || t == "+00:00" {
        return FixedOffset::east_opt(0);
    }
    let (sign, rest) = match t.as_bytes().first()? {
        b'+' => (1, &t[1..]),
        b'-' => (-1, &t[1..]),
        _ => return None,
    };
    let (h, m) = rest.split_once(':')?;
    let (h, m): (i32, i32) = (h.parse().ok()?, m.parse().ok()?);
    if h > 14 || !(0..60).contains(&m) {
        return None;
    }
    FixedOffset::east_opt(sign * (h * 3600 + m * 60))
}

/// `2026-04-01 10:20:30.123456` (wall clock at `offset`) → `2026-04-01T04:50:30.123Z`.
/// Fractions beyond milliseconds are dropped, as the reference's JS `Date` dropped them.
pub fn wall_clock_to_utc(text: &str, offset: FixedOffset) -> Option<String> {
    let t = text.trim();
    let naive = NaiveDateTime::parse_from_str(t, "%Y-%m-%d %H:%M:%S%.f")
        .or_else(|_| NaiveDateTime::parse_from_str(t, "%Y-%m-%d %H:%M:%S"))
        .ok()?;
    let utc = offset.from_local_datetime(&naive).single()?.with_timezone(&chrono::Utc);
    Some(utc.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string())
}

/// `timestamptz` text in the session time zone (UTC): `2026-04-01 10:20:30.123+00`.
pub fn timestamptz_to_utc(text: &str) -> Option<String> {
    let t = text.trim();
    let (body, zone) = match t.rfind(['+', '-']) {
        Some(i) if i > 10 => (&t[..i], &t[i..]),
        _ => return None,
    };
    let zone = if zone.len() == 3 { format!("{zone}:00") } else { zone.to_string() };
    wall_clock_to_utc(body, parse_offset(&zone)?)
}

/// Converts one value from its PostgreSQL type to the SQLite column's storage.
pub fn convert(
    table: &str,
    column: &str,
    pg_type: &str,
    sqlite_type: &str,
    value: Option<&str>,
    offset: FixedOffset,
) -> Result<Value, ConvertError> {
    let Some(v) = value else { return Ok(Value::Null) };
    let fail =
        |why: &str| ConvertError { table: table.into(), column: column.into(), value: v.into(), why: why.into() };
    let sqlite_int = sqlite_type.eq_ignore_ascii_case("INTEGER");
    match pg_type {
        "numeric" if sqlite_int => {
            // Money and quantities are stored in hundredths; more than two decimals would be lost.
            Money::parse_decimal(v)
                .map(|m| Value::Int(m.cents()))
                .ok_or_else(|| fail("not a number with at most 2 decimals"))
        }
        "integer" | "bigint" | "smallint" if sqlite_int => {
            v.trim().parse().map(Value::Int).map_err(|_| fail("not an integer"))
        }
        "boolean" if sqlite_int => match v {
            "true" | "t" => Ok(Value::Int(1)),
            "false" | "f" => Ok(Value::Int(0)),
            _ => Err(fail("not a boolean")),
        },
        "timestamp without time zone" => {
            wall_clock_to_utc(v, offset).map(Value::Text).ok_or_else(|| fail("not a timestamp"))
        }
        "timestamp with time zone" => timestamptz_to_utc(v).map(Value::Text).ok_or_else(|| fail("not a timestamp")),
        _ if sqlite_int => Err(fail(&format!("PostgreSQL {pg_type} into an INTEGER column"))),
        _ => Ok(Value::Text(v.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn off(t: &str) -> FixedOffset {
        parse_offset(t).unwrap_or_else(|| panic!("offset {t}"))
    }

    #[test]
    fn offsets_and_timestamps() {
        assert_eq!(parse_offset("+05:30").map(|o| o.local_minus_utc()), Some(19_800));
        assert_eq!(parse_offset("UTC").map(|o| o.local_minus_utc()), Some(0));
        assert!(parse_offset("Asia/Colombo").is_none(), "named zones are refused: give the offset");
        assert_eq!(
            wall_clock_to_utc("2026-04-01 10:20:30.123456", off("+05:30")).as_deref(),
            Some("2026-04-01T04:50:30.123Z")
        );
        assert_eq!(wall_clock_to_utc("2026-04-01 00:00:00", off("UTC")).as_deref(), Some("2026-04-01T00:00:00.000Z"));
        assert_eq!(timestamptz_to_utc("2026-04-01 10:20:30.5+00").as_deref(), Some("2026-04-01T10:20:30.500Z"));
        assert_eq!(timestamptz_to_utc("2026-04-01 10:20:30+05:30").as_deref(), Some("2026-04-01T04:50:30.000Z"));
    }

    #[test]
    fn values_convert_by_type() -> Result<(), ConvertError> {
        let z = off("UTC");
        assert_eq!(convert("t", "c", "numeric", "INTEGER", Some("83850.00"), z)?, Value::Int(8_385_000));
        assert_eq!(convert("t", "c", "numeric", "INTEGER", Some("-0.5"), z)?, Value::Int(-50));
        assert!(convert("t", "c", "numeric", "INTEGER", Some("1.005"), z).is_err(), "never round silently");
        assert_eq!(convert("t", "c", "boolean", "INTEGER", Some("true"), z)?, Value::Int(1));
        assert_eq!(convert("t", "c", "bigint", "INTEGER", Some("89"), z)?, Value::Int(89));
        assert_eq!(convert("t", "c", "jsonb", "TEXT", Some("[\"a\"]"), z)?, Value::Text("[\"a\"]".into()));
        assert_eq!(convert("t", "c", "uuid", "TEXT", None, z)?, Value::Null);
        assert!(convert("t", "c", "text", "INTEGER", Some("x"), z).is_err());
        Ok(())
    }
}
