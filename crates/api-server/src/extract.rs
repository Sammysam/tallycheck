//! Authentication and permission extractors — the reference's `AuthenticatedGuard` +
//! `JwtStrategy` + `PermissionsGuard`.
//!
//! * no / invalid / expired token → 401 `Unauthorized`
//! * a `pre_2fa` token on a protected route → 401 `Full authentication required`
//! * inactive account → 403 `Account is inactive` (on every protected route — deviation D2;
//!   the reference only checked it on permission-guarded routes)
//! * missing permission → 403 `Insufficient permissions` (the Admin role bypasses)

use crate::state::AppState;
use app_core::AppError;
use auth::{users, Claims, Stage, User};
use axum::extract::FromRequestParts;
use axum::http::header::AUTHORIZATION;
use axum::http::request::Parts;
use std::marker::PhantomData;

/// passport-jwt `fromAuthHeaderAsBearerToken`: `Authorization: Bearer <token>`, scheme
/// case-insensitive.
pub fn bearer_token(parts: &Parts) -> Option<String> {
    let value = parts.headers.get(AUTHORIZATION)?.to_str().ok()?;
    let mut it = value.split_whitespace();
    let scheme = it.next()?;
    let token = it.next()?;
    (scheme.eq_ignore_ascii_case("bearer") && it.next().is_none()).then(|| token.to_string())
}

pub struct CurrentUser {
    pub user: User,
    pub claims: Claims,
}

impl FromRequestParts<AppState> for CurrentUser {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        let token = bearer_token(parts).ok_or_else(AppError::unauthorized_default)?;
        let claims = state.jwt.verify(&token).ok_or_else(AppError::unauthorized_default)?;
        if claims.stage != Stage::Authenticated {
            return Err(AppError::Unauthorized("Full authentication required".into()));
        }
        let user = users::get_by_id(&state.db, &claims.sub).await?.ok_or_else(AppError::unauthorized_default)?;
        if !user.is_active {
            return Err(AppError::Forbidden("Account is inactive".into()));
        }
        Ok(CurrentUser { user, claims })
    }
}

pub trait PermissionKey {
    const KEY: &'static str;
}

/// A signed-in user who holds permission `P` (or is Admin).
pub struct Authorized<P> {
    pub current: CurrentUser,
    _p: PhantomData<P>,
}

impl<P> Authorized<P> {
    pub fn user(&self) -> &User {
        &self.current.user
    }
}

impl<P: PermissionKey + Send + Sync> FromRequestParts<AppState> for Authorized<P> {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        let current = CurrentUser::from_request_parts(parts, state).await?;
        if !current.user.has_permission(P::KEY) {
            return Err(AppError::Forbidden("Insufficient permissions".into()));
        }
        Ok(Authorized { current, _p: PhantomData })
    }
}

macro_rules! permission {
    ($name:ident, $key:literal) => {
        pub struct $name;
        impl PermissionKey for $name {
            const KEY: &'static str = $key;
        }
    };
}

pub mod perm {
    use super::PermissionKey;
    permission!(Users, "users");
    permission!(Audit, "audit");
    permission!(Settings, "settings");
    permission!(Accounts, "accounts");
    permission!(Ledger, "ledger");
    permission!(Customers, "customers");
    permission!(Vendors, "vendors");
    permission!(Banking, "banking");
    permission!(Invoices, "invoices");
    permission!(Bills, "bills");
    permission!(Expenses, "expenses");
    permission!(Payments, "payments");
    permission!(Reports, "reports");
}
