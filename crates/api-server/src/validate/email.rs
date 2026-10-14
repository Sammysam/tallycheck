//! validator.js `isEmail` with the default options class-validator's `@IsEmail()` uses
//! (validator 13.15): UTF-8 local part allowed, TLD required, no IP domains, no display name.
//! Verified against `tests/fixtures/reference_w2.json` (run through the reference validator).

/// `emailUserUtf8Part`: one dot-separated segment of an unquoted local part.
fn local_char(c: char) -> bool {
    c.is_ascii_alphanumeric()
        || "!#$%&'*+-/=?^_`{|}~".contains(c)
        || ('\u{00A0}'..='\u{D7FF}').contains(&c)
        || ('\u{F900}'..='\u{FDCF}').contains(&c)
        || ('\u{FDF0}'..='\u{FFEF}').contains(&c)
}

/// `quotedEmailUserUtf8`: the inside of a quoted local part.
fn quoted_ok(s: &str) -> bool {
    let utf8 = |c: char| {
        ('\u{00A0}'..='\u{D7FF}').contains(&c)
            || ('\u{F900}'..='\u{FDCF}').contains(&c)
            || ('\u{FDF0}'..='\u{FFEF}').contains(&c)
    };
    let plain = |c: char| {
        c.is_whitespace()
            || matches!(c as u32, 0x01..=0x08 | 0x0b | 0x0c | 0x0e..=0x1f | 0x7f | 0x21 | 0x23..=0x5b | 0x5d..=0x7e)
            || utf8(c)
    };
    let escaped = |c: char| matches!(c as u32, 0x01..=0x09 | 0x0b | 0x0c | 0x0d..=0x7f) || utf8(c);
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some(n) if escaped(n) => {}
                _ => return false,
            }
        } else if !plain(c) {
            return false;
        }
    }
    true
}

/// validator.js `isFQDN` with `require_tld: true`, no underscores, no trailing dot, no
/// numeric TLD.
fn is_fqdn(domain: &str) -> bool {
    let parts: Vec<&str> = domain.split('.').collect();
    let Some(tld) = parts.last() else { return false };
    if parts.len() < 2 {
        return false;
    }
    let tld_alpha = tld.chars().count() >= 2
        && tld.chars().all(|c| {
            c.is_ascii_alphabetic()
                || ('\u{00A1}'..='\u{00A8}').contains(&c)
                || ('\u{00AA}'..='\u{D7FF}').contains(&c)
                || ('\u{F900}'..='\u{FDCF}').contains(&c)
                || ('\u{FDF0}'..='\u{FFEF}').contains(&c)
        });
    let tld_punycode = tld.len() >= 4
        && tld[..2].eq_ignore_ascii_case("xn")
        && tld[2..].chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
    if !(tld_alpha || tld_punycode) || tld.chars().any(char::is_whitespace) {
        return false;
    }
    if tld.chars().all(|c| c.is_ascii_digit()) {
        return false;
    }
    parts.iter().all(|part| {
        part.chars().count() <= 63
            && !part.is_empty()
            && part.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c >= '\u{00A1}')
            && !part.chars().any(|c| ('\u{FF01}'..='\u{FF5E}').contains(&c))
            && !part.starts_with('-')
            && !part.ends_with('-')
            && !part.contains('_')
    })
}

pub fn is_email(s: &str) -> bool {
    if s.encode_utf16().count() > 254 {
        return false;
    }
    let Some(at) = s.rfind('@') else { return false };
    let (user, domain) = (&s[..at], &s[at + 1..]);
    if user.len() > 64 || domain.len() > 254 {
        return false;
    }
    if !is_fqdn(domain) {
        return false;
    }
    if let Some(inner) = user.strip_prefix('"') {
        let inner = inner.strip_suffix('"').unwrap_or_else(|| inner.get(..inner.len().saturating_sub(1)).unwrap_or(""));
        return quoted_ok(inner);
    }
    user.split('.').all(|part| !part.is_empty() && part.chars().all(local_char))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    #[test]
    fn matches_reference_validator() -> Result<(), String> {
        let fx: Value =
            serde_json::from_str(include_str!("../../tests/fixtures/reference_w2.json")).map_err(|e| e.to_string())?;
        let cases = fx["email_cases"].as_array().ok_or("email_cases")?;
        assert!(cases.len() > 50);
        for c in cases {
            let (Some(e), Some(ok)) = (c[0].as_str(), c[1].as_bool()) else { return Err(format!("bad case {c}")) };
            assert_eq!(is_email(e), ok, "{e:?}");
        }
        Ok(())
    }
}
