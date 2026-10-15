//! W5 routes: payroll runs, rates and company details (reference `payroll-runs.controller.ts`).
//! Everything needs the `payroll` permission. Nest answers POST with 201 unless told otherwise,
//! so post / pay / remit / void / slips-filed / add line all return 201.

use crate::body::JsonBody;
use crate::extract::{parse_uuid, perm, Authorized};
use crate::state::AppState;
use crate::validate::payroll as dto;
use app_core::AppResult;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use employees::Actor;
use payroll::runs;
use serde_json::Value;

type Created = AppResult<(StatusCode, Json<Value>)>;

fn actor(a: &Authorized<perm::Payroll>) -> Actor {
    Actor { id: Some(a.user().id.clone()), username: Some(a.user().username.clone()) }
}

pub async fn rates_get(State(s): State<AppState>, _a: Authorized<perm::Payroll>) -> AppResult<Json<Value>> {
    Ok(Json(runs::get_rates(&s.db).await?))
}

pub async fn rates_update(
    State(s): State<AppState>,
    _a: Authorized<perm::Payroll>,
    body: JsonBody,
) -> AppResult<Json<Value>> {
    let values = dto::rates(body.object())?;
    Ok(Json(runs::update_rates(&s.db, values).await?))
}

pub async fn company_get(State(s): State<AppState>, _a: Authorized<perm::Payroll>) -> AppResult<Json<Value>> {
    Ok(Json(runs::get_company(&s.db).await?))
}

pub async fn company_update(
    State(s): State<AppState>,
    _a: Authorized<perm::Payroll>,
    body: JsonBody,
) -> AppResult<Json<Value>> {
    let values = dto::company(body.object())?;
    Ok(Json(runs::update_company(&s.db, values).await?))
}

pub async fn list(State(s): State<AppState>, _a: Authorized<perm::Payroll>) -> AppResult<Json<Vec<Value>>> {
    Ok(Json(runs::find_all(&s.db).await?))
}

pub async fn create(State(s): State<AppState>, a: Authorized<perm::Payroll>, body: JsonBody) -> Created {
    let (period, notes) = dto::create_run(body.object())?;
    Ok((StatusCode::CREATED, Json(runs::create(&s.db, &period, notes, &actor(&a)).await?)))
}

pub async fn get(
    State(s): State<AppState>,
    _a: Authorized<perm::Payroll>,
    Path(id): Path<String>,
) -> AppResult<Json<Value>> {
    parse_uuid(&id)?;
    Ok(Json(runs::find_one(&s.db, &id).await?))
}

pub async fn remove(
    State(s): State<AppState>,
    _a: Authorized<perm::Payroll>,
    Path(id): Path<String>,
) -> AppResult<StatusCode> {
    parse_uuid(&id)?;
    runs::remove(&s.db, &id).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn line_add(
    State(s): State<AppState>,
    _a: Authorized<perm::Payroll>,
    Path(id): Path<String>,
    body: JsonBody,
) -> Created {
    parse_uuid(&id)?;
    let employee_id = dto::add_line(body.object())?;
    Ok((StatusCode::CREATED, Json(runs::add_line(&s.db, &id, &employee_id).await?)))
}

pub async fn line_update(
    State(s): State<AppState>,
    _a: Authorized<perm::Payroll>,
    Path((id, line_id)): Path<(String, String)>,
    body: JsonBody,
) -> AppResult<Json<Value>> {
    parse_uuid(&id)?;
    parse_uuid(&line_id)?;
    let patch = dto::line_patch(body.object())?;
    Ok(Json(runs::update_line(&s.db, &id, &line_id, patch).await?))
}

pub async fn line_remove(
    State(s): State<AppState>,
    _a: Authorized<perm::Payroll>,
    Path((id, line_id)): Path<(String, String)>,
) -> AppResult<Json<Value>> {
    parse_uuid(&id)?;
    parse_uuid(&line_id)?;
    Ok(Json(runs::remove_line(&s.db, &id, &line_id).await?))
}

pub async fn post(State(s): State<AppState>, _a: Authorized<perm::Payroll>, Path(id): Path<String>) -> Created {
    parse_uuid(&id)?;
    Ok((StatusCode::CREATED, Json(runs::post(&s.db, &id).await?)))
}

pub async fn pay(
    State(s): State<AppState>,
    _a: Authorized<perm::Payroll>,
    Path(id): Path<String>,
    body: JsonBody,
) -> Created {
    parse_uuid(&id)?;
    let (account, date) = dto::pay(body.object())?;
    Ok((StatusCode::CREATED, Json(runs::pay(&s.db, &id, &account, &date).await?)))
}

pub async fn remit(
    State(s): State<AppState>,
    a: Authorized<perm::Payroll>,
    Path(id): Path<String>,
    body: JsonBody,
) -> Created {
    parse_uuid(&id)?;
    let r = dto::remit(body.object())?;
    Ok((StatusCode::CREATED, Json(runs::remit(&s.db, &id, r, &actor(&a)).await?)))
}

pub async fn slips_filed(State(s): State<AppState>, _a: Authorized<perm::Payroll>, Path(id): Path<String>) -> Created {
    parse_uuid(&id)?;
    Ok((StatusCode::CREATED, Json(runs::slips_filed(&s.db, &id).await?)))
}

pub async fn void(State(s): State<AppState>, _a: Authorized<perm::Payroll>, Path(id): Path<String>) -> Created {
    parse_uuid(&id)?;
    Ok((StatusCode::CREATED, Json(runs::void(&s.db, &id).await?)))
}
