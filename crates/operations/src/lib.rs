//! Operational accounting (reference W2 modules): customers, vendors, bank accounts,
//! invoices, bills, expenses and payments.
//!
//! Every operation that touches money runs in one write transaction together with its ledger
//! posting, so a document and its journal entry are saved or rolled back together. Status
//! checks happen inside that transaction, so a retried request finds the document already
//! posted / paid / voided and is refused instead of posting twice.
//!
//! JSON shapes follow the reference exactly, including TypeORM's quirks: a freshly inserted
//! row echoes the numbers the client sent (JSON numbers) except columns with a database
//! default (strings), while a row read back has every `numeric` column as a 2-dp string.

pub mod banking;
pub mod bills;
pub mod credit_notes;
pub mod expenses;
pub mod invoices;
pub mod parties;
pub mod payments;
pub mod statements;
pub mod tax;

use app_core::money::Money;
use app_core::{AppError, AppResult};
use serde_json::Value;
use sqlx::SqliteConnection;

pub(crate) fn money_str(m: Money) -> Value {
    Value::String(m.to_db_string())
}

pub(crate) fn money_num(m: Money) -> Value {
    m.to_json_number()
}

pub(crate) fn opt(v: &Option<String>) -> Value {
    v.clone().map(Value::String).unwrap_or(Value::Null)
}

/// Fails with the reference's `findOne` message unless the account exists. The reference let
/// the database reject a dangling account id (a 500); here the caller gets a 404.
pub(crate) async fn require_account(conn: &mut SqliteConnection, id: &str) -> AppResult<()> {
    ledger::accounts::find_one(conn, id).await.map(|_| ())
}

pub(crate) fn overflow() -> AppError {
    AppError::bad_request("Amount is too large")
}

#[cfg(test)]
mod tests;
