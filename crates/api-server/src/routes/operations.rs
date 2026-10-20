//! W2 routes: customers, vendors, banking, invoices, bills, expenses, payments
//! (reference controllers of the same names).

use crate::body::JsonBody;
use crate::extract::{perm, Authorized, CurrentUser};
use crate::state::AppState;
use crate::validate::{self, ops};
use app_core::time::today_local;
use app_core::AppResult;
use auth::User;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use operations::invoices::{self, Scope, View};
use operations::parties::{self, Party, PartyKind};
use operations::{banking, bills, credit_notes, expenses, payments};
use serde_json::{json, Value};

type Pairs = Query<Vec<(String, String)>>;

fn paginated(data: Vec<Value>, total: i64, page: i64, limit: i64) -> Json<Value> {
    let total_pages = if limit > 0 { (total + limit - 1) / limit } else { 0 };
    Json(json!({ "data": data, "total": total, "page": page, "limit": limit, "totalPages": total_pages }))
}

fn today() -> String {
    today_local().to_string()
}

// ─── Customers (reading open to every signed-in user) ───────────────────────

pub async fn customers_list(State(s): State<AppState>, _u: CurrentUser) -> AppResult<Json<Vec<Party>>> {
    Ok(Json(parties::find_all(&s.db, PartyKind::Customer).await?))
}

pub async fn customers_get(
    State(s): State<AppState>,
    _u: CurrentUser,
    Path(id): Path<String>,
) -> AppResult<Json<Party>> {
    let mut c = s.db.read().acquire().await?;
    Ok(Json(parties::find_one(&mut c, PartyKind::Customer, &id).await?))
}

pub async fn customers_create(
    State(s): State<AppState>,
    _a: Authorized<perm::Customers>,
    body: JsonBody,
) -> AppResult<(StatusCode, Json<Party>)> {
    let f = ops::party(body.object(), false)?;
    Ok((StatusCode::CREATED, Json(parties::create(&s.db, PartyKind::Customer, f).await?)))
}

pub async fn customers_update(
    State(s): State<AppState>,
    _a: Authorized<perm::Customers>,
    Path(id): Path<String>,
    body: JsonBody,
) -> AppResult<Json<Party>> {
    let f = ops::party(body.object(), true)?;
    Ok(Json(parties::update(&s.db, PartyKind::Customer, &id, f).await?))
}

