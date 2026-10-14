//! Exact money: integer minor units (cents). No binary floating point is used for
//! arithmetic, storage or comparison.
//!
//! JSON compatibility with the reference:
//! * amounts read from `numeric(15,2)` columns are serialised by TypeORM/pg as strings with
//!   exactly two decimals (`"83850.00"`) — see [`Money::to_db_string`];
//! * amounts computed in JS are plain JSON numbers — see [`Money::to_json_number`], which
//!   formats from the exact decimal string (the only place an `f64` appears, for output);
//! * request bodies carry JSON numbers that must have at most 2 decimal places
//!   (class-validator `IsNumber({ maxDecimalPlaces: 2 })`) — see [`Money::from_json`].

use serde_json::{Number, Value};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Money(i64);

/// Largest magnitude a `numeric(15,2)` column can hold, in cents.
pub const MAX_NUMERIC_15_2_CENTS: i64 = 99_999_999_999_999;

impl Money {
    pub const ZERO: Money = Money(0);

    pub const fn from_cents(cents: i64) -> Self {
        Money(cents)
    }

    pub const fn cents(self) -> i64 {
        self.0
    }

    pub fn checked_add(self, other: Money) -> Option<Money> {
        self.0.checked_add(other.0).map(Money)
    }

    pub fn checked_sub(self, other: Money) -> Option<Money> {
        self.0.checked_sub(other.0).map(Money)
    }

    /// Parses an exact decimal string with at most two fraction digits, e.g. `"83850"`,
    /// `"-0.5"`, `"12.34"`. Rejects exponents, more decimals and anything else.
    pub fn parse_decimal(s: &str) -> Option<Money> {
        let s = s.trim();
        let (neg, digits) = match s.strip_prefix('-') {
            Some(rest) => (true, rest),
            None => (false, s),
        };
        let (int_part, frac_part) = match digits.split_once('.') {
            Some((i, f)) => (i, f),
            None => (digits, ""),
        };
        if int_part.is_empty() || !int_part.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        if frac_part.len() > 2 || !frac_part.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        if digits.contains('.') && frac_part.is_empty() {
            return None;
        }
        let whole: i64 = int_part.parse().ok()?;
        let mut frac: i64 = if frac_part.is_empty() { 0 } else { frac_part.parse().ok()? };
        if frac_part.len() == 1 {
            frac *= 10;
        }
        let cents = whole.checked_mul(100)?.checked_add(frac)?;
        Some(Money(if neg { -cents } else { cents }))
    }

    /// Accepts a JSON number with at most two decimal places, as class-validator's
    /// `IsNumber({ maxDecimalPlaces: 2 })` does. Strings are rejected (no implicit conversion).
    pub fn from_json(value: &Value) -> Option<Money> {
        let n = value.as_number()?;
        if let Some(i) = n.as_i64() {
            return i.checked_mul(100).map(Money);
        }
        // serde_json prints floats in their shortest round-trip form, which is the same
        // decimal text JS would show, so decimal places are counted on that text.
        let text = n.to_string();
        if text.contains(['e', 'E']) {
            return None;
        }
        Money::parse_decimal(&text)
    }

    /// `"1234.50"` — the pg/TypeORM rendering of a `numeric(15,2)` column.
    pub fn to_db_string(self) -> String {
        self.to_string()
    }

    /// A JSON number equal to the exact decimal value (as the reference's JS numbers are).
    pub fn to_json_number(self) -> Value {
        if self.0 % 100 == 0 {
            return Value::Number(Number::from(self.0 / 100));
        }
        // Parsing the exact decimal text gives the nearest double, which serde_json prints
        // back as the same shortest decimal text.
        self.to_string().parse::<f64>().ok().and_then(Number::from_f64).map(Value::Number).unwrap_or(Value::Null)
    }
}

impl fmt::Display for Money {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let sign = if self.0 < 0 { "-" } else { "" };
        let abs = self.0.unsigned_abs();
        write!(f, "{sign}{}.{:02}", abs / 100, abs % 100)
    }
}

/// `serialize_with` helper: a `numeric(15,2)` column as TypeORM returns it (`"1000.50"`).
pub fn as_db_string<S: serde::Serializer>(m: &Money, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(&m.to_db_string())
}

/// `serialize_with` helper: an amount the reference computed in JavaScript (a JSON number).
pub fn as_json_number<S: serde::Serializer>(m: &Money, s: S) -> Result<S::Ok, S::Error> {
    serde::Serialize::serialize(&m.to_json_number(), s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_exact_decimals() {
        assert_eq!(Money::parse_decimal("83850"), Some(Money(8_385_000)));
        assert_eq!(Money::parse_decimal("0.5"), Some(Money(50)));
        assert_eq!(Money::parse_decimal("12.34"), Some(Money(1234)));
        assert_eq!(Money::parse_decimal("-0.01"), Some(Money(-1)));
        assert_eq!(Money::parse_decimal("1.005"), None);
        assert_eq!(Money::parse_decimal("1e3"), None);
        assert_eq!(Money::parse_decimal("1."), None);
        assert_eq!(Money::parse_decimal(".5"), None);
        assert_eq!(Money::parse_decimal("abc"), None);
    }

    #[test]
    fn json_input_follows_max_two_decimal_places() {
        assert_eq!(Money::from_json(&json!(100)), Some(Money(10_000)));
        assert_eq!(Money::from_json(&json!(0.1)), Some(Money(10)));
        assert_eq!(Money::from_json(&json!(1234.56)), Some(Money(123_456)));
        assert_eq!(Money::from_json(&json!(1.005)), None);
        assert_eq!(Money::from_json(&json!("100")), None);
        assert_eq!(Money::from_json(&json!(null)), None);
    }

    #[test]
    fn renders_like_pg_numeric_and_js_numbers() {
        assert_eq!(Money(8_385_000).to_db_string(), "83850.00");
        assert_eq!(Money(-5).to_db_string(), "-0.05");
        assert_eq!(Money(8_385_000).to_json_number(), json!(83850));
        assert_eq!(Money(10).to_json_number().to_string(), "0.1");
        assert_eq!(Money(123_456).to_json_number().to_string(), "1234.56");
        assert_eq!(Money(MAX_NUMERIC_15_2_CENTS).to_json_number().to_string(), "999999999999.99");
    }

    #[test]
    fn every_cent_value_round_trips_through_json_number() {
        for cents in (-100_000..100_000).step_by(7) {
            let m = Money(cents);
            assert_eq!(Money::from_json(&m.to_json_number()), Some(m), "{cents}");
        }
    }
}
