//! D22: bank reconciliation. Everything needs the `ledger` permission. Nothing here posts to
//! the ledger; see `ledger::reconciliation` for the rules.

use crate::body::JsonBody;
use crate::extract::{perm, Authorized};
use crate::state::AppState;
use crate::validate::{self, Rule, Validator};
use app_core::money::Money;
use app_core::{AppError, AppResult};
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use ledger::reconciliation as rec;
use serde_json::Value;

type Pairs = Query<Vec<(String, String)>>;

/// `GET /api/reconciliations?accountId=…`: the account's reconciliations, newest first.
pub async fn list(State(s): State<AppState>, _a: Authorized<perm::Ledger>, Query(p): Pairs) -> AppResult<Json<Value>> {
    let mut v = Validator::new(validate::query_map(p));
    v.field("accountId", false, &[Rule::IsUuid]);
    let q = v.finish()?;
    let account = q.get("accountId").and_then(Value::as_str).unwrap_or_default();
    let mut conn = s.db.read().acquire().await?;
    let rows = rec::list(&mut conn, account).await?;
    Ok(Json(Value::Array(rows.iter().map(|r| r.to_json()).collect())))
}

/// `POST /api/reconciliations` `{accountId, statementDate, statementBalance}`.
pub async fn start(
    State(s): State<AppState>,
    a: Authorized<perm::Ledger>,
    body: JsonBody,
) -> AppResult<(StatusCode, Json<Value>)> {
    let mut v = Validator::new(body.object());
    v.field("accountId", false, &[Rule::IsUuid]).field("statementDate", false, &[Rule::IsDateFormat]).field(
        "statementBalance",
        false,
        &[Rule::IsMoneyNumber],
    );
    let m = v.finish()?;
    let text = |k: &str| m.get(k).and_then(Value::as_str).unwrap_or_default().to_string();
    let balance = m.get("statementBalance").and_then(Money::from_json).unwrap_or_default();
    let ws = rec::start(&s.db, &text("accountId"), &text("statementDate"), balance, Some(&a.user().id)).await?;
    Ok((StatusCode::CREATED, Json(ws.to_json())))
}

pub async fn get(
    State(s): State<AppState>,
    _a: Authorized<perm::Ledger>,
    Path(id): Path<String>,
) -> AppResult<Json<Value>> {
    let mut conn = s.db.read().acquire().await?;
    Ok(Json(rec::workspace(&mut conn, &id).await?.to_json()))
}

/// `PUT /api/reconciliations/:id/lines` `{lineIds: [...]}`: the ticked lines, all of them.
pub async fn set_lines(
    State(s): State<AppState>,
    _a: Authorized<perm::Ledger>,
    Path(id): Path<String>,
    body: JsonBody,
) -> AppResult<Json<Value>> {
    let mut v = Validator::new(body.object());
    v.field("lineIds", false, &[Rule::IsArray, Rule::IsStringEach]);
    let m = v.finish()?;
    let ids: Vec<String> = m
        .get("lineIds")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect())
        .unwrap_or_default();
    if ids.len() > 10_000 {
        return Err(AppError::bad_request("Too many lines in one request"));
    }
    Ok(Json(rec::set_cleared(&s.db, &id, &ids).await?.to_json()))
}

pub async fn complete(
    State(s): State<AppState>,
    a: Authorized<perm::Ledger>,
    Path(id): Path<String>,
) -> AppResult<Json<Value>> {
    Ok(Json(rec::complete(&s.db, &id, Some(&a.user().id)).await?.to_json()))
}

pub async fn undo(
    State(s): State<AppState>,
    _a: Authorized<perm::Ledger>,
    Path(id): Path<String>,
) -> AppResult<Json<Value>> {
    Ok(Json(rec::undo(&s.db, &id).await?.to_json()))
}

pub async fn discard(
    State(s): State<AppState>,
    _a: Authorized<perm::Ledger>,
    Path(id): Path<String>,
) -> AppResult<StatusCode> {
    rec::discard(&s.db, &id).await?;
    Ok(StatusCode::NO_CONTENT)
}
