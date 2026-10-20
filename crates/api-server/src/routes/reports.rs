//! Reference `reports.controller.ts` — every route needs the `reports` permission.
//!
//! Query dates follow the reference: an absent value takes the default (the start of the
//! current year / today), an empty value means "no bound", and a malformed value is a 400
//! (the reference passed it to SQL and crashed with a 500). "Today" is the Sri Lanka date (D5).

use crate::extract::{perm, Authorized};
use crate::state::AppState;
use crate::validate::{self, Rule};
use app_core::time::today_local;
use app_core::{AppError, AppResult};
use axum::extract::{Query, State};
use axum::Json;
use chrono::{Datelike, NaiveDate};
use serde_json::{Map, Value};

type Pairs = Query<Vec<(String, String)>>;

/// `None` = absent; `Some("")` = empty; otherwise a validated `YYYY-MM-DD`.
fn raw_date(query: &Map<String, Value>, prop: &str) -> Result<Option<String>, AppError> {
    let raw = match query.get(prop) {
        None => return Ok(None),
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(items)) => items.first().and_then(Value::as_str).unwrap_or_default().to_string(),
        Some(_) => String::new(),
    };
    if raw.is_empty() || app_core::time::is_date_format(&raw) {
        Ok(Some(raw))
    } else {
        Err(AppError::validation(vec![Rule::IsDateFormat.message(prop)]))
    }
}

fn today() -> NaiveDate {
    today_local()
}

/// D25: the first day of the current financial year — the month in the `region.fiscalYearStart`
/// setting (1 = January, the default; 4 = April; 7 = July …). The reference used 1 January.
async fn fiscal_year_start(db: &database::Db) -> AppResult<String> {
    let month = database::settings::find_all(db).await?.remove("region.fiscalYearStart");
    let month = month.and_then(|m| m.trim().parse::<u32>().ok()).filter(|m| (1..=12).contains(m)).unwrap_or(1);
    let t = today();
    let year = if t.month() >= month { t.year() } else { t.year() - 1 };
    Ok(NaiveDate::from_ymd_opt(year, month, 1).unwrap_or(t).to_string())
}

pub async fn profit_loss(
    State(s): State<AppState>,
    _a: Authorized<perm::Reports>,
    Query(p): Pairs,
) -> AppResult<Json<Value>> {
    let q = validate::query_map(p);
    let from = raw_date(&q, "dateFrom")?;
    let to = raw_date(&q, "dateTo")?;
    let mut c = s.db.read().acquire().await?;
    let from = match from {
        Some(f) => f,
        None => fiscal_year_start(&s.db).await?,
    };
    let to = to.unwrap_or_else(|| today().to_string());
    Ok(Json(reports::profit_and_loss(&mut c, &from, &to).await?.to_json()))
}

pub async fn balance_sheet(
    State(s): State<AppState>,
    _a: Authorized<perm::Reports>,
    Query(p): Pairs,
) -> AppResult<Json<Value>> {
    let q = validate::query_map(p);
    let as_of = raw_date(&q, "asOf")?.unwrap_or_else(|| today().to_string());
    let mut c = s.db.read().acquire().await?;
    Ok(Json(reports::balance_sheet(&mut c, &as_of).await?))
}

/// Ageing is measured to `asOf`, or to today when it is absent or empty.
fn aging_day(q: &Map<String, Value>) -> Result<NaiveDate, AppError> {
    match raw_date(q, "asOf")?.filter(|s| !s.is_empty()) {
        Some(d) => NaiveDate::parse_from_str(&d, "%Y-%m-%d").map_err(AppError::internal),
        None => Ok(today()),
    }
}

pub async fn ar_aging(
    State(s): State<AppState>,
    _a: Authorized<perm::Reports>,
    Query(p): Pairs,
) -> AppResult<Json<Value>> {
    let day = aging_day(&validate::query_map(p))?;
    let mut c = s.db.read().acquire().await?;
    Ok(Json(reports::ar_aging(&mut c, day).await?))
}

pub async fn ap_aging(
    State(s): State<AppState>,
    _a: Authorized<perm::Reports>,
    Query(p): Pairs,
) -> AppResult<Json<Value>> {
    let day = aging_day(&validate::query_map(p))?;
    let mut c = s.db.read().acquire().await?;
    Ok(Json(reports::ap_aging(&mut c, day).await?))
}

/// D24: `GET /api/reports/cash-flow?dateFrom&dateTo&cashAccounts=id,id` — dates as for profit
/// and loss; `cashAccounts` defaults to the 10xx asset accounts.
pub async fn cash_flow(
    State(s): State<AppState>,
    _a: Authorized<perm::Reports>,
    Query(p): Pairs,
) -> AppResult<Json<Value>> {
    let q = validate::query_map(p);
    let from = raw_date(&q, "dateFrom")?;
    let to = raw_date(&q, "dateTo")?;
    let mut c = s.db.read().acquire().await?;
    let from = match from {
        Some(f) => f,
        None => fiscal_year_start(&s.db).await?,
    };
    let to = to.unwrap_or_else(|| today().to_string());
    let mut ids: Vec<String> = match q.get("cashAccounts").and_then(Value::as_str) {
        Some(list) if !list.trim().is_empty() => list.split(',').map(|id| id.trim().to_string()).collect(),
        _ => reports::cash_flow::default_cash_accounts(&mut c).await?,
    };
    ids.sort();
    ids.dedup();
    if ids.len() > 50 || ids.iter().any(|id| !app_core::ids::is_uuid(id)) {
        return Err(AppError::validation(vec!["cashAccounts must be a comma-separated list of account ids".into()]));
    }
    Ok(Json(reports::cash_flow::cash_flow(&mut c, &from, &to, &ids).await?))
}
