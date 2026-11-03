//! D45 in-app updates: `GET /api/system/update` (is there a newer version?) and
//! `POST /api/system/update/install` (download, verify, install and restart). Like backup and
//! restore, only an Administrator at the main PC may use them; LAN clients follow the main PC,
//! since they load the app from it.

use crate::extract::CurrentUser;
use crate::state::AppState;
use app_core::{AppError, AppResult};
use axum::extract::{FromRequestParts, State};
use axum::http::request::Parts;
use axum::http::StatusCode;
use axum::Json;
use serde_json::{json, Value};

/// This build's version (the workspace version, the same as the installer's).
pub const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

/// An Administrator connected from this PC.
pub struct UpdateAdmin;

impl FromRequestParts<AppState> for UpdateAdmin {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        let current = CurrentUser::from_request_parts(parts, state).await?;
        let local = crate::extract::from_this_pc(parts);
        if !current.user.is_admin() || !local {
            return Err(AppError::Forbidden("Updates are installed by an Administrator on the main PC".into()));
        }
        Ok(UpdateAdmin)
    }
}

fn info_json(u: &crate::updates::UpdateInfo) -> Value {
    json!({ "version": u.version, "notes": u.notes, "date": u.date })
}

/// `{current, channel, supported, available, error}`: `channel` is "direct" or "store" (D47);
/// `available` is the newer version or null; `error` is why the feed couldn't be checked
/// (usually no internet), which the screen may ignore.
pub async fn check(State(s): State<AppState>, _a: UpdateAdmin) -> AppResult<Json<Value>> {
    let channel = s.update_channel.as_str();
    let Some(updater) = s.updater.clone() else {
        return Ok(Json(json!({
            "current": CURRENT_VERSION, "channel": channel, "supported": false, "available": null, "error": null
        })));
    };
    let (available, error) = match updater.check().await {
        Ok(found) => (found.as_ref().map(info_json).unwrap_or(Value::Null), Value::Null),
        Err(e) => (Value::Null, Value::String(e)),
    };
    Ok(Json(json!({
        "current": CURRENT_VERSION, "channel": channel, "supported": true, "available": available, "error": error
    })))
}

/// 202 once the update is downloaded and its signature checked; the app then closes, updates
/// itself and opens again.
pub async fn install(State(s): State<AppState>, _a: UpdateAdmin) -> AppResult<(StatusCode, Json<Value>)> {
    if s.update_channel == crate::updates::UpdateChannel::Store {
        return Err(AppError::bad_request("This copy of TallyCheck is updated by the Microsoft Store"));
    }
    let updater = s
        .updater
        .clone()
        .ok_or_else(|| AppError::bad_request("Updates are installed from the TallyCheck desktop app"))?;
    let info = updater.install().await.map_err(AppError::bad_request)?;
    tracing::info!(from = CURRENT_VERSION, to = %info.version, "update downloaded and verified; installing");
    Ok((StatusCode::ACCEPTED, Json(json!({ "current": CURRENT_VERSION, "installing": info_json(&info) }))))
}
