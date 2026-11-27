//! D59: moving books in from another accounting system. Administrators only: a conversion
//! creates accounts, customers, vendors, posted documents and the opening entry at once.
//! A preview changes nothing and is not written to the activity log.

use crate::body::JsonBody;
use crate::extract::{parse_uuid, CurrentUser};
use crate::state::AppState;
use app_core::time::today_local;
use app_core::{AppError, AppResult};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use ledger::accounts::AccountType;
use operations::conversion::{self, AccountRow, BalanceRow, OpenDocument, PartyRow};
use operations::imports::DateOrder;
use serde_json::{Map, Value};

fn admin(user: &CurrentUser) -> AppResult<()> {
    if user.user.is_admin() {
        Ok(())
    } else {
        Err(AppError::Forbidden("Only an administrator can move books in from another system".into()))
    }
}

/// A field as text: strings as they are, numbers as written (Excel cells arrive as numbers when
/// a client sends them so), anything else empty.
fn s(m: &Map<String, Value>, key: &str) -> String {
    match m.get(key) {
        Some(Value::String(v)) => v.clone(),
        Some(Value::Number(n)) => n.to_string(),
        _ => String::new(),
    }
}

fn objects<'a>(m: &'a Map<String, Value>, key: &str) -> AppResult<Vec<&'a Map<String, Value>>> {
    match m.get(key) {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::Array(a)) => a
            .iter()
            .map(|v| v.as_object().ok_or_else(|| AppError::bad_request(format!("each row of {key} must be an object"))))
            .collect(),
        Some(_) => Err(AppError::bad_request(format!("{key} must be an array"))),
    }
}

fn parties(m: &Map<String, Value>, key: &str) -> AppResult<Vec<PartyRow>> {
    Ok(objects(m, key)?
        .into_iter()
        .map(|r| PartyRow {
            name: s(r, "name"),
            email: s(r, "email"),
            phone: s(r, "phone"),
            address: s(r, "address"),
            tax_id: s(r, "taxId"),
        })
        .collect())
}

fn documents(m: &Map<String, Value>, key: &str, party: &str) -> AppResult<Vec<OpenDocument>> {
    Ok(objects(m, key)?
        .into_iter()
        .map(|r| OpenDocument {
            number: s(r, "number"),
            party: s(r, party),
            date: s(r, "date"),
            due_date: s(r, "dueDate"),
            amount: s(r, "amount"),
        })
        .collect())
}

fn request(m: &Map<String, Value>) -> AppResult<conversion::Request> {
    let date_order = match s(m, "dateOrder").as_str() {
        "" => DateOrder::Dmy,
        d => DateOrder::parse(d).ok_or_else(|| AppError::bad_request("dateOrder must be DMY, MDY or YMD"))?,
    };
    let accounts = objects(m, "accounts")?
        .into_iter()
        .map(|r| {
            let kind = s(r, "type");
            Ok(AccountRow {
                key: s(r, "key"),
                account_id: Some(s(r, "accountId")).filter(|a| !a.is_empty()),
                code: s(r, "code"),
                name: s(r, "name"),
                subtype: {
                    let sub = s(r, "subtype");
                    let sub = sub.trim();
                    if sub.chars().count() > 50 {
                        return Err(AppError::bad_request("subtype must be at most 50 characters"));
                    }
                    Some(sub.to_string()).filter(|v| !v.is_empty())
                },
                account_type: match kind.trim() {
                    "" => None,
                    k => Some(AccountType::parse(&k.to_ascii_uppercase()).ok_or_else(|| {
                        AppError::bad_request(format!("type must be one of {}", AccountType::ALL.join(", ")))
                    })?),
                },
            })
        })
        .collect::<AppResult<Vec<_>>>()?;
    let balances = objects(m, "balances")?
        .into_iter()
        .map(|r| BalanceRow { account: s(r, "account"), debit: s(r, "debit"), credit: s(r, "credit") })
        .collect();
    Ok(conversion::Request {
        source: s(m, "source"),
        cutover_date: s(m, "cutoverDate"),
        date_order,
        accounts,
        customers: parties(m, "customers")?,
        vendors: parties(m, "vendors")?,
        balances,
        invoices: documents(m, "invoices", "customer")?,
        bills: documents(m, "bills", "vendor")?,
        accept_differences: m.get("acceptDifferences").and_then(Value::as_bool).unwrap_or(false),
    })
}

pub async fn list(
    State(state): State<AppState>,
    user: CurrentUser,
) -> AppResult<Json<Vec<conversion::ConversionRecord>>> {
    admin(&user)?;
    Ok(Json(conversion::list(&state.db).await?))
}

pub async fn preview(
    State(state): State<AppState>,
    user: CurrentUser,
    body: JsonBody,
) -> AppResult<Json<conversion::Outcome>> {
    admin(&user)?;
    let req = request(&body.object())?;
    Ok(Json(conversion::run(&state.db, req, Some(&user.user.id), false).await?))
}

pub async fn run(
    State(state): State<AppState>,
    user: CurrentUser,
    body: JsonBody,
) -> AppResult<(StatusCode, Json<conversion::Outcome>)> {
    admin(&user)?;
    let req = request(&body.object())?;
    let out = conversion::run(&state.db, req, Some(&user.user.id), true).await?;
    if out.id.is_none() {
        // Nothing was saved: answer 400 with the first problem, the full list in the preview.
        let first = out.issues.first().map(|i| i.message.clone()).unwrap_or_else(|| "Nothing was saved".into());
        return Err(AppError::bad_request(format!(
            "Nothing was saved: {first}{}",
            if out.issues.len() > 1 {
                format!(" (and {} more; see the preview)", out.issues.len() - 1)
            } else {
                String::new()
            }
        )));
    }
    Ok((StatusCode::CREATED, Json(out)))
}

pub async fn reverse(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<String>,
) -> AppResult<(StatusCode, Json<conversion::ConversionRecord>)> {
    admin(&user)?;
    parse_uuid(&id)?;
    let out = conversion::reverse(&state.db, &id, &today_local().to_string()).await?;
    Ok((StatusCode::CREATED, Json(out)))
}
