//! Reference `auth.controller.ts` / `auth.service.ts`.
//!
//! Flow (unchanged): password → `tempToken` (stage `pre_2fa`, 10 min) → first login sets up
//! TOTP via `/2fa/setup` + `/2fa/enable`; later logins use `/2fa/verify` → `accessToken`.
//! Deviations (D2): throttling, failed-login audit, inactive users refused, and 2FA setup
//! refused once 2FA is enabled.

use crate::body::JsonBody;
use crate::extract::CurrentUser;
use crate::local_verify::Verification;
use crate::state::AppState;
use app_core::{AppError, AppResult};
use auth::password::Verified;
use auth::{recovery, totp, users, Stage, User};
use axum::extract::{FromRequestParts, State};
use axum::http::header::AUTHORIZATION;
use axum::http::request::Parts;
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use serde_json::{json, Value};

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

/// D52: a failed sign-in always takes at least this long, whatever the stored hash is (Argon2,
/// an imported bcrypt hash, or no user at all), so its timing does not tell which usernames
/// exist or which still have an imported password. Successful sign-ins are not slowed.
const FAILED_LOGIN_FLOOR: std::time::Duration = std::time::Duration::from_millis(800);

/// The attempt was already counted by `begin`; this only writes the log and the answer.
async fn reject_login(
    state: &AppState,
    started: tokio::time::Instant,
    user_id: Option<&str>,
    username: &str,
) -> AppError {
    audit_login(state, user_id, username, "Failed login", 401).await;
    tokio::time::sleep_until(started + FAILED_LOGIN_FLOOR).await;
    AppError::Unauthorized("Invalid credentials".into())
}