pub async fn customers_remove(
    State(s): State<AppState>,
    _a: Authorized<perm::Customers>,
    Path(id): Path<String>,
) -> AppResult<StatusCode> {
    parties::remove(&s.db, PartyKind::Customer, &id).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ─── Vendors (all routes need `vendors`) ────────────────────────────────────

pub async fn vendors_list(State(s): State<AppState>, _a: Authorized<perm::Vendors>) -> AppResult<Json<Vec<Party>>> {
    Ok(Json(parties::find_all(&s.db, PartyKind::Vendor).await?))
}

pub async fn vendors_get(
    State(s): State<AppState>,
    _a: Authorized<perm::Vendors>,
    Path(id): Path<String>,
) -> AppResult<Json<Party>> {
    let mut c = s.db.read().acquire().await?;
    Ok(Json(parties::find_one(&mut c, PartyKind::Vendor, &id).await?))
}

pub async fn vendors_create(
    State(s): State<AppState>,
    _a: Authorized<perm::Vendors>,
    body: JsonBody,
) -> AppResult<(StatusCode, Json<Party>)> {
    let f = ops::party(body.object(), false)?;
    Ok((StatusCode::CREATED, Json(parties::create(&s.db, PartyKind::Vendor, f).await?)))
}

pub async fn vendors_update(
    State(s): State<AppState>,
    _a: Authorized<perm::Vendors>,
    Path(id): Path<String>,
    body: JsonBody,
) -> AppResult<Json<Party>> {
    let f = ops::party(body.object(), true)?;
    Ok(Json(parties::update(&s.db, PartyKind::Vendor, &id, f).await?))
}

pub async fn vendors_remove(
    State(s): State<AppState>,
    _a: Authorized<perm::Vendors>,
    Path(id): Path<String>,
) -> AppResult<StatusCode> {
    parties::remove(&s.db, PartyKind::Vendor, &id).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ─── Banking ────────────────────────────────────────────────────────────────

pub async fn banking_list(
    State(s): State<AppState>,
    _a: Authorized<perm::Banking>,
) -> AppResult<Json<Vec<banking::BankAccount>>> {
    Ok(Json(banking::find_all(&s.db).await?))
}

pub async fn banking_get(
    State(s): State<AppState>,
    _a: Authorized<perm::Banking>,
    Path(id): Path<String>,
) -> AppResult<Json<banking::BankAccount>> {
    let mut c = s.db.read().acquire().await?;
    Ok(Json(banking::find_one(&mut c, &id).await?))
}

pub async fn banking_create(
    State(s): State<AppState>,
    _a: Authorized<perm::Banking>,
    body: JsonBody,
) -> AppResult<(StatusCode, Json<banking::BankAccount>)> {
    let f = ops::bank(body.object(), false)?;
    Ok((StatusCode::CREATED, Json(banking::create(&s.db, f).await?)))
}

pub async fn banking_update(
    State(s): State<AppState>,
    _a: Authorized<perm::Banking>,
    Path(id): Path<String>,
    body: JsonBody,
) -> AppResult<Json<banking::BankAccount>> {
    let f = ops::bank(body.object(), true)?;
    Ok(Json(banking::update(&s.db, &id, f).await?))
}

pub async fn banking_deactivate(
    State(s): State<AppState>,
    _a: Authorized<perm::Banking>,
    Path(id): Path<String>,
) -> AppResult<StatusCode> {
    banking::deactivate(&s.db, &id).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ─── Invoices (`invoices`; only Admin / `invoices:all` see other people's) ──

fn scope(user: &User) -> Scope {
    Scope {
        user_id: Some(user.id.clone()),
        sees_all: user.is_admin() || user.role.as_ref().is_some_and(|r| r.grants("invoices:all")),
    }
}

pub async fn invoices_create(
    State(s): State<AppState>,
    a: Authorized<perm::Invoices>,
    body: JsonBody,
) -> AppResult<(StatusCode, Json<Value>)> {
    let dto = ops::create_invoice(body.object())?;
    let inv = invoices::create(&s.db, dto, Some(&a.user().id)).await?;
    Ok((StatusCode::CREATED, Json(inv.to_json(View::Created))))
}

pub async fn invoices_list(
    State(s): State<AppState>,
    a: Authorized<perm::Invoices>,
    Query(p): Pairs,
) -> AppResult<Json<Value>> {
    let q = ops::invoice_query(validate::query_map(p))?;
    let (rows, total) = invoices::find_all(&s.db, &q, &scope(a.user())).await?;
    Ok(paginated(rows.iter().map(|i| i.to_json(View::Read)).collect(), total, q.page, q.limit))
}

pub async fn invoices_get(
    State(s): State<AppState>,
    a: Authorized<perm::Invoices>,
    Path(id): Path<String>,
) -> AppResult<Json<Value>> {
    let mut c = s.db.read().acquire().await?;
    Ok(Json(invoices::find_one(&mut c, &id, &scope(a.user())).await?.to_json(View::Read)))
}

pub async fn invoices_update(
    State(s): State<AppState>,
    a: Authorized<perm::Invoices>,
    Path(id): Path<String>,
    body: JsonBody,
) -> AppResult<Json<Value>> {
    let patch = ops::update_invoice(body.object())?;
    let (inv, view) = invoices::update(&s.db, &id, patch, &scope(a.user())).await?;
    Ok(Json(inv.to_json(view)))
}

pub async fn invoices_remove(
    State(s): State<AppState>,
    a: Authorized<perm::Invoices>,
    Path(id): Path<String>,
) -> AppResult<StatusCode> {
    invoices::remove(&s.db, &id, &scope(a.user())).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn invoices_duplicate(
    State(s): State<AppState>,
    a: Authorized<perm::Invoices>,
    Path(id): Path<String>,
) -> AppResult<(StatusCode, Json<Value>)> {
    let inv = invoices::duplicate(&s.db, &id, &scope(a.user()), &today()).await?;
    Ok((StatusCode::CREATED, Json(inv.to_json(View::Created))))
}

pub async fn invoices_status(
    State(s): State<AppState>,
    a: Authorized<perm::Invoices>,
    Path(id): Path<String>,
    body: JsonBody,
) -> AppResult<Json<Value>> {
    let target = ops::change_status(body.object())?;
    let inv = invoices::change_status(&s.db, &id, &target, &scope(a.user()), &today()).await?;
    Ok(Json(inv.to_json(View::Read)))
}

pub async fn invoices_post(
    State(s): State<AppState>,
    a: Authorized<perm::Invoices>,
    Path(id): Path<String>,
) -> AppResult<(StatusCode, Json<Value>)> {
    let inv = invoices::post(&s.db, &id, &scope(a.user())).await?;
    Ok((StatusCode::CREATED, Json(inv.to_json(View::Read))))
}

pub async fn invoices_unpost(
    State(s): State<AppState>,
    a: Authorized<perm::Invoices>,
    Path(id): Path<String>,
) -> AppResult<(StatusCode, Json<Value>)> {
    let inv = invoices::unpost(&s.db, &id, &scope(a.user()), &today()).await?;
    Ok((StatusCode::CREATED, Json(inv.to_json(View::Read))))
}

pub async fn invoices_void(
    State(s): State<AppState>,
    a: Authorized<perm::Invoices>,
    Path(id): Path<String>,
) -> AppResult<(StatusCode, Json<Value>)> {
    let inv = invoices::void(&s.db, &id, &scope(a.user()), &today()).await?;
    Ok((StatusCode::CREATED, Json(inv.to_json(View::Read))))
}

// ─── Bills ──────────────────────────────────────────────────────────────────

pub async fn bills_create(
    State(s): State<AppState>,
    _a: Authorized<perm::Bills>,
    body: JsonBody,
) -> AppResult<(StatusCode, Json<Value>)> {
    let dto = ops::create_bill(body.object())?;
    Ok((StatusCode::CREATED, Json(bills::create(&s.db, dto).await?.to_json(true))))
}

pub async fn bills_list(
    State(s): State<AppState>,
    _a: Authorized<perm::Bills>,
    Query(p): Pairs,
) -> AppResult<Json<Value>> {
    let (page, limit) = validate::pagination(&validate::query_map(p))?;
    let (rows, total) = bills::find_all(&s.db, page, limit).await?;
    Ok(paginated(rows.iter().map(|b| b.to_json(false)).collect(), total, page, limit))
}

pub async fn bills_get(
    State(s): State<AppState>,
    _a: Authorized<perm::Bills>,
    Path(id): Path<String>,
) -> AppResult<Json<Value>> {
    let mut c = s.db.read().acquire().await?;
    Ok(Json(bills::find_one(&mut c, &id).await?.to_json(false)))
}

pub async fn bills_post(
    State(s): State<AppState>,
    _a: Authorized<perm::Bills>,
    Path(id): Path<String>,
) -> AppResult<(StatusCode, Json<Value>)> {
    Ok((StatusCode::CREATED, Json(bills::post(&s.db, &id).await?.to_json(false))))
}

pub async fn bills_void(
    State(s): State<AppState>,
    _a: Authorized<perm::Bills>,
    Path(id): Path<String>,
) -> AppResult<(StatusCode, Json<Value>)> {
    Ok((StatusCode::CREATED, Json(bills::void(&s.db, &id, &today()).await?.to_json(false))))
}

// ─── Expenses ───────────────────────────────────────────────────────────────

pub async fn expenses_create(
    State(s): State<AppState>,
    _a: Authorized<perm::Expenses>,
    body: JsonBody,
) -> AppResult<(StatusCode, Json<Value>)> {
    let dto = ops::create_expense(body.object())?;
    Ok((StatusCode::CREATED, Json(expenses::create(&s.db, dto).await?.to_json(true))))
}

pub async fn expenses_list(
    State(s): State<AppState>,
    _a: Authorized<perm::Expenses>,
    Query(p): Pairs,
) -> AppResult<Json<Value>> {
    let (page, limit) = validate::pagination(&validate::query_map(p))?;
    let (rows, total) = expenses::find_all(&s.db, page, limit).await?;
    Ok(paginated(rows.iter().map(|e| e.to_json(false)).collect(), total, page, limit))
}

pub async fn expenses_get(
    State(s): State<AppState>,
    _a: Authorized<perm::Expenses>,
    Path(id): Path<String>,
) -> AppResult<Json<Value>> {
    let mut c = s.db.read().acquire().await?;
    Ok(Json(expenses::find_one(&mut c, &id).await?.to_json(false)))
}

pub async fn expenses_post(
    State(s): State<AppState>,
    _a: Authorized<perm::Expenses>,
    Path(id): Path<String>,
) -> AppResult<(StatusCode, Json<Value>)> {
    Ok((StatusCode::CREATED, Json(expenses::post(&s.db, &id).await?.to_json(false))))
}

pub async fn expenses_void(
    State(s): State<AppState>,
    _a: Authorized<perm::Expenses>,
    Path(id): Path<String>,
) -> AppResult<(StatusCode, Json<Value>)> {
    Ok((StatusCode::CREATED, Json(expenses::void(&s.db, &id, &today()).await?.to_json(false))))
}

// ─── Payments ───────────────────────────────────────────────────────────────

pub async fn payments_create(
    State(s): State<AppState>,
    _a: Authorized<perm::Payments>,
    body: JsonBody,
) -> AppResult<(StatusCode, Json<Value>)> {
    let dto = ops::create_payment(body.object())?;
    Ok((StatusCode::CREATED, Json(payments::create(&s.db, dto).await?.to_json(true))))
}

pub async fn payments_list(
    State(s): State<AppState>,
    _a: Authorized<perm::Payments>,
    Query(p): Pairs,
) -> AppResult<Json<Value>> {
    let (page, limit) = validate::pagination(&validate::query_map(p))?;
    let (rows, total) = payments::find_all(&s.db, page, limit).await?;
    Ok(paginated(rows.iter().map(|x| x.to_json(false)).collect(), total, page, limit))
}

pub async fn payments_get(
    State(s): State<AppState>,
    _a: Authorized<perm::Payments>,
    Path(id): Path<String>,
) -> AppResult<Json<Value>> {
    let mut c = s.db.read().acquire().await?;
    Ok(Json(payments::find_one(&mut c, &id).await?.to_json(false)))
}

/// D18: reverses the payment's entry and makes the amount outstanding again.
pub async fn payments_void(
    State(s): State<AppState>,
    _a: Authorized<perm::Payments>,
    Path(id): Path<String>,
) -> AppResult<(StatusCode, Json<Value>)> {
    let p = payments::void(&s.db, &id, &today()).await?;
    Ok((StatusCode::CREATED, Json(p.to_json(false))))
}

// ─── Credit notes (AR, `invoices` permission) and debit notes (AP, `bills`) — D21 ───────

async fn notes_list(s: &AppState, kind: &str, user: &User) -> AppResult<Json<Value>> {
    let rows = credit_notes::find_all(&s.db, kind, &scope(user)).await?;
    Ok(Json(Value::Array(rows.iter().map(|n| n.to_json()).collect())))
}

async fn notes_create(s: &AppState, kind: &str, user: &User, body: JsonBody) -> AppResult<(StatusCode, Json<Value>)> {
    let dto = ops::create_note(body.object(), kind)?;
    Ok((StatusCode::CREATED, Json(credit_notes::create(&s.db, dto, &scope(user)).await?.to_json())))
}

async fn notes_void(s: &AppState, kind: &str, user: &User, id: &str) -> AppResult<(StatusCode, Json<Value>)> {
    let note = credit_notes::void(&s.db, kind, id, &scope(user), &today()).await?;
    Ok((StatusCode::CREATED, Json(note.to_json())))
}

pub async fn credit_notes_list(State(s): State<AppState>, a: Authorized<perm::Invoices>) -> AppResult<Json<Value>> {
    notes_list(&s, credit_notes::AR, a.user()).await
}

pub async fn credit_notes_create(
    State(s): State<AppState>,
    a: Authorized<perm::Invoices>,
    body: JsonBody,
) -> AppResult<(StatusCode, Json<Value>)> {
    notes_create(&s, credit_notes::AR, a.user(), body).await
}

pub async fn credit_notes_void(
    State(s): State<AppState>,
    a: Authorized<perm::Invoices>,
    Path(id): Path<String>,
) -> AppResult<(StatusCode, Json<Value>)> {
    notes_void(&s, credit_notes::AR, a.user(), &id).await
}

pub async fn debit_notes_list(State(s): State<AppState>, a: Authorized<perm::Bills>) -> AppResult<Json<Value>> {
    notes_list(&s, credit_notes::AP, a.user()).await
}

pub async fn debit_notes_create(
    State(s): State<AppState>,
    a: Authorized<perm::Bills>,
    body: JsonBody,
) -> AppResult<(StatusCode, Json<Value>)> {
    notes_create(&s, credit_notes::AP, a.user(), body).await
}

pub async fn debit_notes_void(
    State(s): State<AppState>,
    a: Authorized<perm::Bills>,
    Path(id): Path<String>,
) -> AppResult<(StatusCode, Json<Value>)> {
    notes_void(&s, credit_notes::AP, a.user(), &id).await
}
