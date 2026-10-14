//! Reference `settings.controller.ts`: reading is open to any signed-in user (invoice PDFs
//! need the signatory); changing needs the `settings` permission.

use crate::body::JsonBody;
use crate::extract::{perm, Authorized, CurrentUser};
use crate::state::AppState;
use crate::validate;
use app_core::AppResult;
use axum::extract::State;
use axum::Json;
use database::settings::{self, SettingsMap};

pub async fn find_all(State(state): State<AppState>, _user: CurrentUser) -> AppResult<Json<SettingsMap>> {
    Ok(Json(settings::find_all(&state.db).await?))
}

/// Values are trimmed before saving, as `String(value).trim()` in the reference. A `null`
/// value is ignored (the reference would have stored the text "null").
pub async fn update(
    State(state): State<AppState>,
    _auth: Authorized<perm::Settings>,
    body: JsonBody,
) -> AppResult<Json<SettingsMap>> {
    let entries = validate::update_settings(body.object())?;
    let mut tx = state.db.begin().await?;
    for (key, value) in entries {
        settings::upsert(&mut tx, &key, value.trim()).await?;
    }
    tx.commit().await?;
    Ok(Json(settings::find_all(&state.db).await?))
}
