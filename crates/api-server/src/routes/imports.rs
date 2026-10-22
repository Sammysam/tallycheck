//! D37: import from file and the bank transactions it brings in.
//!
//! An import needs the permission of what it creates (bank lines: `banking`; invoices:
//! `invoices`; customers / vendors: theirs). Reading a file and previewing change nothing and
//! are not written to the activity log.

use crate::body::JsonBody;
use crate::extract::{parse_uuid, perm, Authorized, CurrentUser};
use crate::state::AppState;
use crate::upload::UploadForm;
use app_core::{AppError, AppResult};
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use operations::imports::{self, DateOrder, Kind};
use operations::{bank_lines, sheet};
use serde_json::{json, Map, Value};
use std::collections::HashMap;

pub type SheetForm = UploadForm<{ sheet::MAX_FILE_BYTES }>;
pub const SHEET_BODY_LIMIT: usize = sheet::MAX_FILE_BYTES + 1024 * 1024;

const IMPORT_PERMISSIONS: &[&str] = &["banking", "invoices", "customers", "vendors"];

fn forbidden() -> AppError {
    AppError::Forbidden("Insufficient permissions".into())
}

fn may_import(user: &CurrentUser) -> AppResult<()> {
    if IMPORT_PERMISSIONS.iter().any(|p| user.user.has_permission(p)) {
        Ok(())
    } else {
        Err(forbidden())
    }
}

fn text(m: &Map<String, Value>, key: &str) -> Option<String> {
    match m.get(key) {
        Some(Value::String(s)) if !s.trim().is_empty() => Some(s.trim().to_string()),
        _ => None,
    }
}

/// Reads an uploaded CSV / Excel file and returns its sheets as text rows.
pub async fn read(user: CurrentUser, form: SheetForm) -> AppResult<Json<Value>> {
    may_import(&user)?;
    let file = form.file.ok_or_else(|| AppError::bad_request("Choose a file"))?;
    let sheets = tokio::task::spawn_blocking(move || {
        sheet::read(&file.original_name, &file.bytes).map(|s| (file.original_name, s))
    })
    .await
    .map_err(AppError::internal)??;
    Ok(Json(json!({ "fileName": sheets.0, "sheets": sheets.1 })))
}

fn request(user: &CurrentUser, m: &Map<String, Value>) -> AppResult<imports::Request> {
    let kind = text(m, "kind")
        .and_then(|k| Kind::parse(&k))
        .ok_or_else(|| AppError::bad_request("kind must be one of BANK, INVOICES, CUSTOMERS, VENDORS"))?;
    if !user.user.has_permission(kind.permission()) {
        return Err(forbidden());
    }
    let date_order = match text(m, "dateOrder") {
        None => DateOrder::Dmy,
        Some(d) => DateOrder::parse(&d).ok_or_else(|| AppError::bad_request("dateOrder must be DMY, MDY or YMD"))?,
    };
    let rows = m
        .get("rows")
        .and_then(Value::as_array)
        .ok_or_else(|| AppError::bad_request("rows must be an array"))?
        .iter()
        .map(|r| {
            let obj = r.as_object().ok_or_else(|| AppError::bad_request("each row must be an object"))?;
            Ok(obj
                .iter()
                .map(|(k, v)| {
                    let s = match v {
                        Value::String(s) => s.clone(),
                        Value::Null => String::new(),
                        other => other.to_string(),
                    };
                    (k.clone(), s)
                })
                .collect::<HashMap<_, _>>())
        })
        .collect::<AppResult<Vec<_>>>()?;
    Ok(imports::Request {
        kind,
        source: text(m, "source").unwrap_or_default(),
        file_name: text(m, "fileName"),
        account_id: text(m, "accountId"),
        default_account_id: text(m, "defaultAccountId"),
        date_order,
        rows,
    })
}

/// What an import would do, row by row — nothing is saved.
pub async fn preview(
    State(state): State<AppState>,
    user: CurrentUser,
    body: JsonBody,
) -> AppResult<Json<imports::Outcome>> {
    let req = request(&user, &body.object())?;
    Ok(Json(imports::run(&state.db, req, Some(&user.user.id), false).await?))
}

