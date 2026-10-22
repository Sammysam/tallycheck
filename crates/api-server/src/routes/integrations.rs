//! D39: Settings → Integrations (Shopify). Connecting and the account matching need `settings`;
//! a sync creates draft invoices and bank lines, so it needs `invoices` and `banking`.

use crate::body::JsonBody;
use crate::extract::{perm, Authorized};
use crate::state::AppState;
use app_core::{AppError, AppResult};
use axum::extract::State;
use axum::Json;
use integrations::shopify;
use ledger::AccountType;
use serde_json::{Map, Value};
use std::path::Path;

fn data_dir(s: &AppState) -> AppResult<&Path> {
    s.data_dir.as_deref().ok_or_else(|| AppError::internal("no data directory configured"))
}

pub async fn get(State(s): State<AppState>, _auth: Authorized<perm::Settings>) -> AppResult<Json<Value>> {
    let cfg = shopify::load_config(&s.db).await?;
    Ok(Json(shopify::status_json(&cfg, shopify::connected(data_dir(&s)?))))
}

pub async fn connect(
    State(s): State<AppState>,
    _auth: Authorized<perm::Settings>,
    body: JsonBody,
) -> AppResult<Json<Value>> {
    let m = body.object();
    let text = |k: &str| m.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    Ok(Json(shopify::connect(&s.db, data_dir(&s)?, &text("shop"), &text("token")).await?))
}

pub async fn disconnect(State(s): State<AppState>, _auth: Authorized<perm::Settings>) -> AppResult<Json<Value>> {
    Ok(Json(shopify::disconnect(&s.db, data_dir(&s)?).await?))
}

/// `None` = not sent; `Some(None)` = cleared.
fn opt_text(m: &Map<String, Value>, k: &str) -> AppResult<Option<Option<String>>> {
    match m.get(k) {
        None => Ok(None),
        Some(Value::Null) => Ok(Some(None)),
        Some(Value::String(s)) => Ok(Some(Some(s.trim().to_string()).filter(|s| !s.is_empty()))),
        Some(_) => Err(AppError::bad_request(format!("{k} must be a string"))),
    }
}

async fn check_account(s: &AppState, id: &str, allowed: &[AccountType], what: &str) -> AppResult<()> {
    let mut conn = s.db.read().acquire().await?;
    let a = ledger::accounts::find_one(&mut conn, id).await?;
    if !allowed.contains(&a.account_type) {
        return Err(AppError::bad_request(format!("{} {} cannot be the {what}", a.code, a.name)));
    }
    Ok(())
}

/// The account matching, start date, API version and automatic sync.
pub async fn update(
    State(s): State<AppState>,
    _auth: Authorized<perm::Settings>,
    body: JsonBody,
) -> AppResult<Json<Value>> {
    let m = body.object();
    let mut values: Vec<(&str, Option<String>)> = Vec::new();
    let accounts: [(&str, &[AccountType], &str); 5] = [
        ("clearingAccountId", &[AccountType::Asset], "Shopify clearing account"),
        ("payoutAccountId", &[AccountType::Asset], "account payouts reach"),
        ("feeAccountId", &[AccountType::Expense], "fee account"),
        ("incomeAccountId", &[AccountType::Revenue], "sales account"),
        ("shippingAccountId", &[AccountType::Revenue], "shipping account"),
    ];
    for (key, allowed, what) in accounts {
        if let Some(v) = opt_text(&m, key)? {
            if let Some(id) = &v {
                check_account(&s, id, allowed, what).await?;
            }
            values.push((key, v));
        }
    }
    if let Some(v) = opt_text(&m, "startDate")? {
        if let Some(d) = &v {
            if chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d").is_err() {
                return Err(AppError::bad_request("startDate must be a valid date (YYYY-MM-DD)"));
            }
        }
        values.push(("startDate", v));
    }
    if let Some(v) = opt_text(&m, "apiVersion")? {
        if let Some(ver) = &v {
            let ok =
                ver.len() == 7 && ver.as_bytes()[4] == b'-' && ver.chars().filter(|c| c.is_ascii_digit()).count() == 6;
            if !ok {
                return Err(AppError::bad_request("apiVersion looks like 2026-07"));
            }
        }
        values.push(("apiVersion", v));
    }
    match m.get("autoSync") {
        None => {}
        Some(Value::Bool(b)) => values.push(("autoSync", Some(b.to_string()))),
        Some(_) => return Err(AppError::bad_request("autoSync must be a boolean value")),
    }
    if let Some(map) = m.get("taxMap") {
        let obj = map.as_object().ok_or_else(|| AppError::bad_request("taxMap must be an object"))?;
        let mut clean = serde_json::Map::new();
        for (k, v) in obj {
            let Some(code) = v.as_str().filter(|c| !c.is_empty()) else { continue };
            let mut conn = s.db.read().acquire().await?;
            operations::tax::usable(&mut conn, code).await?;
            clean.insert(k.clone(), Value::String(code.to_string()));
        }
        values.push(("taxMap", Some(Value::Object(clean).to_string())));
    }
    shopify::save(&s.db, &values).await?;
    let cfg = shopify::load_config(&s.db).await?;
    Ok(Json(shopify::status_json(&cfg, shopify::connected(data_dir(&s)?))))
}

/// Adds "1090 Shopify Clearing" (an asset account the store's money passes through) if needed
/// and chooses it.
pub async fn clearing_account(State(s): State<AppState>, _auth: Authorized<perm::Settings>) -> AppResult<Json<Value>> {
    shopify::ensure_clearing_account(&s.db).await?;
    let cfg = shopify::load_config(&s.db).await?;
    Ok(Json(shopify::status_json(&cfg, shopify::connected(data_dir(&s)?))))
}

pub async fn sync(State(s): State<AppState>, auth: Authorized<perm::Invoices>) -> AppResult<Json<Value>> {
    if !auth.user().has_permission("banking") {
        return Err(AppError::Forbidden("Insufficient permissions".into()));
    }
    Ok(Json(shopify::sync(&s.db, data_dir(&s)?, Some(&auth.user().id)).await?))
}