/// `POST /api/auth/login` (public, 200).
pub async fn login(
    State(state): State<AppState>,
    FromThisPc(local): FromThisPc,
    JsonBody(body): JsonBody,
) -> AppResult<Json<Value>> {
    let username = body.get("username").and_then(Value::as_str).unwrap_or_default().to_string();
    let password = body.get("password").and_then(Value::as_str).unwrap_or_default().to_string();
    if username.is_empty() || password.is_empty() || username.chars().count() > MAX_USERNAME_CHARS {
        return Err(AppError::Unauthorized("Invalid credentials".into()));
    }
    let throttle_key = throttle_key("login", local, &username);
    state.throttle.begin(&throttle_key).map_err(throttled)?;
    let started = tokio::time::Instant::now();

    let Some(user) = users::get_by_username(&state.db, &username).await? else {
        users::burn_equivalent_time(&password).await?;
        return Err(reject_login(&state, started, None, &username).await);
    };
    let needs_rehash = match users::verify_password(&password, &user.password_hash).await? {
        Verified::Match { needs_rehash } => needs_rehash,
        Verified::Mismatch => return Err(reject_login(&state, started, Some(&user.id), &username).await),
    };
    if !user.is_active {
        audit_login(&state, Some(&user.id), &user.username, "Failed login (account inactive)", 403).await;
        return Err(AppError::Forbidden("Account is inactive".into()));
    }
    state.throttle.reset(&throttle_key);
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
    if !auth::sessions::still_valid(&state.db, &user.id, claims.iat).await? {
        return Err(AppError::unauthorized_default());
    }
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

/// D52: the last 30-second step each user signed in with, so a code works once (a code seen
/// over someone's shoulder, or replayed, is refused). In memory: a restart clears it, and codes
/// are only valid for about a minute anyway.
static LAST_TOTP_STEP: std::sync::Mutex<Option<std::collections::HashMap<String, u64>>> = std::sync::Mutex::new(None);

/// Records `step` for the user unless it is not newer than the last one used.
fn first_use_of_step(user_id: &str, step: u64) -> bool {
    let mut guard = LAST_TOTP_STEP.lock().unwrap_or_else(|p| p.into_inner());
    let map = guard.get_or_insert_with(Default::default);
    if map.len() > auth::throttle::MAX_KEYS {
        map.clear();
    }
    match map.get(user_id) {
        Some(last) if step <= *last => false,
        _ => {
            map.insert(user_id.to_string(), step);
            true
        }
    }
}

fn check_code(state: &AppState, local: bool, user: &User, secret: &str, code: &str) -> AppResult<()> {
    // Keyed by the user's id, not the typed name, so the keyspace is one entry per real user.
    let key = throttle_key("2fa", local, &user.id);
    state.throttle.begin(&key).map_err(throttled)?;
    match totp::matching_step(secret, code) {
        Some(step) if first_use_of_step(&user.id, step) => {
            state.throttle.reset(&key);
            Ok(())
        }
        Some(_) => Err(AppError::Unauthorized(
            "This code was already used. Wait for the next code in your authenticator app.".into(),
        )),
        None => Err(AppError::Unauthorized("Invalid 2FA code".into())),
    }
}

/// `POST /api/auth/2fa/enable` (public route; 200).
pub async fn enable_2fa(
    State(state): State<AppState>,
    FromThisPc(local): FromThisPc,
    headers: HeaderMap,
    JsonBody(body): JsonBody,
) -> AppResult<Json<Value>> {
    let user = user_from_header(&state, &headers).await?;
    let secret = user
        .two_factor_secret
        .clone()
        .ok_or_else(|| AppError::bad_request("Set up two-factor authentication first"))?;
    check_code(&state, local, &user, &secret, &code_of(&body))?;
    users::enable_two_factor(&state.db, &user.id).await?;
    let access = state.jwt.sign(&user.id, &user.username, Stage::Authenticated)?;
    Ok(Json(json!({ "accessToken": access })))
}

/// `POST /api/auth/2fa/verify` (public route; 200).
pub async fn verify_2fa(
    State(state): State<AppState>,
    FromThisPc(local): FromThisPc,
    headers: HeaderMap,
    JsonBody(body): JsonBody,
) -> AppResult<Json<Value>> {
    let user = user_from_header(&state, &headers).await?;
    let secret = match (&user.two_factor_secret, user.is_two_factor_enabled) {
        (Some(s), true) => s.clone(),
        _ => return Err(AppError::Unauthorized("2FA not configured".into())),
    };
    check_code(&state, local, &user, &secret, &code_of(&body))?;
    let access = state.jwt.sign(&user.id, &user.username, Stage::Authenticated)?;
    Ok(Json(json!({ "accessToken": access })))
}

/// Whether the request comes from this PC (what `into_make_service_with_connect_info` records).
pub struct FromThisPc(bool);

impl FromRequestParts<AppState> for FromThisPc {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, _state: &AppState) -> Result<Self, Self::Rejection> {
        Ok(FromThisPc(crate::extract::from_this_pc(parts)))
    }
}

/// Usernames longer than this are never valid; such attempts are refused before they are
/// throttled or written to the activity log (which would let anyone fill memory and the log).
const MAX_USERNAME_CHARS: usize = 255;

/// Throttle key for a username. Attempts from other PCs are counted apart from attempts on the
/// main PC, so a LAN client that types wrong passwords on purpose cannot lock the owner out.
fn throttle_key(kind: &str, local: bool, username: &str) -> String {
    format!("{kind}|{}|{}", if local { "local" } else { "lan" }, username.trim())
}

/// Only one Windows confirmation at a time (a second request would stack prompts on the screen).
static WINDOWS_PROMPT_OPEN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

struct PromptGuard;

impl PromptGuard {
    fn take() -> Option<Self> {
        use std::sync::atomic::Ordering;
        WINDOWS_PROMPT_OPEN.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire).ok().map(|_| PromptGuard)
    }
}