pub async fn run(
    State(state): State<AppState>,
    user: CurrentUser,
    body: JsonBody,
) -> AppResult<(StatusCode, Json<imports::Outcome>)> {
    let req = request(&user, &body.object())?;
    let out = imports::run(&state.db, req, Some(&user.user.id), true).await?;
    Ok((StatusCode::CREATED, Json(out)))
}

pub async fn list(State(state): State<AppState>, user: CurrentUser) -> AppResult<Json<Vec<imports::ImportRecord>>> {
    may_import(&user)?;
    Ok(Json(imports::list(&state.db).await?))
}

/// Undoes an import (DELETE): what it created is removed if nothing has used it yet.
pub async fn undo(
    State(state): State<AppState>,
    user: CurrentUser,
    Path(id): Path<String>,
) -> AppResult<Json<imports::ImportRecord>> {
    parse_uuid(&id)?;
    let imp = {
        let mut conn = state.db.read().acquire().await?;
        imports::find_one(&mut conn, &id).await?
    };
    let kind = Kind::parse(&imp.kind).ok_or_else(|| AppError::internal("stored import kind"))?;
    if !user.user.has_permission(kind.permission()) {
        return Err(forbidden());
    }
    Ok(Json(imports::undo(&state.db, &id).await?))
}

pub async fn layouts(
    State(state): State<AppState>,
    user: CurrentUser,
    Query(q): Query<HashMap<String, String>>,
) -> AppResult<Json<Vec<imports::Layout>>> {
    may_import(&user)?;
    Ok(Json(imports::layouts(&state.db, q.get("kind").map(String::as_str).filter(|k| !k.is_empty())).await?))
}

pub async fn layout_save(
    State(state): State<AppState>,
    user: CurrentUser,
    body: JsonBody,
) -> AppResult<Json<imports::Layout>> {
    may_import(&user)?;
    let m = body.object();
    let kind = text(&m, "kind")
        .filter(|k| imports::LAYOUT_KINDS.contains(&k.as_str()))
        .ok_or_else(|| AppError::bad_request("kind must be one of BANK, INVOICES, CUSTOMERS, VENDORS, OPENING"))?;
    let name = text(&m, "name").ok_or_else(|| AppError::bad_request("Name the layout"))?;
    if name.chars().count() > 100 {
        return Err(AppError::bad_request("The layout name is too long"));
    }
    let layout =
        m.get("layout").filter(|v| v.is_object()).ok_or_else(|| AppError::bad_request("layout must be an object"))?;
    Ok(Json(imports::save_layout(&state.db, &kind, &name, layout).await?))
}

pub async fn layout_remove(State(state): State<AppState>, user: CurrentUser, Path(id): Path<String>) -> AppResult<()> {
    may_import(&user)?;
    parse_uuid(&id)?;
    imports::delete_layout(&state.db, &id).await
}

// ─── Bank transactions ──────────────────────────────────────────────────────

pub async fn bank_list(
    State(state): State<AppState>,
    _auth: Authorized<perm::Banking>,
    Query(q): Query<HashMap<String, String>>,
) -> AppResult<Json<Value>> {
    let account =
        q.get("accountId").filter(|a| !a.is_empty()).ok_or_else(|| AppError::bad_request("Choose an account"))?;
    parse_uuid(account)?;
    let status = q.get("status").map(String::as_str).filter(|s| !s.is_empty());
    if let Some(s) = status {
        if !["UNMATCHED", "MATCHED", "IGNORED"].contains(&s) {
            return Err(AppError::bad_request("status must be UNMATCHED, MATCHED or IGNORED"));
        }
    }
    Ok(Json(bank_lines::list(&state.db, account, status).await?))
}

pub async fn bank_accounts(State(state): State<AppState>, _auth: Authorized<perm::Banking>) -> AppResult<Json<Value>> {
    Ok(Json(bank_lines::accounts(&state.db).await?))
}

