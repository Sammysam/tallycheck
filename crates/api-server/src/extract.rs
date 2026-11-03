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
use auth::api_keys::{self, ApiKey};
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
    /// D41: set when the request came with a key (Claude / MCP) instead of a sign-in.
    pub via_key: Option<ApiKey>,
}

/// D41: what a key may never reach, even read-only: people's pay and records, users, settings,
/// integrations, backups, the activity log and other keys.
fn key_may_not_reach(path: &str) -> bool {
    const CLOSED: &[&str] = &[
        "/api/employees",
        "/api/payroll",
        "/api/users",
        "/api/roles",
        "/api/audit-logs",
        "/api/system",
        "/api/integrations",
        "/api/api-keys",
        "/api/imports",
    ];
    path != "/api/api-keys/me"
        && CLOSED
            .iter()
            .any(|c| path == c.trim_end_matches('/') || path.starts_with(&format!("{}/", c.trim_end_matches('/'))))
}

/// Whether a request really comes from this PC: the connection is from a loopback address
/// **and** the address-bar host is a loopback name (`127.0.0.1`, `localhost`, `[::1]`). The
/// second part stops a web page that rebinds its own domain name to 127.0.0.1 (DNS rebinding)
/// from reaching the main-PC-only actions: its requests carry the attacker's host name. A
/// request without a recorded peer address is not from this PC.
pub fn from_this_pc(parts: &Parts) -> bool {
    let peer_local = parts
        .extensions
        .get::<axum::extract::ConnectInfo<std::net::SocketAddr>>()
        .is_some_and(|c| c.0.ip().is_loopback());
    let host = parts.headers.get(axum::http::header::HOST).and_then(|h| h.to_str().ok()).unwrap_or("");
    peer_local && host_is_loopback(host)
}

/// `127.0.0.1[:port]`, `localhost[:port]` or `[::1][:port]`.
pub fn host_is_loopback(host: &str) -> bool {
    let h = host.trim().to_ascii_lowercase();
    let name = if let Some(rest) = h.strip_prefix('[') {
        rest.split(']').next().unwrap_or("").to_string()
    } else {
        h.rsplit_once(':').map_or(h.as_str(), |(n, _)| n).to_string()
    };
    name == "localhost" || name.parse::<std::net::IpAddr>().is_ok_and(|ip| ip.is_loopback())
}

/// D41: a request authenticated by a key: only from this PC; reads, or (if allowed) drafts.
async fn by_key(parts: &Parts, state: &AppState, token: &str) -> Result<CurrentUser, AppError> {
    let key = api_keys::verify(&state.db, token).await?.ok_or_else(AppError::unauthorized_default)?;
    if !from_this_pc(parts) {
        return Err(AppError::Forbidden("Keys work only on the PC that runs the app".into()));
    }
    // Inside the nested /api router the URI has lost its prefix; the original one has it.
    let full = parts.extensions.get::<axum::extract::OriginalUri>().map(|u| u.0.path().to_string());
    let path = full.as_deref().unwrap_or_else(|| parts.uri.path());
    let method = parts.method.as_str();
    if key_may_not_reach(path) {
        return Err(AppError::Forbidden("Claude cannot open this part of the app".into()));
    }
    if method != "GET" && !(key.can_write && api_keys::draft_only_allows(method, path)) {
        return Err(AppError::Forbidden(if key.can_write {
            "Claude may only create drafts; post, pay and change records in the app".into()
        } else {
            "This key may only read".into()
        }));
    }
    let user = users::get_by_id(&state.db, &key.user_id).await?.ok_or_else(AppError::unauthorized_default)?;
    if !user.is_active {
        return Err(AppError::Forbidden("Account is inactive".into()));
    }
    let claims =
        Claims { sub: user.id.clone(), username: user.username.clone(), stage: Stage::Authenticated, iat: 0, exp: 0 };
    Ok(CurrentUser { user, claims, via_key: Some(key) })
}

impl FromRequestParts<AppState> for CurrentUser {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        let token = bearer_token(parts).ok_or_else(AppError::unauthorized_default)?;
        if api_keys::looks_like_key(&token) {
            return by_key(parts, state, &token).await;
        }
        let claims = state.jwt.verify(&token).ok_or_else(AppError::unauthorized_default)?;
        if claims.stage != Stage::Authenticated {
            return Err(AppError::Unauthorized("Full authentication required".into()));
        }
        let user = users::get_by_id(&state.db, &claims.sub).await?.ok_or_else(AppError::unauthorized_default)?;
        if !user.is_active {
            return Err(AppError::Forbidden("Account is inactive".into()));
        }
        Ok(CurrentUser { user, claims, via_key: None })
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
    permission!(Employees, "employees");
    permission!(Payroll, "payroll");
}

/// Nest's `ParseUUIDPipe`: 400 `Validation failed (uuid is expected)`.
pub fn parse_uuid(value: &str) -> Result<(), AppError> {
    if app_core::ids::is_uuid(value) {
        Ok(())
    } else {
        Err(AppError::bad_request("Validation failed (uuid is expected)"))
    }
}
