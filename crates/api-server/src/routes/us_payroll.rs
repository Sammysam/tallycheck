//! D42/D43 US payroll routes (ported from the web edition's waves U2–U5b), all needing the
//! `payroll` permission:
//! `GET|PATCH /payroll/us/settings`, `GET|PUT /payroll/employees/:id/us-tax`,
//! `POST /payroll/us/preview` (one paycheck, nothing saved; 201 like every POST here).
//! U3 runs: `GET|POST /payroll/us/runs`, `GET|DELETE /payroll/us/runs/:id`,
//! `POST /payroll/us/runs/:id/lines`, `PATCH|DELETE /payroll/us/runs/:id/lines/:lineId`,
//! `POST /payroll/us/runs/:id/{post,pay,remit,void}`.
//! U5a forms: `GET /payroll/us/reports/941?year&quarter`, `/940?year`, `/w2?year`.
//! U5b state income tax: `GET /payroll/us/state-taxes`, `PUT|DELETE /payroll/us/state-taxes/:state`.

use crate::body::JsonBody;
use crate::extract::{parse_uuid, perm, Authorized};
use crate::state::AppState;
use crate::validate::us_payroll as dto;
use app_core::AppResult;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use employees::Actor;
use payroll::{us_reports, us_runs, us_setup, us_state};
use serde_json::Value;
use std::collections::HashMap;

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

// ─── Runs ───────────────────────────────────────────────────────────────────

type Created = AppResult<(StatusCode, Json<Value>)>;

fn actor(a: &Authorized<perm::Payroll>) -> Actor {
    Actor { id: Some(a.user().id.clone()), username: Some(a.user().username.clone()) }
}

pub async fn runs(State(s): State<AppState>, _a: Authorized<perm::Payroll>) -> AppResult<Json<Vec<Value>>> {
    Ok(Json(us_runs::find_all(&s.db).await?))
}

pub async fn run_create(State(s): State<AppState>, a: Authorized<perm::Payroll>, body: JsonBody) -> Created {
    let n = dto::create_run(body.object())?;
    Ok((StatusCode::CREATED, Json(us_runs::create(&s.db, n, &actor(&a)).await?)))
}

pub async fn run_get(
    State(s): State<AppState>,
    _a: Authorized<perm::Payroll>,
    Path(id): Path<String>,
) -> AppResult<Json<Value>> {
    parse_uuid(&id)?;
    Ok(Json(us_runs::find_one(&s.db, &id).await?))
}

pub async fn run_remove(
    State(s): State<AppState>,
    _a: Authorized<perm::Payroll>,
    Path(id): Path<String>,
) -> AppResult<StatusCode> {
    parse_uuid(&id)?;
    us_runs::remove(&s.db, &id).await?;
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
    Ok((StatusCode::CREATED, Json(us_runs::add_line(&s.db, &id, &employee_id).await?)))
}

pub async fn line_update(
    State(s): State<AppState>,
    _a: Authorized<perm::Payroll>,
    Path((id, line_id)): Path<(String, String)>,
    body: JsonBody,
) -> AppResult<Json<Value>> {
    parse_uuid(&id)?;
    parse_uuid(&line_id)?;
    let p = dto::line_patch(body.object())?;
    Ok(Json(us_runs::update_line(&s.db, &id, &line_id, p).await?))
}

pub async fn line_remove(
    State(s): State<AppState>,
    _a: Authorized<perm::Payroll>,
    Path((id, line_id)): Path<(String, String)>,
) -> AppResult<Json<Value>> {
    parse_uuid(&id)?;
    parse_uuid(&line_id)?;
    Ok(Json(us_runs::remove_line(&s.db, &id, &line_id).await?))
}

pub async fn run_post(State(s): State<AppState>, _a: Authorized<perm::Payroll>, Path(id): Path<String>) -> Created {
    parse_uuid(&id)?;
    Ok((StatusCode::CREATED, Json(us_runs::post(&s.db, &id).await?)))
}

pub async fn run_pay(
    State(s): State<AppState>,
    _a: Authorized<perm::Payroll>,
    Path(id): Path<String>,
    body: JsonBody,
) -> Created {
    parse_uuid(&id)?;
    let (account, date) = dto::pay(body.object())?;
    Ok((StatusCode::CREATED, Json(us_runs::pay(&s.db, &id, &account, date).await?)))
}

pub async fn run_remit(
    State(s): State<AppState>,
    a: Authorized<perm::Payroll>,
    Path(id): Path<String>,
    body: JsonBody,
) -> Created {
    parse_uuid(&id)?;
    let r = dto::remit(body.object())?;
    Ok((StatusCode::CREATED, Json(us_runs::remit(&s.db, &id, r, &actor(&a)).await?)))
}

pub async fn run_void(State(s): State<AppState>, _a: Authorized<perm::Payroll>, Path(id): Path<String>) -> Created {
    parse_uuid(&id)?;
    Ok((StatusCode::CREATED, Json(us_runs::void(&s.db, &id).await?)))
}

// ─── Form figures ───────────────────────────────────────────────────────────

type Params = Query<HashMap<String, String>>;

pub async fn report_941(
    State(s): State<AppState>,
    _a: Authorized<perm::Payroll>,
    Query(q): Params,
) -> AppResult<Json<Value>> {
    let (year, quarter) = dto::report_period(&q, true)?;
    Ok(Json(us_reports::form_941(&s.db, year, quarter).await?))
}

pub async fn report_940(
    State(s): State<AppState>,
    _a: Authorized<perm::Payroll>,
    Query(q): Params,
) -> AppResult<Json<Value>> {
    let (year, _) = dto::report_period(&q, false)?;
    Ok(Json(us_reports::form_940(&s.db, year).await?))
}

pub async fn report_w2(
    State(s): State<AppState>,
    _a: Authorized<perm::Payroll>,
    Query(q): Params,
) -> AppResult<Json<Value>> {
    let (year, _) = dto::report_period(&q, false)?;
    Ok(Json(us_reports::w2(&s.db, year).await?))
}

// ─── State income tax rules ─────────────────────────────────────────────────

pub async fn state_taxes(State(s): State<AppState>, _a: Authorized<perm::Payroll>) -> AppResult<Json<Value>> {
    Ok(Json(us_state::list_rules(&s.db).await?))
}

pub async fn state_tax_put(
    State(s): State<AppState>,
    _a: Authorized<perm::Payroll>,
    Path(state): Path<String>,
    body: JsonBody,
) -> AppResult<Json<Value>> {
    let state = dto::state_code(&state)?;
    let (rule, notes) = dto::state_rule(body.object())?;
    Ok(Json(us_state::put_rule(&s.db, &state, rule, notes).await?))
}

pub async fn state_tax_delete(
    State(s): State<AppState>,
    _a: Authorized<perm::Payroll>,
    Path(state): Path<String>,
) -> AppResult<StatusCode> {
    let state = dto::state_code(&state)?;
    us_state::delete_rule(&s.db, &state).await?;
    Ok(StatusCode::NO_CONTENT)
}