pub async fn bank_documents(
    State(state): State<AppState>,
    _auth: Authorized<perm::Banking>,
    Path(id): Path<String>,
) -> AppResult<Json<Value>> {
    parse_uuid(&id)?;
    Ok(Json(bank_lines::documents(&state.db, &id).await?))
}

pub async fn bank_match(
    State(state): State<AppState>,
    _auth: Authorized<perm::Banking>,
    Path(id): Path<String>,
    body: JsonBody,
) -> AppResult<Json<Value>> {
    parse_uuid(&id)?;
    let m = body.object();
    let t = text(&m, "type").ok_or_else(|| AppError::bad_request("type must be PAYMENT or EXPENSE"))?;
    let record = text(&m, "id").ok_or_else(|| AppError::bad_request("Choose the payment or expense"))?;
    parse_uuid(&record)?;
    Ok(Json(bank_lines::match_record(&state.db, &id, &t, &record).await?))
}

pub async fn bank_pay(
    State(state): State<AppState>,
    auth: Authorized<perm::Banking>,
    Path(id): Path<String>,
    body: JsonBody,
) -> AppResult<Json<Value>> {
    if !auth.user().has_permission("payments") {
        return Err(forbidden());
    }
    parse_uuid(&id)?;
    let m = body.object();
    let (invoice_id, bill_id) = (text(&m, "invoiceId"), text(&m, "billId"));
    for d in invoice_id.iter().chain(bill_id.iter()) {
        parse_uuid(d)?;
    }
    let method = text(&m, "paymentMethod").unwrap_or_else(|| "BANK_TRANSFER".into());
    if !["CASH", "BANK_TRANSFER", "CHECK"].contains(&method.as_str()) {
        return Err(AppError::bad_request("paymentMethod must be one of CASH, BANK_TRANSFER, CHECK"));
    }
    Ok(Json(bank_lines::pay(&state.db, &id, invoice_id, bill_id, method).await?))
}

pub async fn bank_expense(
    State(state): State<AppState>,
    auth: Authorized<perm::Banking>,
    Path(id): Path<String>,
    body: JsonBody,
) -> AppResult<Json<Value>> {
    if !auth.user().has_permission("expenses") {
        return Err(forbidden());
    }
    parse_uuid(&id)?;
    let m = body.object();
    let account = text(&m, "expenseAccountId").ok_or_else(|| AppError::bad_request("Choose the expense account"))?;
    parse_uuid(&account)?;
    let tax = text(&m, "taxCodeId");
    if let Some(t) = &tax {
        parse_uuid(t)?;
    }
    Ok(Json(bank_lines::expense(&state.db, &id, account, tax, text(&m, "description")).await?))
}

/// D39: the line is a transfer to / from another of the company's accounts.
pub async fn bank_transfer(
    State(state): State<AppState>,
    auth: Authorized<perm::Banking>,
    Path(id): Path<String>,
    body: JsonBody,
) -> AppResult<Json<Value>> {
    if !auth.user().has_permission("ledger") {
        return Err(forbidden());
    }
    parse_uuid(&id)?;
    let m = body.object();
    let account = text(&m, "accountId").ok_or_else(|| AppError::bad_request("Choose the other account"))?;
    parse_uuid(&account)?;
    Ok(Json(bank_lines::transfer(&state.db, &id, &account, text(&m, "note")).await?))
}

async fn change(state: AppState, id: String, action: &str) -> AppResult<Json<Value>> {
    parse_uuid(&id)?;
    Ok(Json(bank_lines::change(&state.db, &id, action).await?))
}

pub async fn bank_ignore(
    State(s): State<AppState>,
    _a: Authorized<perm::Banking>,
    Path(id): Path<String>,
) -> AppResult<Json<Value>> {
    change(s, id, "IGNORE").await
}

pub async fn bank_restore(
    State(s): State<AppState>,
    _a: Authorized<perm::Banking>,
    Path(id): Path<String>,
) -> AppResult<Json<Value>> {
    change(s, id, "RESTORE").await
}

pub async fn bank_unmatch(
    State(s): State<AppState>,
    _a: Authorized<perm::Banking>,
    Path(id): Path<String>,
) -> AppResult<Json<Value>> {
    change(s, id, "UNMATCH").await
}