impl Drop for PromptGuard {
    fn drop(&mut self) {
        WINDOWS_PROMPT_OPEN.store(false, std::sync::atomic::Ordering::Release);
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
    // D49: the first administrator gets a recovery key straight away; the page shows it once.
    let recovery_key = recovery::issue(&state.db, &user.id).await?;
    Ok((StatusCode::CREATED, Json(json!({ "accessToken": access, "recoveryKey": recovery_key }))))
}

async fn audit_auth(state: &AppState, user: Option<&User>, username: &str, action: &str, path: &str, status: u16) {
    audit::record(
        &state.db,
        audit::NewEntry {
            user_id: user.map(|u| u.id.clone()),
            username: Some(user.map_or(username, |u| u.username.as_str()).to_string()),
            action: action.to_string(),
            method: Some("POST".into()),
            path: Some(path.into()),
            status_code: Some(status),
            detail: None,
        },
    )
    .await;
}

/// After a successful reset, the user's sign-in lock (from this PC and from others) is lifted.
fn unlock_sign_in(state: &AppState, username: &str) {
    state.throttle.reset(&throttle_key("login", true, username));
    state.throttle.reset(&throttle_key("login", false, username));
}

fn text(body: &serde_json::Map<String, Value>, key: &str) -> String {
    body.get(key).and_then(Value::as_str).unwrap_or_default().trim().to_string()
}

/// `GET /api/auth/recovery/options` (public, D49): which ways of resetting a forgotten password
/// this browser can use. The recovery key always; Windows verification only in the desktop app
/// on the main PC.
pub async fn recovery_options(State(state): State<AppState>, FromThisPc(local): FromThisPc) -> Json<Value> {
    Json(json!({ "recoveryKey": true, "windows": local && state.windows_verifier.is_some() }))
}

const RECOVERY_KEY_PATH: &str = "/api/auth/recovery/key";
const RECOVERY_WINDOWS_PATH: &str = "/api/auth/recovery/windows";

/// `POST /api/auth/recovery/key` (public, 200, D49) `{username, recoveryKey, password}`: sets a
/// new password with the user's recovery key. The key is used up; the answer carries the new
/// one. A wrong username and a wrong key get the same answer, and failures are throttled per
/// username (apart from sign-in failures).
pub async fn recover_with_key(
    State(state): State<AppState>,
    FromThisPc(local): FromThisPc,
    body: JsonBody,
) -> AppResult<Json<Value>> {
    let obj = body.object();
    let (username, key) = (text(&obj, "username"), text(&obj, "recoveryKey"));
    if username.is_empty() || key.is_empty() {
        return Err(AppError::bad_request("Enter your username and your recovery key"));
    }
    if username.chars().count() > MAX_USERNAME_CHARS {
        return Err(AppError::bad_request("That username and recovery key don't match. Check both and try again."));
    }
    let password = crate::validate::reset_password(obj)?;
    let throttle_key = throttle_key("recovery", local, &username);
    state.recovery_throttle.begin(&throttle_key).map_err(throttled)?;
    match recovery::reset_with_key(&state.db, &username, &key, &password).await? {
        Some((user, next)) => {
            state.recovery_throttle.reset(&throttle_key);
            unlock_sign_in(&state, &username);
            audit_auth(
                &state,
                Some(&user),
                &username,
                "Reset the password with the recovery key",
                RECOVERY_KEY_PATH,
                200,
            )
            .await;
            Ok(Json(json!({ "recoveryKey": next })))
        }
        None => {
            audit_auth(&state, None, &username, "Failed password recovery", RECOVERY_KEY_PATH, 400).await;
            Err(AppError::bad_request("That username and recovery key don't match. Check both and try again."))
        }
    }
}

/// `POST /api/auth/recovery/windows` (public, 200, D49) `{username, password}`: only on the main
/// PC and only in the desktop app. Windows asks the person to confirm they are the Windows user
/// running TallyCheck (Windows Hello or the Windows password); then the password is set and a
/// new recovery key issued.
pub async fn recover_with_windows(
    State(state): State<AppState>,
    FromThisPc(local): FromThisPc,
    body: JsonBody,
) -> AppResult<Json<Value>> {
    if !local {
        return Err(AppError::Forbidden(
            "Resetting with Windows sign-in works only on the main PC where TallyCheck is installed".into(),
        ));
    }
    let verifier = state.windows_verifier.clone().ok_or_else(|| {
        AppError::bad_request("Resetting with Windows sign-in works only in the TallyCheck desktop app")
    })?;
    let obj = body.object();
    let username = text(&obj, "username");
    if username.is_empty() {
        return Err(AppError::bad_request("Enter the username whose password you want to reset"));
    }
    if username.chars().count() > MAX_USERNAME_CHARS {
        return Err(AppError::bad_request("There is no TallyCheck user with that username."));
    }
    let password = crate::validate::reset_password(obj)?;
    let throttle_key = throttle_key("recovery-windows", true, &username);
    state.recovery_throttle.begin(&throttle_key).map_err(throttled)?;
    let Some(_prompt) = PromptGuard::take() else {
        return Err(AppError::bad_request("A Windows confirmation is already open. Finish or cancel it first."));
    };
    // Windows confirms first; only then does the answer say whether the username exists, so the
    // usernames can't be listed by trying names (only the PC's owner gets that far).
    let message = format!("Confirm it's you to reset the TallyCheck password of {username}.");
    match verifier.verify(message).await {
        Verification::Verified => {
            let Some((user, next)) = recovery::reset_for_windows_owner(&state.db, &username, &password).await? else {
                audit_auth(&state, None, &username, "Failed password recovery (Windows)", RECOVERY_WINDOWS_PATH, 400)
                    .await;
                return Err(AppError::bad_request("There is no TallyCheck user with that username."));
            };
            state.recovery_throttle.reset(&throttle_key);
            unlock_sign_in(&state, &username);
            audit_auth(
                &state,
                Some(&user),
                &username,
                "Reset the password with Windows sign-in",
                RECOVERY_WINDOWS_PATH,
                200,
            )
            .await;
            Ok(Json(json!({ "recoveryKey": next })))
        }
        Verification::NotVerified => {
            audit_auth(&state, None, &username, "Failed password recovery (Windows)", RECOVERY_WINDOWS_PATH, 400).await;
            Err(AppError::bad_request("Windows did not confirm it's you, so the password was not changed."))
        }
        Verification::Unavailable(reason) => Err(AppError::bad_request(reason)),
    }
}

/// `GET /api/auth/recovery-key` (signed in, D49): whether the signed-in user has a recovery key.
pub async fn recovery_key_status(State(state): State<AppState>, current: CurrentUser) -> AppResult<Json<Value>> {
    let created = recovery::created_at(&state.db, &current.user.id).await?;
    Ok(Json(json!({ "hasKey": created.is_some(), "createdAt": created })))
}

/// `POST /api/auth/recovery-key` (signed in, 201, D49) `{password}`: makes a new recovery key for
/// the signed-in user, replacing the old one. Needs the user's own password, so an unattended
/// signed-in screen is not enough; never through a Claude key.
pub async fn create_recovery_key(
    State(state): State<AppState>,
    current: CurrentUser,
    body: JsonBody,
) -> AppResult<(StatusCode, Json<Value>)> {
    if current.via_key.is_some() {
        return Err(AppError::Forbidden("Recovery keys are made in the app, not through a key".into()));
    }
    let user = current.user;
    let password = text(&body.object(), "password");
    let throttle_key = format!("new-key|{}", user.id);
    state.recovery_throttle.begin(&throttle_key).map_err(throttled)?;
    if !matches!(users::verify_password(&password, &user.password_hash).await?, Verified::Match { .. }) {
        return Err(AppError::bad_request("That is not your current password"));
    }
    state.recovery_throttle.reset(&throttle_key);
    let key = recovery::issue(&state.db, &user.id).await?;
    audit_auth(&state, Some(&user), &user.username, "Created a recovery key", "/api/auth/recovery-key", 201).await;
    Ok((StatusCode::CREATED, Json(json!({ "recoveryKey": key }))))
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
