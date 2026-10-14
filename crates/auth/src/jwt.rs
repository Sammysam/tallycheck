//! JWTs with the reference's claims and lifetimes.
//!
//! `@nestjs/jwt` signs HS256 tokens carrying `{sub, username, stage, iat, exp}`. Access tokens
//! (`stage = "authenticated"`) live 8 hours; the token handed out after the password step
//! (`stage = "pre_2fa"`) lives 10 minutes. passport-jwt applies no clock leeway.

use app_core::{AppError, AppResult};
use jsonwebtoken::{decode, encode, Algorithm, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

pub const ACCESS_TTL_SECS: u64 = 8 * 3600;
pub const PRE_2FA_TTL_SECS: u64 = 10 * 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    #[serde(rename = "pre_2fa")]
    Pre2fa,
    Authenticated,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String,
    pub username: String,
    pub stage: Stage,
    pub iat: u64,
    pub exp: u64,
}

#[derive(Clone)]
pub struct JwtKeys {
    encoding: EncodingKey,
    decoding: DecodingKey,
}

impl std::fmt::Debug for JwtKeys {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("JwtKeys(<redacted>)")
    }
}

fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

impl JwtKeys {
    pub fn new(secret: &[u8]) -> Self {
        JwtKeys { encoding: EncodingKey::from_secret(secret), decoding: DecodingKey::from_secret(secret) }
    }

    pub fn sign(&self, user_id: &str, username: &str, stage: Stage) -> AppResult<String> {
        let ttl = match stage {
            Stage::Pre2fa => PRE_2FA_TTL_SECS,
            Stage::Authenticated => ACCESS_TTL_SECS,
        };
        self.sign_with(user_id, username, stage, now_secs(), ttl)
    }

    pub fn sign_with(&self, user_id: &str, username: &str, stage: Stage, iat: u64, ttl: u64) -> AppResult<String> {
        let claims = Claims { sub: user_id.to_string(), username: username.to_string(), stage, iat, exp: iat + ttl };
        encode(&Header::new(Algorithm::HS256), &claims, &self.encoding)
            .map_err(|e| AppError::internal(format!("jwt sign: {e}")))
    }

    /// Verifies signature, algorithm and expiry. Any failure is `None`; callers decide which
    /// 401 message the reference would give in their context.
    pub fn verify(&self, token: &str) -> Option<Claims> {
        let mut v = Validation::new(Algorithm::HS256);
        v.leeway = 0;
        v.validate_exp = true;
        v.set_required_spec_claims(&["exp", "sub"]);
        decode::<Claims>(token, &self.decoding, &v).ok().map(|d| d.claims)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_claims() -> AppResult<()> {
        let k = JwtKeys::new(b"0123456789abcdef0123456789abcdef");
        let t = k.sign("u1", "admin", Stage::Authenticated)?;
        let c = k.verify(&t).ok_or_else(|| AppError::internal("verify"))?;
        assert_eq!(c.sub, "u1");
        assert_eq!(c.username, "admin");
        assert_eq!(c.stage, Stage::Authenticated);
        assert_eq!(c.exp - c.iat, ACCESS_TTL_SECS);
        // The stage string must match what the frontend checks: payload.stage === 'authenticated'.
        let payload = t.split('.').nth(1).unwrap_or_default();
        let json = base64::Engine::decode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, payload)
            .map_err(AppError::internal)?;
        let v: serde_json::Value = serde_json::from_slice(&json).map_err(AppError::internal)?;
        assert_eq!(v["stage"], "authenticated");
        let pre = k.sign("u1", "admin", Stage::Pre2fa)?;
        let c = k.verify(&pre).ok_or_else(|| AppError::internal("verify"))?;
        assert_eq!(c.exp - c.iat, PRE_2FA_TTL_SECS);
        Ok(())
    }

    #[test]
    fn rejects_expired_tampered_and_foreign_tokens() -> AppResult<()> {
        let k = JwtKeys::new(b"0123456789abcdef0123456789abcdef");
        let expired = k.sign_with("u1", "a", Stage::Authenticated, now_secs() - 100, 10)?;
        assert!(k.verify(&expired).is_none());
        let other = JwtKeys::new(b"another-secret-another-secret-xx").sign("u1", "a", Stage::Authenticated)?;
        assert!(k.verify(&other).is_none());
        let good = k.sign("u1", "a", Stage::Authenticated)?;
        let tampered = format!("{}x", good);
        assert!(k.verify(&tampered).is_none());
        assert!(k.verify("garbage").is_none());
        Ok(())
    }

    #[test]
    fn rejects_alg_none() {
        let k = JwtKeys::new(b"0123456789abcdef0123456789abcdef");
        // {"alg":"none"} . {"sub":"u1","username":"a","stage":"authenticated","iat":1,"exp":9999999999}
        let t = "eyJhbGciOiJub25lIn0.eyJzdWIiOiJ1MSIsInVzZXJuYW1lIjoiYSIsInN0YWdlIjoiYXV0aGVudGljYXRlZCIsImlhdCI6MSwiZXhwIjo5OTk5OTk5OTk5fQ.";
        assert!(k.verify(t).is_none());
    }
}
