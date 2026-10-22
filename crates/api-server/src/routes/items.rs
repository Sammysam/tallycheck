//! D38: products & services and stock adjustments.
//!
//! Reading the list is open to any signed-in user (the invoice and bill forms need it); adding
//! or changing items needs `invoices` or `bills`; a stock adjustment posts to the ledger, so it
//! needs `ledger`.

use crate::body::JsonBody;
use crate::extract::{parse_uuid, perm, Authorized, CurrentUser};
use crate::state::AppState;
use app_core::money::Money;
use app_core::{AppError, AppResult};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use operations::stock::{self, Adjustment, ItemFields};
use serde_json::{Map, Value};

fn may_edit(user: &CurrentUser) -> AppResult<()> {
    if user.user.has_permission("invoices") || user.user.has_permission("bills") {
        Ok(())
    } else {
        Err(AppError::Forbidden("Insufficient permissions".into()))
    }
}

/// `None` = not sent; `Some(None)` = null or blank; a non-string is a 400.
fn text(m: &Map<String, Value>, key: &str) -> AppResult<Option<Option<String>>> {
    match m.get(key) {
        None => Ok(None),
        Some(Value::Null) => Ok(Some(None)),
        Some(Value::String(s)) => Ok(Some(Some(s.trim().to_string()).filter(|s| !s.is_empty()))),
        Some(_) => Err(AppError::bad_request(format!("{key} must be a string"))),
    }
}

fn id(m: &Map<String, Value>, key: &str) -> AppResult<Option<Option<String>>> {
    let v = text(m, key)?;
    if let Some(Some(s)) = &v {
        if !app_core::ids::is_uuid(s) {
            return Err(AppError::bad_request(format!("{key} must be a UUID")));
        }
    }
    Ok(v)
}

/// A money (or quantity) number with at most two decimals, not negative; null clears it.
fn amount(m: &Map<String, Value>, key: &str) -> AppResult<Option<Option<Money>>> {
    match m.get(key) {
        None => Ok(None),
        Some(Value::Null) => Ok(Some(None)),
        Some(v) => match Money::from_json(v) {
            Some(x) if x >= Money::ZERO => Ok(Some(Some(x))),
            _ => {
                Err(AppError::bad_request(format!("{key} must be a number of 0 or more with at most 2 decimal places")))
            }
        },
    }
}

fn fields(m: &Map<String, Value>) -> AppResult<ItemFields> {
    let item_type = match text(m, "type")? {
        Some(Some(t)) if stock::TYPES.contains(&t.as_str()) => Some(t),
        None => None,
        _ => return Err(AppError::bad_request("type must be one of INVENTORY, SERVICE, NON_INVENTORY")),
    };
    let is_active = match m.get("isActive") {
        None => None,
        Some(Value::Bool(b)) => Some(*b),
        Some(_) => return Err(AppError::bad_request("isActive must be a boolean value")),
    };
    Ok(ItemFields {
        sku: text(m, "sku")?,
        name: text(m, "name")?.map(Option::unwrap_or_default),
        item_type,
        description: text(m, "description")?,
        sale_price: amount(m, "salePrice")?,
        purchase_cost: amount(m, "purchaseCost")?,
        income_account_id: id(m, "incomeAccountId")?,
        expense_account_id: id(m, "expenseAccountId")?,
        inventory_account_id: id(m, "inventoryAccountId")?,
        cogs_account_id: id(m, "cogsAccountId")?,
        tax_code_id: id(m, "taxCodeId")?,
        reorder_point: amount(m, "reorderPoint")?,
        is_active,
    })
}

pub async fn list(State(state): State<AppState>, _user: CurrentUser) -> AppResult<Json<Value>> {
    let items = stock::find_all(&state.db).await?;
    Ok(Json(Value::Array(items.iter().map(stock::Item::to_json).collect())))
}

pub async fn get(
    State(state): State<AppState>,
    _user: CurrentUser,
    Path(item): Path<String>,
) -> AppResult<Json<Value>> {
    parse_uuid(&item)?;
    let mut conn = state.db.read().acquire().await?;
    Ok(Json(stock::find_one(&mut conn, &item).await?.to_json()))
}

pub async fn create(
    State(state): State<AppState>,
    user: CurrentUser,
    body: JsonBody,
) -> AppResult<(StatusCode, Json<Value>)> {
    may_edit(&user)?;
    let f = fields(&body.object())?;
    if f.name.as_deref().is_none_or(str::is_empty) {
        return Err(AppError::bad_request("Name the product or service"));
    }
    Ok((StatusCode::CREATED, Json(stock::create(&state.db, f).await?.to_json())))
}

pub async fn update(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(item): Path<String>,
    body: JsonBody,
) -> AppResult<Json<Value>> {
    may_edit(&user)?;
    parse_uuid(&item)?;
    let f = fields(&body.object())?;
    Ok(Json(stock::update(&state.db, &item, f).await?.to_json()))
}

pub async fn remove(State(state): State<AppState>, user: CurrentUser, Path(item): Path<String>) -> AppResult<()> {
    may_edit(&user)?;
    parse_uuid(&item)?;
    stock::remove(&state.db, &item).await
}

pub async fn moves(
    State(state): State<AppState>,
    _user: CurrentUser,
    Path(item): Path<String>,
) -> AppResult<Json<Value>> {
    parse_uuid(&item)?;
    Ok(Json(stock::moves(&state.db, &item).await?))
}

pub async fn adjust(
    State(state): State<AppState>,
    _auth: Authorized<perm::Ledger>,
    Path(item): Path<String>,
    body: JsonBody,
) -> AppResult<Json<Value>> {
    parse_uuid(&item)?;
    let m = body.object();
    let date = match m.get("date").and_then(Value::as_str) {
        Some(d) if chrono::NaiveDate::parse_from_str(d, "%Y-%m-%d").is_ok() => d.to_string(),
        _ => return Err(AppError::bad_request("date must be a valid date (YYYY-MM-DD)")),
    };
    let quantity = m.get("quantity").and_then(Money::from_json).filter(|q| *q != Money::ZERO).ok_or_else(|| {
        AppError::bad_request("quantity must be a number other than 0, with at most 2 decimal places")
    })?;
    let unit_cost = amount(&m, "unitCost")?.flatten();
    let account_id = id(&m, "accountId")?
        .flatten()
        .ok_or_else(|| AppError::bad_request("Choose the account for the other side of the adjustment"))?;
    let note = text(&m, "note")?.flatten();
    let out = stock::adjust(&state.db, &item, Adjustment { date, quantity, unit_cost, account_id, note }).await?;
    Ok(Json(out.to_json()))
}
