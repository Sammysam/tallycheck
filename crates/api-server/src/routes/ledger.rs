//! Reference `ledger.controller.ts` — every route needs the `ledger` permission.

use crate::body::JsonBody;
use crate::extract::{perm, Authorized};
use crate::state::AppState;
use crate::validate;
use app_core::time::today_local;
use app_core::{AppError, AppResult};
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use ledger::entries::{self, EntryFilter, JournalEntry, Totals, TrialBalanceRow};
use ledger::{NewEntry, NewLine};
use serde_json::{json, Value};

pub async fn post_entry(
    State(state): State<AppState>,
    _auth: Authorized<perm::Ledger>,
    body: JsonBody,
) -> AppResult<(StatusCode, Json<JournalEntry>)> {
    let dto = validate::create_journal_entry(body.object())?;
    let mut tx = state.db.begin().await?;
    let entry = entries::post(
        &mut tx,
        NewEntry {
            date: dto.date,
            description: dto.description,
            reference: dto.reference,
            source_type: dto.source_type,
            source_id: dto.source_id,
            lines: dto
                .lines
                .into_iter()
                .map(|l| NewLine {
                    account_id: l.account_id,
                    debit: l.debit,
                    credit: l.credit,
                    description: l.description,
                })
                .collect(),
        },
    )
    .await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(entry)))
}

pub async fn list_entries(
    State(state): State<AppState>,
    _auth: Authorized<perm::Ledger>,
    Query(pairs): Query<Vec<(String, String)>>,
) -> AppResult<Json<Vec<JournalEntry>>> {
    let f = validate::ledger_filter(validate::query_map(pairs))?;
    let mut conn = state.db.read().acquire().await?;
    let filter = EntryFilter { source_type: f.source_type, date_from: f.date_from, date_to: f.date_to };
    Ok(Json(entries::list(&mut conn, &filter).await?))
}

pub async fn get_entry(
    State(state): State<AppState>,
    _auth: Authorized<perm::Ledger>,
    Path(id): Path<String>,
) -> AppResult<Json<JournalEntry>> {
    let mut conn = state.db.read().acquire().await?;
    Ok(Json(entries::get(&mut conn, &id).await?))
}

/// Which screen owns an entry posted by a business document.
fn document_name(source_type: &str) -> Option<&'static str> {
    Some(match source_type {
        "INVOICE" => "an invoice",
        "BILL" => "a bill",
        "EXPENSE" => "an expense",
        "PAYMENT_AR" | "PAYMENT_AP" => "a payment",
        "PAYROLL" | "PAYROLL_PAYMENT" | "PAYROLL_REMITTANCE" => "a payroll run",
        _ => return None,
    })
}

/// Dated with the Sri Lanka business date (D5); the reference used the UTC date.
pub async fn reverse(
    State(state): State<AppState>,
    _auth: Authorized<perm::Ledger>,
    Path(id): Path<String>,
) -> AppResult<(StatusCode, Json<JournalEntry>)> {
    let mut tx = state.db.begin().await?;
    // D18: an entry an invoice, bill, expense, payment or payroll run posted is undone from
    // that document, which also fixes the document's status and amounts. Reversing it here
    // would leave, say, an invoice marked PAID with its payment gone from the ledger.
    let entry = entries::get(&mut tx, &id).await?;
    if let (Some(kind), Some(_)) = (entry.source_type.as_deref(), entry.source_id.as_deref()) {
        if let Some(doc) = document_name(kind) {
            return Err(AppError::bad_request(format!(
                "This entry was posted by {doc}. Void or unpost it from that screen instead."
            )));
        }
    }
    let reversal = entries::reverse(&mut tx, &id, &today_local().to_string()).await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(reversal)))
}

pub async fn trial_balance(
    State(state): State<AppState>,
    _auth: Authorized<perm::Ledger>,
    Query(pairs): Query<Vec<(String, String)>>,
) -> AppResult<Json<Vec<TrialBalanceRow>>> {
    let as_of = validate::optional_query_date(&validate::query_map(pairs), "asOf")?;
    let mut conn = state.db.read().acquire().await?;
    Ok(Json(entries::trial_balance(&mut conn, as_of.as_deref()).await?))
}

pub async fn account_balance(
    State(state): State<AppState>,
    _auth: Authorized<perm::Ledger>,
    Path(account_id): Path<String>,
    Query(pairs): Query<Vec<(String, String)>>,
) -> AppResult<Json<Value>> {
    let q = validate::query_map(pairs);
    let from = validate::optional_query_date(&q, "dateFrom")?;
    let to = validate::optional_query_date(&q, "dateTo")?;
    let mut conn = state.db.read().acquire().await?;
    let t: Totals = entries::account_balance(&mut conn, &account_id, from.as_deref(), to.as_deref()).await?;
    Ok(Json(json!({
        "accountId": account_id,
        "debit": t.debit.to_json_number(),
        "credit": t.credit.to_json_number(),
        "balance": t.balance.to_json_number(),
    })))
}
