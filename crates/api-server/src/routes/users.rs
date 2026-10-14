//! Reference `users.controller.ts` — all routes need the `users` permission.

use crate::body::JsonBody;
use crate::extract::{perm, Authorized};
use crate::state::AppState;
use crate::validate;
use app_core::{AppError, AppResult};
use auth::users::{self, CreateUser, UpdateUser, UserView};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde_json::Value;

pub async fn list(State(state): State<AppState>, _auth: Authorized<perm::Users>) -> AppResult<Json<Vec<UserView>>> {
    Ok(Json(users::find_all(&state.db).await?.into_iter().map(UserView::from).collect()))
}

pub async fn create(
    State(state): State<AppState>,
    _auth: Authorized<perm::Users>,
    body: JsonBody,
) -> AppResult<(StatusCode, Json<Value>)> {
    let dto = validate::create_user(body.object())?;
    let user = users::create_user(
        &state.db,
        CreateUser { username: dto.username, password: dto.password, role_id: dto.role_id },
    )
    .await?;
    // The reference returns the freshly saved entity, whose eager `role` relation was never
    // loaded, so the key is absent (not null) in this one response.
    let mut view = serde_json::to_value(UserView::from(user)).map_err(AppError::internal)?;
    if let Some(obj) = view.as_object_mut() {
        obj.remove("role");
    }
    Ok((StatusCode::CREATED, Json(view)))
}

pub async fn update(
    State(state): State<AppState>,
    _auth: Authorized<perm::Users>,
    Path(id): Path<String>,
    body: JsonBody,
) -> AppResult<Json<UserView>> {
    let dto = validate::update_user(body.object())?;
    let user = users::update_user(
        &state.db,
        &id,
        UpdateUser { username: dto.username, role_id: dto.role_id, is_active: dto.is_active },
    )
    .await?;
    Ok(Json(user.into()))
}

/// POST without `@HttpCode`, so 201 like the reference.
pub async fn reset_password(
    State(state): State<AppState>,
    _auth: Authorized<perm::Users>,
    Path(id): Path<String>,
    body: JsonBody,
) -> AppResult<(StatusCode, Json<UserView>)> {
    let password = validate::reset_password(body.object())?;
    let user = users::reset_password(&state.db, &id, &password).await?;
    Ok((StatusCode::CREATED, Json(user.into())))
}

pub async fn reset_two_factor(
    State(state): State<AppState>,
    _auth: Authorized<perm::Users>,
    Path(id): Path<String>,
) -> AppResult<(StatusCode, Json<UserView>)> {
    let user = users::reset_two_factor(&state.db, &id).await?;
    Ok((StatusCode::CREATED, Json(user.into())))
}
