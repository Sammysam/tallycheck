//! D19: the books lock date. Nothing can be posted to the ledger on or before it (the check
//! lives in `ledger::entries`). Anyone signed in may read it; only an Administrator may move it.

use crate::body::JsonBody;
use crate::extract::CurrentUser;
use crate::state::AppState;
use app_core::time::{is_date_format, today_local};
use app_core::{AppError, AppResult};
use axum::extract::State;
use axum::Json;
use ledger::entries;
use serde_json::{json, Value};

pub async fn lock_get(State(s): State<AppState>, _user: CurrentUser) -> AppResult<Json<Value>> {
    let mut conn = s.db.read().acquire().await?;
    Ok(Json(json!({ "lockedThrough": entries::locked_through(&mut conn).await? })))
}

/// Body `{"lockedThrough": "YYYY-MM-DD"}` locks everything up to and including that date;
/// `{"lockedThrough": null}` unlocks. The date may not be after today.
pub async fn lock_set(State(s): State<AppState>, user: CurrentUser, body: JsonBody) -> AppResult<Json<Value>> {
    if !user.user.is_admin() {
        return Err(AppError::Forbidden("Only an administrator can change the lock date".into()));
    }
    let body = body.object();
    let date = match body.get("lockedThrough") {
        Some(Value::Null) => None,
        Some(Value::String(d)) if is_date_format(d) => Some(d.clone()),
        _ => {
            return Err(AppError::bad_request(
                "lockedThrough must be a valid date in YYYY-MM-DD format, or null to unlock",
            ))
        }
    };
    let today = today_local().to_string();
    if date.as_deref().is_some_and(|d| d > today.as_str()) {
        return Err(AppError::bad_request("The lock date cannot be after today"));
    }
    let mut tx = s.db.begin().await?;
    entries::set_locked_through(&mut tx, date.as_deref()).await?;
    tx.commit().await?;
    // `name` is what the activity log shows next to "Changed the books lock date".
    let shown = date.clone().map_or_else(|| "Unlocked".to_string(), |d| format!("Locked through {d}"));
    Ok(Json(json!({ "lockedThrough": date, "name": shown })))
}
