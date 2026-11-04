//! Reference `roles.controller.ts` — all routes need the `users` permission.

use super::users::{within_rights, BEYOND_OWN_RIGHTS};
use crate::body::JsonBody;
use crate::extract::{perm, Authorized};
use crate::state::AppState;
use crate::validate;
use app_core::permissions::PERMISSIONS;
use app_core::{AppError, AppResult};
use auth::roles::{self, Role};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde_json::{json, Value};

pub async fn permission_catalog(_auth: Authorized<perm::Users>) -> Json<Value> {
    Json(Value::Array(PERMISSIONS.iter().map(|p| json!({ "key": p.key, "label": p.label })).collect()))
}

pub async fn list(State(state): State<AppState>, _auth: Authorized<perm::Users>) -> AppResult<Json<Vec<Role>>> {
    Ok(Json(roles::find_all(&state.db).await?))
}

/// D52: someone who is not an Admin may only touch a role that is not the Admin role and whose
/// rights (now, and after the change) they hold themselves.
async fn may_manage_role(state: &AppState, caller: &auth::User, id: &str) -> AppResult<()> {
    if caller.is_admin() {
        return Ok(());
    }
    let mut conn = state.db.read().acquire().await?;
    let Some(role) = roles::get(&mut conn, id).await? else { return Ok(()) };
    if role.is_admin() || !within_rights(caller, &role.permissions) {
        return Err(AppError::Forbidden(BEYOND_OWN_RIGHTS.into()));
    }
    Ok(())
}

pub async fn create(
    State(state): State<AppState>,
    auth: Authorized<perm::Users>,
    body: JsonBody,
) -> AppResult<(StatusCode, Json<Role>)> {
    let (name, permissions) = validate::create_role(body.object())?;
    if !within_rights(auth.user(), &permissions) {
        return Err(AppError::Forbidden(BEYOND_OWN_RIGHTS.into()));
    }
    Ok((StatusCode::CREATED, Json(roles::create(&state.db, &name, &permissions).await?)))
}

pub async fn update(
    State(state): State<AppState>,
    auth: Authorized<perm::Users>,
    Path(id): Path<String>,
    body: JsonBody,
) -> AppResult<Json<Role>> {
    let dto = validate::update_role(body.object())?;
    may_manage_role(&state, auth.user(), &id).await?;
    if dto.permissions.as_deref().is_some_and(|p| !within_rights(auth.user(), p)) {
        return Err(AppError::Forbidden(BEYOND_OWN_RIGHTS.into()));
    }
    Ok(Json(roles::update(&state.db, &id, dto.name.as_deref(), dto.permissions.as_deref()).await?))
}

pub async fn remove(
    State(state): State<AppState>,
    auth: Authorized<perm::Users>,
    Path(id): Path<String>,
) -> AppResult<StatusCode> {
    may_manage_role(&state, auth.user(), &id).await?;
    roles::remove(&state.db, &id).await?;
    Ok(StatusCode::NO_CONTENT)
}
