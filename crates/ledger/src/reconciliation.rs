//! Bank reconciliation (approved deviation D22, requested 2026-10-20; the reference had none).
//!
//! One asset account (cash or bank) is matched to a bank statement. A reconciliation starts
//! from the previous completed one's statement balance (the opening balance), the user ticks the
//! ledger lines the statement shows, and it can be completed only when
//! opening + ticked debits - ticked credits equals the statement's closing balance exactly.
//!
//! * Candidate lines: on the account, dated on or before the statement date, and not cleared
//!   by another reconciliation. Lines left unticked (outstanding cheques, deposits in transit)
//!   come up again next time.
//! * A voided document's entry and its reversal net to nothing and never reach the bank, so
//!   while neither is ticked the pair is left off the list.
//! * One reconciliation in progress per account; statement dates move forward; only the
//!   latest completed reconciliation can be undone (it goes back to in progress).
//! * Nothing here posts to the ledger. Bank charges and interest are entered as journal
//!   entries first, then ticked.

use crate::accounts::{self, AccountType};
use app_core::ids::new_id;
use app_core::money::Money;
use app_core::time::now_iso;
use app_core::{AppError, AppResult};
use database::{Db, Tx};
use serde_json::{json, Value};
use sqlx::sqlite::SqliteRow;
use sqlx::{Row, SqliteConnection};
use std::collections::{HashMap, HashSet};

pub const DRAFT: &str = "DRAFT";
pub const COMPLETED: &str = "COMPLETED";

#[derive(Debug, Clone)]
pub struct Reconciliation {
    pub id: String,
    pub account_id: String,
    pub statement_date: String,
    pub statement_balance: Money,
    pub opening_balance: Money,
    pub status: String,
    pub completed_at: Option<String>,
    pub completed_by: Option<String>,
    pub created_at: String,
}

impl Reconciliation {
    pub fn to_json(&self) -> Value {
        json!({
            "id": self.id,
            "accountId": self.account_id,
            "statementDate": self.statement_date,
            "statementBalance": self.statement_balance.to_json_number(),
            "openingBalance": self.opening_balance.to_json_number(),
            "status": self.status,
            "completedAt": self.completed_at,
            "completedBy": self.completed_by,
            "createdAt": self.created_at,
        })
    }
}

#[derive(Debug, Clone)]
pub struct RecLine {
    pub line_id: String,
    pub entry_id: String,
    pub entry_number: String,
    pub date: String,
    pub description: String,
    pub reference: Option<String>,
    pub debit: Money,
    pub credit: Money,
    pub cleared: bool,
}

/// A reconciliation with its lines and running totals, as the screen shows it.
#[derive(Debug, Clone)]
pub struct Workspace {
    pub rec: Reconciliation,
    pub lines: Vec<RecLine>,
    pub cleared_balance: Money,
    pub difference: Money,
}

impl Workspace {
    pub fn to_json(&self) -> Value {
        let lines: Vec<Value> = self
            .lines
            .iter()
            .map(|l| {
                json!({
                    "lineId": l.line_id,
                    "entryId": l.entry_id,
                    "entryNumber": l.entry_number,
                    "date": l.date,
                    "description": l.description,
                    "reference": l.reference,
                    "debit": l.debit.to_json_number(),
                    "credit": l.credit.to_json_number(),
                    "cleared": l.cleared,
                })
            })
            .collect();
        let mut v = self.rec.to_json();
        if let Value::Object(m) = &mut v {
            m.insert("lines".into(), Value::Array(lines));
            m.insert("clearedBalance".into(), self.cleared_balance.to_json_number());
            m.insert("difference".into(), self.difference.to_json_number());
        }
        v
    }
}

const SELECT: &str = r#"SELECT r."id", r."accountId", r."statementDate", r."statementBalance", r."openingBalance", r."status",
       r."completedAt", u."username" AS "completedBy", r."createdAt"
  FROM "bank_reconciliations" r LEFT JOIN "users" u ON u."id" = r."completedById""#;

