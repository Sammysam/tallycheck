//! Reference `roles.controller.ts` — all routes need the `users` permission.

use crate::body::JsonBody;
use crate::extract::{perm, Authorized};
use crate::state::AppState;
use crate::validate;
use app_core::permissions::PERMISSIONS;
use app_core::AppResult;
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

pub async fn create(
    State(state): State<AppState>,
    _auth: Authorized<perm::Users>,
    body: JsonBody,
) -> AppResult<(StatusCode, Json<Role>)> {
    let (name, permissions) = validate::create_role(body.object())?;
    Ok((StatusCode::CREATED, Json(roles::create(&state.db, &name, &permissions).await?)))
}

pub async fn update(
    State(state): State<AppState>,
    _auth: Authorized<perm::Users>,
    Path(id): Path<String>,
    body: JsonBody,
) -> AppResult<Json<Role>> {
    let dto = validate::update_role(body.object())?;
    Ok(Json(roles::update(&state.db, &id, dto.name.as_deref(), dto.permissions.as_deref()).await?))
}

pub async fn remove(
    State(state): State<AppState>,
    _auth: Authorized<perm::Users>,
    Path(id): Path<String>,
) -> AppResult<StatusCode> {
    roles::remove(&state.db, &id).await?;
    Ok(StatusCode::NO_CONTENT)
}
