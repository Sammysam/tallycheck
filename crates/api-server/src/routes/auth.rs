//! Reference `auth.controller.ts` / `auth.service.ts`.
//!
//! Flow (unchanged): password → `tempToken` (stage `pre_2fa`, 10 min) → first login sets up
//! TOTP via `/2fa/setup` + `/2fa/enable`; later logins use `/2fa/verify` → `accessToken`.
//! Deviations (D2): throttling, failed-login audit, inactive users refused, and 2FA setup
//! refused once 2FA is enabled.

use crate::body::JsonBody;
use crate::extract::CurrentUser;
use crate::state::AppState;
use app_core::{AppError, AppResult};
use auth::password::Verified;
use auth::{totp, users, Stage, User};
use axum::extract::{ConnectInfo, FromRequestParts, State};
use axum::http::header::AUTHORIZATION;
use axum::http::request::Parts;
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use serde_json::{json, Value};
use std::net::SocketAddr;

const LOGIN_PATH: &str = "/api/auth/login";

fn throttled(wait: std::time::Duration) -> AppError {
    let minutes = wait.as_secs().div_ceil(60).max(1);
    AppError::TooManyRequests(format!(
        "Too many failed attempts. Try again in {minutes} minute{}.",
        if minutes == 1 { "" } else { "s" }
    ))
}

async fn audit_login(state: &AppState, user_id: Option<&str>, username: &str, action: &str, status: u16) {
    audit::record(
        &state.db,
        audit::NewEntry {
            user_id: user_id.map(str::to_string),
            username: Some(username.to_string()),
            action: action.to_string(),
            method: Some("POST".into()),
            path: Some(LOGIN_PATH.into()),
            status_code: Some(status),
            detail: None,
        },
    )
    .await;
}

async fn reject_login(state: &AppState, user_id: Option<&str>, username: &str) -> AppError {
    state.throttle.record_failure(username);
    audit_login(state, user_id, username, "Failed login", 401).await;
    AppError::Unauthorized("Invalid credentials".into())
}

/// `POST /api/auth/login` (public, 200).
pub async fn login(State(state): State<AppState>, JsonBody(body): JsonBody) -> AppResult<Json<Value>> {
    let username = body.get("username").and_then(Value::as_str).unwrap_or_default().to_string();
    let password = body.get("password").and_then(Value::as_str).unwrap_or_default().to_string();
    if username.is_empty() || password.is_empty() {
        return Err(AppError::Unauthorized("Invalid credentials".into()));
    }
    state.throttle.check(&username).map_err(throttled)?;

    let Some(user) = users::get_by_username(&state.db, &username).await? else {
        users::burn_equivalent_time(&password).await?;
        return Err(reject_login(&state, None, &username).await);
    };
    let needs_rehash = match users::verify_password(&password, &user.password_hash).await? {
        Verified::Match { needs_rehash } => needs_rehash,
        Verified::Mismatch => return Err(reject_login(&state, Some(&user.id), &username).await),
    };
    if !user.is_active {
        audit_login(&state, Some(&user.id), &user.username, "Failed login (account inactive)", 403).await;
        return Err(AppError::Forbidden("Account is inactive".into()));
    }
    state.throttle.reset(&username);
    if needs_rehash {
        if let Err(e) = users::upgrade_password_hash(&state.db, &user.id, &password).await {
            tracing::error!(user = %user.id, "could not upgrade legacy password hash: {e}");
        }
    }
    audit_login(&state, Some(&user.id), &user.username, "Logged in", 200).await;

    let temp_token = state.jwt.sign(&user.id, &user.username, Stage::Pre2fa)?;
    if user.is_two_factor_enabled {
        return Ok(Json(json!({ "tempToken": temp_token, "requiresTwoFactor": true, "isNewUser": false })));
    }
    // D16: two-factor sign-in is optional. Without it the password step signs the user in;
    // the user can turn it on later (setup/enable accept the signed-in token).
    let access = state.jwt.sign(&user.id, &user.username, Stage::Authenticated)?;
    Ok(Json(json!({
        "tempToken": temp_token,
        "requiresTwoFactor": false,
        "isNewUser": false,
        "accessToken": access,
    })))
}

/// Reference `extractUserId`: accepts a `pre_2fa` or `authenticated` token from the header.
async fn user_from_header(state: &AppState, headers: &HeaderMap) -> AppResult<User> {
    let raw = headers.get(AUTHORIZATION).and_then(|v| v.to_str().ok()).unwrap_or_default();
    let token = raw.replacen("Bearer ", "", 1).trim().to_string();
    if token.is_empty() {
        return Err(AppError::Unauthorized("Missing token".into()));
    }
    let claims = state.jwt.verify(&token).ok_or_else(|| AppError::Unauthorized("Invalid token".into()))?;
    let user = users::get_by_id(&state.db, &claims.sub).await?.ok_or_else(AppError::unauthorized_default)?;
    if !user.is_active {
        return Err(AppError::Forbidden("Account is inactive".into()));
    }
    Ok(user)
}

