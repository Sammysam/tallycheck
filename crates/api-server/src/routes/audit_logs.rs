//! Reference `audit.controller.ts` — `audit` permission.

use crate::extract::{perm, Authorized};
use crate::state::AppState;
use crate::validate;
use app_core::AppResult;
use axum::extract::{Query, State};
use axum::Json;
use serde_json::Value;

pub async fn users(
    State(state): State<AppState>,
    _auth: Authorized<perm::Audit>,
) -> AppResult<Json<Vec<audit::AuditUser>>> {
    Ok(Json(audit::users(&state.db).await?))
}

pub async fn list(
    State(state): State<AppState>,
    _auth: Authorized<perm::Audit>,
    Query(pairs): Query<Vec<(String, String)>>,
) -> AppResult<Json<audit::Paginated<audit::AuditLog>>> {
    let query = validate::query_map(pairs);
    let (page, limit) = validate::pagination(&query)?;
    // `@Query('userId') userId || undefined`: an empty value means no filter.
    let text = |key: &str| match query.get(key) {
        Some(Value::String(s)) if !s.is_empty() => Some(s.clone()),
        Some(Value::Array(items)) => items.first().and_then(Value::as_str).map(str::to_string),
        _ => None,
    };
    // D36: dates must be YYYY-MM-DD; anything else is ignored rather than mis-filtering.
    let date = |key: &str| text(key).filter(|d| d.len() == 10 && d.as_bytes()[4] == b'-' && d.as_bytes()[7] == b'-');
    let filter = audit::Filter {
        user_id: text("userId"),
        date_from: date("dateFrom"),
        date_to: date("dateTo"),
        search: text("search"),
        kind: text("kind"),
    };
    Ok(Json(audit::list(&state.db, page, limit, &filter).await?))
}
