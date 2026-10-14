//! TOTP compatible with the reference's speakeasy usage: base32 secret, SHA-1, 6 digits,
//! 30-second step, verification window ±1 step. Secrets created by the reference keep working.

use app_core::{AppError, AppResult};
use base64::Engine;
use std::io::Cursor;
use totp_rs::{Algorithm, Secret, TOTP};

const DIGITS: usize = 6;
const STEP: u64 = 30;
const SKEW: u8 = 1;

/// A new random base32 secret (160 bits).
pub fn generate_secret() -> String {
    match Secret::generate_secret().to_encoded() {
        Secret::Encoded(s) => s,
        Secret::Raw(_) => String::new(),
    }
}

fn totp_for(secret_base32: &str) -> Option<TOTP> {
    let bytes = Secret::Encoded(secret_base32.trim().to_ascii_uppercase()).to_bytes().ok()?;
    if bytes.is_empty() {
        return None;
    }
    // `new_unchecked`: legacy speakeasy secrets are fine but may not meet totp-rs's
    // 128-bit minimum check, which is about generation, not verification.
    Some(TOTP::new_unchecked(Algorithm::SHA1, DIGITS, SKEW, STEP, bytes))
}

pub fn verify(secret_base32: &str, code: &str) -> bool {
    let code = code.trim();
    if code.len() != DIGITS || !code.bytes().all(|b| b.is_ascii_digit()) {
        return false;
    }
    match totp_for(secret_base32) {
        Some(t) => t.check_current(code).unwrap_or(false),
        None => false,
    }
}

/// The current code — for tests and the contract-test harness only.
pub fn current_code(secret_base32: &str) -> Option<String> {
    totp_for(secret_base32).and_then(|t| t.generate_current().ok())
}

/// `otpauth://` URL as speakeasy builds it for `generateSecret({ name })`.
pub fn otpauth_url(username: &str, secret_base32: &str) -> String {
    let label = format!("Accounting System ({username})");
    format!("otpauth://totp/{}?secret={}", encode_uri_component(&label), secret_base32)
}

/// QR code for the URL as a PNG data URL (what `QRCode.toDataURL` returns).
pub fn qr_data_url(url: &str) -> AppResult<String> {
    let code = qrcode::QrCode::new(url.as_bytes()).map_err(|e| AppError::internal(format!("qr: {e}")))?;
    let img = code.render::<image::Luma<u8>>().min_dimensions(200, 200).build();
    let mut png = Cursor::new(Vec::new());
    image::DynamicImage::ImageLuma8(img)
        .write_to(&mut png, image::ImageFormat::Png)
        .map_err(|e| AppError::internal(format!("qr png: {e}")))?;
    Ok(format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(png.into_inner())))
}

/// JavaScript's `encodeURIComponent`.
fn encode_uri_component(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&b) {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_secret_verifies_its_current_code() {
        let s = generate_secret();
        assert_eq!(s.len(), 32);
        let code = current_code(&s).unwrap_or_default();
        assert!(verify(&s, &code));
    }

    #[test]
    fn rfc6238_sha1_vector() {
        // RFC 6238 test secret "12345678901234567890" (base32 below) at T=59 s → 94287082 (8
        // digits); the 6-digit code is the last six digits.
        let secret = "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ";
        let t = totp_for(secret).map(|t| t.generate(59));
        assert_eq!(t.as_deref(), Some("287082"));
    }

    #[test]
    fn rejects_malformed_codes_and_secrets() {
        let s = generate_secret();
        assert!(!verify(&s, "12345"));
        assert!(!verify(&s, "abcdef"));
        assert!(!verify("not base32 !!", "123456"));
        assert!(!verify("", "123456"));
    }

    #[test]
    fn otpauth_url_matches_speakeasy_label_encoding() {
        assert_eq!(otpauth_url("admin", "ABC"), "otpauth://totp/Accounting%20System%20(admin)?secret=ABC");
    }

    #[test]
    fn qr_is_png_data_url() -> AppResult<()> {
        let d = qr_data_url("otpauth://totp/x?secret=ABC")?;
        assert!(d.starts_with("data:image/png;base64,iVBORw0KGgo"));
        Ok(())
    }
}