/// `GET /api/auth/2fa/setup` (public route; token in header).
pub async fn setup_2fa(State(state): State<AppState>, headers: HeaderMap) -> AppResult<Json<Value>> {
    let user = user_from_header(&state, &headers).await?;
    if user.is_two_factor_enabled {
        return Err(AppError::Forbidden(
            "Two-factor authentication is already set up. Ask an administrator to reset it.".into(),
        ));
    }
    let secret = totp::generate_secret();
    users::set_two_factor_secret(&state.db, &user.id, &secret).await?;
    let qr_code = totp::qr_data_url(&totp::otpauth_url(&user.username, &secret))?;
    Ok(Json(json!({ "qrCode": qr_code, "secret": secret })))
}

fn code_of(body: &Value) -> String {
    match body.get("code") {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        _ => String::new(),
    }
}

fn check_code(state: &AppState, user: &User, secret: &str, code: &str) -> AppResult<()> {
    let key = format!("2fa:{}", user.id);
    state.throttle.check(&key).map_err(throttled)?;
    if totp::verify(secret, code) {
        state.throttle.reset(&key);
        Ok(())
    } else {
        state.throttle.record_failure(&key);
        Err(AppError::Unauthorized("Invalid 2FA code".into()))
    }
}

/// `POST /api/auth/2fa/enable` (public route; 200).
pub async fn enable_2fa(
    State(state): State<AppState>,
    headers: HeaderMap,
    JsonBody(body): JsonBody,
) -> AppResult<Json<Value>> {
    let user = user_from_header(&state, &headers).await?;
    let secret = user
        .two_factor_secret
        .clone()
        .ok_or_else(|| AppError::bad_request("Set up two-factor authentication first"))?;
    check_code(&state, &user, &secret, &code_of(&body))?;
    users::enable_two_factor(&state.db, &user.id).await?;
    let access = state.jwt.sign(&user.id, &user.username, Stage::Authenticated)?;
    Ok(Json(json!({ "accessToken": access })))
}

/// `POST /api/auth/2fa/verify` (public route; 200).
pub async fn verify_2fa(
    State(state): State<AppState>,
    headers: HeaderMap,
    JsonBody(body): JsonBody,
) -> AppResult<Json<Value>> {
    let user = user_from_header(&state, &headers).await?;
    let secret = match (&user.two_factor_secret, user.is_two_factor_enabled) {
        (Some(s), true) => s.clone(),
        _ => return Err(AppError::Unauthorized("2FA not configured".into())),
    };
    check_code(&state, &user, &secret, &code_of(&body))?;
    let access = state.jwt.sign(&user.id, &user.username, Stage::Authenticated)?;
    Ok(Json(json!({ "accessToken": access })))
}

/// Whether the request comes from this PC (what `into_make_service_with_connect_info` records).
pub struct FromThisPc(bool);

impl FromRequestParts<AppState> for FromThisPc {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, _state: &AppState) -> Result<Self, Self::Rejection> {
        Ok(FromThisPc(parts.extensions.get::<ConnectInfo<SocketAddr>>().is_some_and(|c| c.0.ip().is_loopback())))
    }
}

/// `GET /api/auth/first-run` (public, D17): whether the app still needs its first
/// administrator, and whether this browser may create it (only on the main PC).
pub async fn first_run_status(State(state): State<AppState>, FromThisPc(local): FromThisPc) -> AppResult<Json<Value>> {
    let needs_setup = !users::any_users(&state.db).await?;
    Ok(Json(json!({ "needsSetup": needs_setup, "canSetUpHere": needs_setup && local })))
}

/// `POST /api/auth/first-run` (public, D17): creates the first administrator. Refused once any
/// user exists, and refused from other PCs, so a LAN client can never claim a fresh install.
pub async fn first_run_create(
    State(state): State<AppState>,
    FromThisPc(local): FromThisPc,
    body: JsonBody,
) -> AppResult<(StatusCode, Json<Value>)> {
    if !local {
        return Err(AppError::Forbidden("The first administrator can only be created on the main PC".into()));
    }
    if users::any_users(&state.db).await? {
        return Err(AppError::Forbidden("The app is already set up. Sign in instead.".into()));
    }
    let dto = crate::validate::create_user(body.object())?;
    let user = users::create_first_admin(&state.db, &dto.username, &dto.password)
        .await?
        .ok_or_else(|| AppError::Forbidden("The app is already set up. Sign in instead.".into()))?;
    audit::record(
        &state.db,
        audit::NewEntry {
            user_id: Some(user.id.clone()),
            username: Some(user.username.clone()),
            action: "Created the first administrator".into(),
            method: Some("POST".into()),
            path: Some("/api/auth/first-run".into()),
            status_code: Some(201),
            detail: None,
        },
    )
    .await;
    let access = state.jwt.sign(&user.id, &user.username, Stage::Authenticated)?;
    Ok((StatusCode::CREATED, Json(json!({ "accessToken": access }))))
}

/// `GET /api/auth/me`.
pub async fn me(current: CurrentUser) -> Json<Value> {
    let u = current.user;
    Json(json!({
        "id": u.id,
        "username": u.username,
        "isTwoFactorEnabled": u.is_two_factor_enabled,
        "isActive": u.is_active,
        "role": u.role.map(|r| json!({ "id": r.id, "name": r.name, "permissions": r.permissions })),
    }))
}
