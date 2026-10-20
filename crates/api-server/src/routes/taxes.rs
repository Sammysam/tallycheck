//! D26: tax codes. Anyone signed in can list them (every invoice and bill form needs them);
//! creating, changing and deleting them needs the `settings` permission.

use crate::body::JsonBody;
use crate::extract::{perm, Authorized, CurrentUser};
use crate::state::AppState;
use crate::validate::ops;
use app_core::AppResult;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use operations::tax;
use serde_json::Value;

pub async fn list(State(s): State<AppState>, _user: CurrentUser) -> AppResult<Json<Value>> {
    let mut c = s.db.read().acquire().await?;
    let codes = tax::find_all(&mut c).await?;
    Ok(Json(Value::Array(codes.iter().map(tax::TaxCode::to_json).collect())))
}

pub async fn create(
    State(s): State<AppState>,
    _a: Authorized<perm::Settings>,
    body: JsonBody,
) -> AppResult<(StatusCode, Json<Value>)> {
    let input = ops::tax_code(body.object(), false)?;
    Ok((StatusCode::CREATED, Json(tax::create(&s.db, input).await?.to_json())))
}

pub async fn update(
    State(s): State<AppState>,
    _a: Authorized<perm::Settings>,
    Path(id): Path<String>,
    body: JsonBody,
) -> AppResult<Json<Value>> {
    let input = ops::tax_code(body.object(), true)?;
    Ok(Json(tax::update(&s.db, &id, input).await?.to_json()))
}

pub async fn remove(
    State(s): State<AppState>,
    _a: Authorized<perm::Settings>,
    Path(id): Path<String>,
) -> AppResult<StatusCode> {
    tax::remove(&s.db, &id).await?;
    Ok(StatusCode::NO_CONTENT)
}
