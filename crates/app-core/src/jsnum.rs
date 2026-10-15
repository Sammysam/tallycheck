//! JavaScript number semantics needed for compatibility: `Number(string)` parsing and
//! `String(number)` printing. These are used for request values and configuration, never for
//! money arithmetic.

/// JavaScript `Number(string)`. `None` stands for NaN.
pub fn js_number(raw: &str) -> Option<f64> {
    let s = raw.trim_matches(|c: char| c.is_whitespace() || c == '\u{FEFF}');
    if s.is_empty() {
        return Some(0.0);
    }
    match s {
        "Infinity" | "+Infinity" => return Some(f64::INFINITY),
        "-Infinity" => return Some(f64::NEG_INFINITY),
        _ => {}
    }
    for (prefix, radix) in [("0x", 16), ("0X", 16), ("0o", 8), ("0O", 8), ("0b", 2), ("0B", 2)] {
        if let Some(digits) = s.strip_prefix(prefix) {
            return u64::from_str_radix(digits, radix).ok().map(|n| n as f64);
        }
    }
    let unsigned = s.strip_prefix(['+', '-']).unwrap_or(s);
    let (mantissa, exponent) = match unsigned.find(['e', 'E']) {
        Some(i) => (&unsigned[..i], Some(&unsigned[i + 1..])),
        None => (unsigned, None),
    };
    let mantissa_ok = {
        let (int, frac) = mantissa.split_once('.').unwrap_or((mantissa, ""));
        let digits = |x: &str| x.bytes().all(|b| b.is_ascii_digit());
        digits(int) && digits(frac) && !(int.is_empty() && frac.is_empty())
    };
    let exponent_ok = exponent.is_none_or(|e| {
        let e = e.strip_prefix(['+', '-']).unwrap_or(e);
        !e.is_empty() && e.bytes().all(|b| b.is_ascii_digit())
    });
    if !(mantissa_ok && exponent_ok) {
        return None;
    }
    s.parse::<f64>().ok()
}

/// JavaScript `String(n)` for a finite number: shortest round-trip digits, exponent form below
/// 1e-6 and from 1e21 up.
pub fn js_string(x: f64) -> String {
    if x == 0.0 {
        return "0".into();
    }
    let a = x.abs();
    if (1e-6..1e21).contains(&a) {
        return format!("{x}");
    }
    let s = format!("{x:e}");
    match s.split_once('e') {
        Some((m, e)) if !e.starts_with('-') => format!("{m}e+{e}"),
        _ => s,
    }
}

/// A finite JavaScript number as JSON, integers without a fraction (as `JSON.stringify`).
pub fn js_value(x: f64) -> serde_json::Value {
    if x.fract() == 0.0 && x.abs() < 9_007_199_254_740_992.0 {
        serde_json::Value::from(x as i64)
    } else {
        serde_json::Number::from_f64(x).map(serde_json::Value::Number).unwrap_or(serde_json::Value::Null)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prints_like_javascript() {
        assert_eq!(js_string(0.08), "0.08");
        assert_eq!(js_string(25.0), "25");
        assert_eq!(js_string(1e-7), "1e-7");
        assert_eq!(js_string(1.5e21), "1.5e+21");
        assert_eq!(js_string(-0.0), "0");
        assert_eq!(js_number(" 0x10 "), Some(16.0));
        assert_eq!(js_number(""), Some(0.0));
        assert_eq!(js_number("1e"), None);
    }
}
