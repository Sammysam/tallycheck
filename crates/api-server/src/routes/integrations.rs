//! D39/D40: Settings → Integrations (Shopify, WooCommerce, PayPal, Stripe). Connecting and the
//! account matching need `settings`; a sync creates draft invoices and bank lines, so it needs
//! `invoices` and `banking`.

use crate::body::JsonBody;
use crate::extract::{perm, Authorized};
use crate::state::AppState;
use app_core::{AppError, AppResult};
use axum::extract::{Path as UrlPath, State};
use axum::Json;
use integrations::{common, Connector};
use ledger::AccountType;
use serde_json::{Map, Value};
use std::path::Path;

fn data_dir(s: &AppState) -> AppResult<&Path> {
    s.data_dir.as_deref().ok_or_else(|| AppError::internal("no data directory configured"))
}

fn connector(id: &str) -> AppResult<Connector> {
    Connector::parse(id).ok_or_else(|| AppError::NotFound(format!("No integration called {id}")))
}

/// Every connector and its state.
pub async fn list(State(s): State<AppState>, _auth: Authorized<perm::Settings>) -> AppResult<Json<Value>> {
    let mut out = Vec::new();
    for c in Connector::ALL {
        out.push(common::connected_status(&s.db, data_dir(&s)?, c).await?);
    }
    Ok(Json(Value::Array(out)))
}

pub async fn get(
    State(s): State<AppState>,
    _auth: Authorized<perm::Settings>,
    UrlPath(id): UrlPath<String>,
) -> AppResult<Json<Value>> {
    Ok(Json(common::connected_status(&s.db, data_dir(&s)?, connector(&id)?).await?))
}

pub async fn connect(
    State(s): State<AppState>,
    _auth: Authorized<perm::Settings>,
    UrlPath(id): UrlPath<String>,
    body: JsonBody,
) -> AppResult<Json<Value>> {
    Ok(Json(integrations::connect(&s.db, data_dir(&s)?, connector(&id)?, &body.object()).await?))
}

pub async fn disconnect(
    State(s): State<AppState>,
    _auth: Authorized<perm::Settings>,
    UrlPath(id): UrlPath<String>,
) -> AppResult<Json<Value>> {
    Ok(Json(common::disconnect(&s.db, data_dir(&s)?, connector(&id)?).await?))
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
    UrlPath(id): UrlPath<String>,
    body: JsonBody,
) -> AppResult<Json<Value>> {
    let c = connector(&id)?;
    let m = body.object();
    let mut values: Vec<(&str, Option<String>)> = Vec::new();
    let accounts: [(&str, &[AccountType], &str); 5] = [
        ("clearingAccountId", &[AccountType::Asset], "money account"),
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
    if c == Connector::Shopify {
        if let Some(v) = opt_text(&m, "apiVersion")? {
            if let Some(ver) = &v {
                let ok = ver.len() == 7
                    && ver.as_bytes()[4] == b'-'
                    && ver.chars().filter(|c| c.is_ascii_digit()).count() == 6;
                if !ok {
                    return Err(AppError::bad_request("apiVersion looks like 2026-07"));
                }
            }
            values.push(("apiVersion", v));
        }
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
    common::save(&s.db, c, &values).await?;
    Ok(Json(common::connected_status(&s.db, data_dir(&s)?, c).await?))
}

/// Adds the connector's money account ("1090 Shopify Clearing", "1091 PayPal", "1092 Stripe").
pub async fn clearing_account(
    State(s): State<AppState>,
    _auth: Authorized<perm::Settings>,
    UrlPath(id): UrlPath<String>,
) -> AppResult<Json<Value>> {
    let c = connector(&id)?;
    common::ensure_clearing_account(&s.db, c).await?;
    Ok(Json(common::connected_status(&s.db, data_dir(&s)?, c).await?))
}

pub async fn sync(
    State(s): State<AppState>,
    auth: Authorized<perm::Invoices>,
    UrlPath(id): UrlPath<String>,
) -> AppResult<Json<Value>> {
    if !auth.user().has_permission("banking") {
        return Err(AppError::Forbidden("Insufficient permissions".into()));
    }
    Ok(Json(integrations::sync(&s.db, data_dir(&s)?, connector(&id)?, Some(&auth.user().id)).await?))
}
