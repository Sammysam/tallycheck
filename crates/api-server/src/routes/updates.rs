//! D45 in-app updates: `GET /api/system/update` (is there a newer version?) and
//! `POST /api/system/update/install` (download, verify, install and restart). Like backup and
//! restore, only an Administrator at the main PC may use them; LAN clients follow the main PC,
//! since they load the app from it.

use crate::extract::CurrentUser;
use crate::state::AppState;
use app_core::{AppError, AppResult};
use axum::extract::{ConnectInfo, FromRequestParts, State};
use axum::http::request::Parts;
use axum::http::StatusCode;
use axum::Json;
use serde_json::{json, Value};
use std::net::SocketAddr;

/// This build's version (the workspace version, the same as the installer's).
pub const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

/// An Administrator connected from this PC.
pub struct UpdateAdmin;

impl FromRequestParts<AppState> for UpdateAdmin {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        let current = CurrentUser::from_request_parts(parts, state).await?;
        let local = parts.extensions.get::<ConnectInfo<SocketAddr>>().is_some_and(|c| c.0.ip().is_loopback());
        if !current.user.is_admin() || !local {
            return Err(AppError::Forbidden("Updates are installed by an Administrator on the main PC".into()));
        }
        Ok(UpdateAdmin)
    }
}

fn info_json(u: &crate::updates::UpdateInfo) -> Value {
    json!({ "version": u.version, "notes": u.notes, "date": u.date })
}

/// `{current, supported, available, error}`: `available` is the newer version or null; `error`
/// is why the feed couldn't be checked (usually no internet), which the screen may ignore.
pub async fn check(State(s): State<AppState>, _a: UpdateAdmin) -> AppResult<Json<Value>> {
    let Some(updater) = s.updater.clone() else {
        return Ok(Json(json!({ "current": CURRENT_VERSION, "supported": false, "available": null, "error": null })));
    };
    let (available, error) = match updater.check().await {
        Ok(found) => (found.as_ref().map(info_json).unwrap_or(Value::Null), Value::Null),
        Err(e) => (Value::Null, Value::String(e)),
    };
    Ok(Json(json!({ "current": CURRENT_VERSION, "supported": true, "available": available, "error": error })))
}

/// 202 once the update is downloaded and its signature checked; the app then closes, updates
/// itself and opens again.
pub async fn install(State(s): State<AppState>, _a: UpdateAdmin) -> AppResult<(StatusCode, Json<Value>)> {
    let updater = s
        .updater
        .clone()
        .ok_or_else(|| AppError::bad_request("Updates are installed from the TallyCheck desktop app"))?;
    let info = updater.install().await.map_err(AppError::bad_request)?;
    tracing::info!(from = CURRENT_VERSION, to = %info.version, "update downloaded and verified; installing");
    Ok((StatusCode::ACCEPTED, Json(json!({ "current": CURRENT_VERSION, "installing": info_json(&info) }))))
}