fn from_row(r: &SqliteRow) -> AppResult<Reconciliation> {
    Ok(Reconciliation {
        id: r.try_get("id")?,
        account_id: r.try_get("accountId")?,
        statement_date: r.try_get("statementDate")?,
        statement_balance: Money::from_cents(r.try_get("statementBalance")?),
        opening_balance: Money::from_cents(r.try_get("openingBalance")?),
        status: r.try_get("status")?,
        completed_at: r.try_get("completedAt")?,
        completed_by: r.try_get("completedBy")?,
        created_at: r.try_get("createdAt")?,
    })
}

fn too_large() -> AppError {
    AppError::bad_request("Amount is too large")
}

async fn find(conn: &mut SqliteConnection, id: &str) -> AppResult<Reconciliation> {
    let row = sqlx::query(&format!(r#"{SELECT} WHERE r."id" = ?1"#)).bind(id).fetch_optional(&mut *conn).await?;
    match row {
        Some(r) => from_row(&r),
        None => Err(AppError::NotFound(format!("Reconciliation {id} not found"))),
    }
}

async fn last_completed(conn: &mut SqliteConnection, account_id: &str) -> AppResult<Option<Reconciliation>> {
    let row = sqlx::query(&format!(
        r#"{SELECT} WHERE r."accountId" = ?1 AND r."status" = 'COMPLETED' ORDER BY r."statementDate" DESC, r.rowid DESC LIMIT 1"#
    ))
    .bind(account_id)
    .fetch_optional(&mut *conn)
    .await?;
    row.as_ref().map(from_row).transpose()
}

/// Every reconciliation of an account, newest statement first.
pub async fn list(conn: &mut SqliteConnection, account_id: &str) -> AppResult<Vec<Reconciliation>> {
    let rows =
        sqlx::query(&format!(r#"{SELECT} WHERE r."accountId" = ?1 ORDER BY r."statementDate" DESC, r.rowid DESC"#))
            .bind(account_id)
            .fetch_all(&mut *conn)
            .await?;
    rows.iter().map(from_row).collect()
}

/// Starts reconciling `account_id` against a statement ending `statement_date` with
/// `statement_balance`.
pub async fn start(
    db: &Db,
    account_id: &str,
    statement_date: &str,
    statement_balance: Money,
    user_id: Option<&str>,
) -> AppResult<Workspace> {
    let mut tx = db.begin().await?;
    let account = accounts::find_one(&mut tx, account_id).await?;
    if account.account_type != AccountType::Asset || !account.is_active {
        return Err(AppError::bad_request("Only an active asset account (cash or bank) can be reconciled"));
    }
    let open: Option<String> =
        sqlx::query_scalar(r#"SELECT "id" FROM "bank_reconciliations" WHERE "accountId" = ?1 AND "status" = 'DRAFT'"#)
            .bind(account_id)
            .fetch_optional(&mut *tx)
            .await?;
    if open.is_some() {
        return Err(AppError::bad_request(
            "A reconciliation of this account is already in progress — finish or discard it first",
        ));
    }
    let previous = last_completed(&mut tx, account_id).await?;
    if let Some(p) = &previous {
        if statement_date <= p.statement_date.as_str() {
            return Err(AppError::bad_request(format!(
                "The statement date must be after the last reconciliation ({})",
                p.statement_date
            )));
        }
    }
    let id = new_id();
    let now = now_iso();
    sqlx::query(
        r#"INSERT INTO "bank_reconciliations" ("id","accountId","statementDate","statementBalance","openingBalance","status",
             "createdById","createdAt","updatedAt") VALUES (?1,?2,?3,?4,?5,'DRAFT',?6,?7,?7)"#,
    )
    .bind(&id)
    .bind(account_id)
    .bind(statement_date)
    .bind(statement_balance.cents())
    .bind(previous.map_or(Money::ZERO, |p| p.statement_balance).cents())
    .bind(user_id)
    .bind(&now)
    .execute(&mut *tx)
    .await?;
    let ws = workspace(&mut tx, &id).await?;
    tx.commit().await?;
    Ok(ws)
}

/// The lines and totals. In progress: every candidate line, ticked or not. Completed: the
/// lines it cleared.
pub async fn workspace(conn: &mut SqliteConnection, id: &str) -> AppResult<Workspace> {
    let rec = find(conn, id).await?;
    let rows = sqlx::query(
        r#"SELECT l."id" AS "lineId", e."id" AS "entryId", e."entryNumber", e."date", e."description", e."reference",
                  e."status" AS "entryStatus", e."reversalOfId", l."debit", l."credit", rl."reconciliationId"
           FROM "journal_lines" l
           JOIN "journal_entries" e ON e."id" = l."journalEntryId"
           LEFT JOIN "reconciled_lines" rl ON rl."journalLineId" = l."id"
           WHERE l."accountId" = ?1 AND e."date" <= ?2
             AND (rl."reconciliationId" = ?3 OR (rl."reconciliationId" IS NULL AND ?4 = 'DRAFT'))
           ORDER BY e."date", e."entryNumber", l.rowid"#,
    )
    .bind(&rec.account_id)
    .bind(&rec.statement_date)
    .bind(&rec.id)
    .bind(&rec.status)
    .fetch_all(&mut *conn)
    .await?;

    struct Raw {
        line: RecLine,
        reversed: bool,
        reversal_of: Option<String>,
    }
    let raw = rows
        .iter()
        .map(|r| -> AppResult<Raw> {
            let cleared: Option<String> = r.try_get("reconciliationId")?;
            let status: String = r.try_get("entryStatus")?;
            Ok(Raw {
                line: RecLine {
                    line_id: r.try_get("lineId")?,
                    entry_id: r.try_get("entryId")?,
                    entry_number: r.try_get("entryNumber")?,
                    date: r.try_get("date")?,
                    description: r.try_get("description")?,
                    reference: r.try_get("reference")?,
                    debit: Money::from_cents(r.try_get("debit")?),
                    credit: Money::from_cents(r.try_get("credit")?),
                    cleared: cleared.is_some(),
                },
                reversed: status == "REVERSED",
                reversal_of: r.try_get("reversalOfId")?,
            })
        })
        .collect::<AppResult<Vec<_>>>()?;

    // A reversed entry and its reversal, both on the list and neither ticked, cancel out.
    let mut hidden: HashSet<String> = HashSet::new();
    if rec.status == DRAFT {
        let unticked_reversed: HashMap<&str, &Raw> =
            raw.iter().filter(|r| r.reversed && !r.line.cleared).map(|r| (r.line.entry_id.as_str(), r)).collect();
        for r in raw.iter().filter(|r| !r.line.cleared) {
            if let Some(original) = r.reversal_of.as_deref().and_then(|o| unticked_reversed.get(o)) {
                hidden.insert(original.line.line_id.clone());
                hidden.insert(r.line.line_id.clone());
            }
        }
    }
    let lines: Vec<RecLine> = raw.into_iter().filter(|r| !hidden.contains(&r.line.line_id)).map(|r| r.line).collect();

    let mut cleared = rec.opening_balance;
    for l in lines.iter().filter(|l| l.cleared) {
        cleared = cleared.checked_add(l.debit).and_then(|c| c.checked_sub(l.credit)).ok_or_else(too_large)?;
    }
    let difference = rec.statement_balance.checked_sub(cleared).ok_or_else(too_large)?;
    Ok(Workspace { rec, lines, cleared_balance: cleared, difference })
}

async fn require_draft(tx: &mut Tx, id: &str) -> AppResult<Reconciliation> {
    let rec = find(tx, id).await?;
    if rec.status != DRAFT {
        return Err(AppError::bad_request("This reconciliation is completed — undo it first to change it"));
    }
    Ok(rec)
}

/// Replaces the ticked lines of an in-progress reconciliation with `line_ids`.
pub async fn set_cleared(db: &Db, id: &str, line_ids: &[String]) -> AppResult<Workspace> {
    let mut tx = db.begin().await?;
    require_draft(&mut tx, id).await?;
    let candidates: HashSet<String> = workspace(&mut tx, id).await?.lines.into_iter().map(|l| l.line_id).collect();
    if let Some(bad) = line_ids.iter().find(|l| !candidates.contains(*l)) {
        return Err(AppError::bad_request(format!(
            "Line {bad} cannot be cleared here — it is on another account, after the statement date, or already reconciled"
        )));
    }
    sqlx::query(r#"DELETE FROM "reconciled_lines" WHERE "reconciliationId" = ?1"#).bind(id).execute(&mut *tx).await?;
    for line in line_ids.iter().collect::<HashSet<_>>() {
        sqlx::query(r#"INSERT INTO "reconciled_lines" ("journalLineId","reconciliationId") VALUES (?1,?2)"#)
            .bind(line)
            .bind(id)
            .execute(&mut *tx)
            .await?;
    }
    sqlx::query(r#"UPDATE "bank_reconciliations" SET "updatedAt" = ?1 WHERE "id" = ?2"#)
        .bind(now_iso())
        .bind(id)
        .execute(&mut *tx)
        .await?;
    let ws = workspace(&mut tx, id).await?;
    tx.commit().await?;
    Ok(ws)
}

/// Completes the reconciliation; the cleared balance must equal the statement balance.
pub async fn complete(db: &Db, id: &str, user_id: Option<&str>) -> AppResult<Workspace> {
    let mut tx = db.begin().await?;
    require_draft(&mut tx, id).await?;
    let ws = workspace(&mut tx, id).await?;
    if ws.difference != Money::ZERO {
        return Err(AppError::bad_request(format!(
            "The cleared balance is {} but the statement says {} — a difference of {}. Tick the missing lines, or post the bank's charges first.",
            ws.cleared_balance, ws.rec.statement_balance, ws.difference
        )));
    }
    let now = now_iso();
    sqlx::query(
        r#"UPDATE "bank_reconciliations" SET "status" = 'COMPLETED', "completedAt" = ?1, "completedById" = ?2, "updatedAt" = ?1
           WHERE "id" = ?3"#,
    )
    .bind(&now)
    .bind(user_id)
    .bind(id)
    .execute(&mut *tx)
    .await?;
    let ws = workspace(&mut tx, id).await?;
    tx.commit().await?;
    Ok(ws)
}

/// Throws away an in-progress reconciliation and its ticks.
pub async fn discard(db: &Db, id: &str) -> AppResult<()> {
    let mut tx = db.begin().await?;
    require_draft(&mut tx, id).await?;
    sqlx::query(r#"DELETE FROM "reconciled_lines" WHERE "reconciliationId" = ?1"#).bind(id).execute(&mut *tx).await?;
    sqlx::query(r#"DELETE FROM "bank_reconciliations" WHERE "id" = ?1"#).bind(id).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(())
}

/// Puts the latest completed reconciliation of its account back in progress, ticks kept.
pub async fn undo(db: &Db, id: &str) -> AppResult<Workspace> {
    let mut tx = db.begin().await?;
    let rec = find(&mut tx, id).await?;
    if rec.status != COMPLETED {
        return Err(AppError::bad_request("Only a completed reconciliation can be undone"));
    }
    let latest = last_completed(&mut tx, &rec.account_id).await?;
    if latest.as_ref().map(|l| l.id.as_str()) != Some(id) {
        return Err(AppError::bad_request("Only the latest reconciliation of an account can be undone"));
    }
    let open: Option<String> =
        sqlx::query_scalar(r#"SELECT "id" FROM "bank_reconciliations" WHERE "accountId" = ?1 AND "status" = 'DRAFT'"#)
            .bind(&rec.account_id)
            .fetch_optional(&mut *tx)
            .await?;
    if open.is_some() {
        return Err(AppError::bad_request(
            "Discard the reconciliation in progress for this account before undoing the last one",
        ));
    }
    sqlx::query(
        r#"UPDATE "bank_reconciliations" SET "status" = 'DRAFT', "completedAt" = NULL, "completedById" = NULL, "updatedAt" = ?1
           WHERE "id" = ?2"#,
    )
    .bind(now_iso())
    .bind(id)
    .execute(&mut *tx)
    .await?;
    let ws = workspace(&mut tx, id).await?;
    tx.commit().await?;
    Ok(ws)
}
