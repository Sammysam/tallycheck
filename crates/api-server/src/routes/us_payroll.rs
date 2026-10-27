//! D42 US payroll routes (ported from the web edition's wave U2), all needing the `payroll` permission:
//! `GET|PATCH /payroll/us/settings`, `GET|PUT /payroll/employees/:id/us-tax`,
//! `POST /payroll/us/preview` (one paycheck, nothing saved; 201 like every POST here).

use crate::body::JsonBody;
use crate::extract::{parse_uuid, perm, Authorized};
use crate::state::AppState;
use crate::validate::us_payroll as dto;
use app_core::AppResult;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use payroll::us_setup;
use serde_json::Value;

pub async fn settings_get(State(s): State<AppState>, _a: Authorized<perm::Payroll>) -> AppResult<Json<Value>> {
    Ok(Json(us_setup::get_settings(&s.db).await?))
}

pub async fn settings_update(
    State(s): State<AppState>,
    _a: Authorized<perm::Payroll>,
    body: JsonBody,
) -> AppResult<Json<Value>> {
    let patch = dto::settings(body.object())?;
    Ok(Json(us_setup::update_settings(&s.db, patch).await?))
}

pub async fn tax_get(
    State(s): State<AppState>,
    _a: Authorized<perm::Payroll>,
    Path(id): Path<String>,
) -> AppResult<Json<Value>> {
    parse_uuid(&id)?;
    Ok(Json(us_setup::get_tax(&s.db, &id).await?))
}

pub async fn tax_put(
    State(s): State<AppState>,
    _a: Authorized<perm::Payroll>,
    Path(id): Path<String>,
    body: JsonBody,
) -> AppResult<Json<Value>> {
    parse_uuid(&id)?;
    let t = dto::tax(body.object())?;
    Ok(Json(us_setup::put_tax(&s.db, &id, t).await?))
}

pub async fn preview(
    State(s): State<AppState>,
    _a: Authorized<perm::Payroll>,
    body: JsonBody,
) -> AppResult<(StatusCode, Json<Value>)> {
    let (employee_id, req) = dto::preview(body.object())?;
    Ok((StatusCode::CREATED, Json(us_setup::preview(&s.db, &employee_id, req).await?)))
}
