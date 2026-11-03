//! Reference `settings.controller.ts`: reading is open to any signed-in user (invoice PDFs
//! need the signatory); changing needs the `settings` permission. `payroll.*` and
//! `integration.*` keys are left out for callers without those permissions.

use crate::body::JsonBody;
use crate::extract::{perm, Authorized, CurrentUser};
use crate::state::AppState;
use crate::validate;
use app_core::AppResult;
use axum::extract::State;
use axum::Json;
use database::settings::{self, SettingsMap};

pub async fn find_all(State(state): State<AppState>, user: CurrentUser) -> AppResult<Json<SettingsMap>> {
    Ok(Json(visible_to(&user, settings::find_all(&state.db).await?)))
}

/// Everyone reads the company, region, tax and signatory settings. Payroll settings (rates,
/// statutory numbers) need the `payroll` permission and the connector settings the `settings`
/// permission; a Claude key sees neither.
fn visible_to(user: &CurrentUser, mut map: SettingsMap) -> SettingsMap {
    let by_key = user.via_key.is_some();
    let payroll = !by_key && user.user.has_permission("payroll");
    let integrations = !by_key && user.user.has_permission("settings");
    map.retain(|k, _| (payroll || !k.starts_with("payroll.")) && (integrations || !k.starts_with("integration.")));
    map
}

/// Values are trimmed before saving, as `String(value).trim()` in the reference. A `null`
/// value is ignored (the reference would have stored the text "null").
pub async fn update(
    State(state): State<AppState>,
    auth: Authorized<perm::Settings>,
    body: JsonBody,
) -> AppResult<Json<SettingsMap>> {
    let entries = validate::update_settings(body.object())?;
    let mut tx = state.db.begin().await?;
    for (key, value) in &entries {
        settings::upsert(&mut tx, key, value.trim()).await?;
    }
    tx.commit().await?;
    // D29: a currency without decimals rounds every amount to whole units from now on.
    if let Some((_, code)) = entries.iter().find(|(k, _)| k == "region.currency") {
        app_core::money::set_currency(code);
    }
    Ok(Json(visible_to(&auth.current, settings::find_all(&state.db).await?)))
}
