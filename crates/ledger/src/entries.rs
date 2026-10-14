//! Journal entries (reference `ledger.service.ts`).
//!
//! Invariants enforced here and in the database:
//! * at least two lines; debits equal credits **exactly** (integer cents, no tolerance);
//! * amounts are never negative; every line references an existing account;
//! * the entry number is taken in the same transaction, so a failed post leaves no gap;
//! * posted entries are never updated or deleted (DB triggers) — the only change is the
//!   status POSTED → REVERSED when a reversal is posted, and an entry is reversed at most once.
//!
//! Balances (approved deviation D1): every entry counts, whatever its status. A reversed entry
//! and its reversal therefore cancel out. The reference counted only POSTED entries, which
//! dropped the original but kept its reversal, so every reversal doubled its own effect.

use crate::accounts::{self, Account, AccountType};
use app_core::ids::new_id;
use app_core::money::{as_db_string, as_json_number, Money};
use app_core::time::now_iso;
use app_core::{AppError, AppResult};
use database::{counters, Tx};
use serde::Serialize;
use sqlx::sqlite::SqliteRow;
use sqlx::{Row, SqliteConnection};
use std::collections::HashMap;

/// Dates are `YYYY-MM-DD` text, compared lexically (chronological for this format).
pub type DateStr = String;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum EntryStatus {
    Posted,
    Reversed,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JournalLine {
    pub id: String,
    pub journal_entry_id: String,
    pub account_id: String,
    pub description: Option<String>,
    #[serde(serialize_with = "as_db_string")]
    pub debit: Money,
    #[serde(serialize_with = "as_db_string")]
    pub credit: Money,
    /// Present when read back (the reference eager-loads it), absent in post/reverse answers.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account: Option<Account>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JournalEntry {
    pub id: String,
    pub entry_number: String,
    pub date: DateStr,
    pub description: String,
    pub reference: Option<String>,
    pub source_type: Option<String>,
    pub source_id: Option<String>,
    pub status: EntryStatus,
    pub reversal_of_id: Option<String>,
    pub lines: Vec<JournalLine>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone)]
pub struct NewLine {
    pub account_id: String,
    pub debit: Money,
    pub credit: Money,
    pub description: Option<String>,
}

#[derive(Debug, Clone)]
pub struct NewEntry {
    pub date: DateStr,
    pub description: String,
    pub reference: Option<String>,
    pub source_type: Option<String>,
    pub source_id: Option<String>,
    pub lines: Vec<NewLine>,
}

fn total(lines: &[NewLine], f: impl Fn(&NewLine) -> Money) -> AppResult<Money> {
    lines.iter().try_fold(Money::ZERO, |acc, l| {
        acc.checked_add(f(l)).ok_or_else(|| AppError::bad_request("Journal entry amounts are too large"))
    })
}

/// Posts a balanced entry inside the caller's transaction.
pub async fn post(tx: &mut Tx, entry: NewEntry) -> AppResult<JournalEntry> {
    // Same order of checks as the reference: balance first, then line count.
    let debits = total(&entry.lines, |l| l.debit)?;
    let credits = total(&entry.lines, |l| l.credit)?;
    if debits != credits {
        return Err(AppError::bad_request(format!(
            "Journal entry is not balanced: debits {} ≠ credits {}",
            debits.to_json_number(),
            credits.to_json_number()
        )));
    }
    if entry.lines.len() < 2 {
        return Err(AppError::bad_request("A journal entry requires at least 2 lines"));
    }
    if entry.lines.iter().any(|l| l.debit < Money::ZERO || l.credit < Money::ZERO) {
        return Err(AppError::bad_request("Journal line amounts cannot be negative"));
    }
    for l in &entry.lines {
        // The reference let the database reject an unknown account (a 500); this says why.
        accounts::find_one(tx, &l.account_id).await?;
    }
    insert(tx, entry, None).await
}

async fn insert(tx: &mut Tx, entry: NewEntry, reversal_of: Option<&str>) -> AppResult<JournalEntry> {
    let id = new_id();
    let number = counters::next_formatted(tx, "JE", 6).await?;
    let now = now_iso();
    sqlx::query(
        r#"INSERT INTO "journal_entries"
             ("id","entryNumber","date","description","reference","sourceType","sourceId","status","reversalOfId","createdAt","updatedAt")
           VALUES (?1,?2,?3,?4,?5,?6,?7,'POSTED',?8,?9,?9)"#,
    )
    .bind(&id)
    .bind(&number)
    .bind(&entry.date)
    .bind(&entry.description)
    .bind(&entry.reference)
    .bind(&entry.source_type)
    .bind(&entry.source_id)
    .bind(reversal_of)
    .bind(&now)
    .execute(&mut **tx)
    .await?;
    for l in &entry.lines {
        sqlx::query(
            r#"INSERT INTO "journal_lines" ("id","journalEntryId","accountId","description","debit","credit")
               VALUES (?1,?2,?3,?4,?5,?6)"#,
        )
        .bind(new_id())
        .bind(&id)
        .bind(&l.account_id)
        .bind(&l.description)
        .bind(l.debit.cents())
        .bind(l.credit.cents())
        .execute(&mut **tx)
        .await?;
    }
    load(tx, &id, false).await?.ok_or_else(|| AppError::internal("posted entry vanished"))
}

/// Posts the mirror image of `id` dated `date` and marks the original REVERSED.
/// The reference dated reversals "today"; callers pass the local business date (D5).
pub async fn reverse(tx: &mut Tx, id: &str, date: &str) -> AppResult<JournalEntry> {
    let original = load(tx, id, false).await?.ok_or_else(|| not_found(id))?;
    if original.status == EntryStatus::Reversed {
        return Err(AppError::bad_request("This entry has already been reversed"));
    }
    let mirror = NewEntry {
        date: date.to_string(),
        description: format!("Reversal of {}: {}", original.entry_number, original.description),
        reference: original.reference.clone(),
        source_type: original.source_type.clone(),
        source_id: original.source_id.clone(),
        lines: original
            .lines
            .iter()
            .map(|l| NewLine {
                account_id: l.account_id.clone(),
                debit: l.credit,
                credit: l.debit,
                description: l.description.clone(),
            })
            .collect(),
    };
    let reversal = insert(tx, mirror, Some(&original.id)).await?;
    sqlx::query(r#"UPDATE "journal_entries" SET "status" = 'REVERSED', "updatedAt" = ?1 WHERE "id" = ?2"#)
        .bind(now_iso())
        .bind(&original.id)
        .execute(&mut **tx)
        .await?;
    Ok(reversal)
}

fn not_found(id: &str) -> AppError {
    AppError::NotFound(format!("Journal entry {id} not found"))
}

const ENTRY_COLUMNS: &str = r#""id","entryNumber","date","description","reference","sourceType","sourceId","status","reversalOfId","createdAt","updatedAt""#;

fn entry_from_row(row: &SqliteRow) -> AppResult<JournalEntry> {
    let status: String = row.try_get("status")?;
    Ok(JournalEntry {
        id: row.try_get("id")?,
        entry_number: row.try_get("entryNumber")?,
        date: row.try_get("date")?,
        description: row.try_get("description")?,
        reference: row.try_get("reference")?,
        source_type: row.try_get("sourceType")?,
        source_id: row.try_get("sourceId")?,
        status: if status == "REVERSED" { EntryStatus::Reversed } else { EntryStatus::Posted },
        reversal_of_id: row.try_get("reversalOfId")?,
        lines: Vec::new(),
        created_at: row.try_get("createdAt")?,
        updated_at: row.try_get("updatedAt")?,
    })
}

/// Attaches lines (in posting order) and, when `with_accounts`, each line's account.
async fn attach_lines(conn: &mut SqliteConnection, entries: &mut [JournalEntry], with_accounts: bool) -> AppResult<()> {
    if entries.is_empty() {
        return Ok(());
    }
    let index: HashMap<String, usize> = entries.iter().enumerate().map(|(i, e)| (e.id.clone(), i)).collect();
    let placeholders = vec!["?"; entries.len()].join(",");
    let sql = format!(
        r#"SELECT l."id", l."journalEntryId", l."accountId", l."description", l."debit", l."credit"
           FROM "journal_lines" l WHERE l."journalEntryId" IN ({placeholders}) ORDER BY l.rowid"#
    );
    let mut q = sqlx::query(&sql);
    for e in entries.iter() {
        q = q.bind(&e.id);
    }
    let rows = q.fetch_all(&mut *conn).await?;
    let mut account_cache: HashMap<String, Account> = HashMap::new();
    for r in &rows {
        let account_id: String = r.try_get("accountId")?;
        let account = if with_accounts {
            if !account_cache.contains_key(&account_id) {
                if let Some(a) = accounts::get(&mut *conn, &account_id).await? {
                    account_cache.insert(account_id.clone(), a);
                }
            }
            account_cache.get(&account_id).cloned()
        } else {
            None
        };
        let entry_id: String = r.try_get("journalEntryId")?;
        let line = JournalLine {
            id: r.try_get("id")?,
            journal_entry_id: entry_id.clone(),
            account_id,
            description: r.try_get("description")?,
            debit: Money::from_cents(r.try_get("debit")?),
            credit: Money::from_cents(r.try_get("credit")?),
            account,
        };
        if let Some(i) = index.get(&entry_id) {
            entries[*i].lines.push(line);
        }
    }
    Ok(())
}

async fn load(conn: &mut SqliteConnection, id: &str, with_accounts: bool) -> AppResult<Option<JournalEntry>> {
    let row = sqlx::query(&format!(r#"SELECT {ENTRY_COLUMNS} FROM "journal_entries" WHERE "id" = ?1"#))
        .bind(id)
        .fetch_optional(&mut *conn)
        .await?;
    let Some(row) = row else { return Ok(None) };
    let mut entries = vec![entry_from_row(&row)?];
    attach_lines(conn, &mut entries, with_accounts).await?;
    Ok(entries.pop())
}

/// Reference `getEntry`: the entry with its lines and their accounts.
pub async fn get(conn: &mut SqliteConnection, id: &str) -> AppResult<JournalEntry> {
    load(conn, id, true).await?.ok_or_else(|| not_found(id))
}

#[derive(Debug, Default, Clone)]
pub struct EntryFilter {
    pub source_type: Option<String>,
    pub date_from: Option<String>,
    pub date_to: Option<String>,
}

/// Reference `getEntries`: newest first; the date range applies only when both ends are given.
pub async fn list(conn: &mut SqliteConnection, filter: &EntryFilter) -> AppResult<Vec<JournalEntry>> {
    let mut sql = format!(r#"SELECT {ENTRY_COLUMNS} FROM "journal_entries" WHERE 1 = 1"#);
    let mut binds: Vec<&str> = Vec::new();
    if let Some(st) = filter.source_type.as_deref().filter(|s| !s.is_empty()) {
        sql.push_str(r#" AND "sourceType" = ?"#);
        binds.push(st);
    }
    if let (Some(from), Some(to)) = (filter.date_from.as_deref(), filter.date_to.as_deref()) {
        if !from.is_empty() && !to.is_empty() {
            sql.push_str(r#" AND "date" BETWEEN ? AND ?"#);
            binds.push(from);
            binds.push(to);
        }
    }
    sql.push_str(r#" ORDER BY "date" DESC, "entryNumber" DESC"#);
    let mut q = sqlx::query(&sql);
    for b in binds {
        q = q.bind(b);
    }
    let rows = q.fetch_all(&mut *conn).await?;
    let mut entries = rows.iter().map(entry_from_row).collect::<AppResult<Vec<_>>>()?;
    attach_lines(conn, &mut entries, true).await?;
    Ok(entries)
}

#[derive(Debug, Clone, Copy, Serialize)]
pub struct Totals {
    #[serde(serialize_with = "as_json_number")]
    pub debit: Money,
    #[serde(serialize_with = "as_json_number")]
    pub credit: Money,
    #[serde(serialize_with = "as_json_number")]
    pub balance: Money,
}

fn signed(t: Option<AccountType>, debit: Money, credit: Money) -> AppResult<Money> {
    let (a, b) = match t {
        Some(t) if t.is_debit_normal() => (debit, credit),
        _ => (credit, debit),
    };
    a.checked_sub(b).ok_or_else(|| AppError::internal("balance overflow"))
}

/// Reference `getAccountBalance`. Empty strings mean "no bound", as the reference's truthiness
/// checks did. An unknown account yields zeros.
pub async fn account_balance(
    conn: &mut SqliteConnection,
    account_id: &str,
    date_from: Option<&str>,
    date_to: Option<&str>,
) -> AppResult<Totals> {
    let from = date_from.filter(|s| !s.is_empty());
    let to = date_to.filter(|s| !s.is_empty());
    let (debit, credit): (i64, i64) = sqlx::query_as(
        r#"SELECT COALESCE(SUM(l."debit"), 0), COALESCE(SUM(l."credit"), 0)
           FROM "journal_lines" l JOIN "journal_entries" e ON e."id" = l."journalEntryId"
           WHERE l."accountId" = ?1 AND (?2 IS NULL OR e."date" >= ?2) AND (?3 IS NULL OR e."date" <= ?3)"#,
    )
    .bind(account_id)
    .bind(from)
    .bind(to)
    .fetch_one(&mut *conn)
    .await?;
    let account = accounts::get(&mut *conn, account_id).await?;
    let (debit, credit) = (Money::from_cents(debit), Money::from_cents(credit));
    Ok(Totals { debit, credit, balance: signed(account.map(|a| a.account_type), debit, credit)? })
}

#[derive(Debug, Clone, Serialize)]
pub struct TrialBalanceRow {
    pub account: Account,
    #[serde(serialize_with = "as_json_number")]
    pub debit: Money,
    #[serde(serialize_with = "as_json_number")]
    pub credit: Money,
    #[serde(serialize_with = "as_json_number")]
    pub balance: Money,
}

/// Reference `getTrialBalance`: active accounts by code, up to `as_of`, omitting accounts with
/// no movement.
pub async fn trial_balance(conn: &mut SqliteConnection, as_of: Option<&str>) -> AppResult<Vec<TrialBalanceRow>> {
    let to = as_of.filter(|s| !s.is_empty());
    let sums: Vec<(String, i64, i64)> = sqlx::query_as(
        r#"SELECT l."accountId", COALESCE(SUM(l."debit"), 0), COALESCE(SUM(l."credit"), 0)
           FROM "journal_lines" l JOIN "journal_entries" e ON e."id" = l."journalEntryId"
           WHERE (?1 IS NULL OR e."date" <= ?1)
           GROUP BY l."accountId""#,
    )
    .bind(to)
    .fetch_all(&mut *conn)
    .await?;
    let sums: HashMap<String, (i64, i64)> = sums.into_iter().map(|(a, d, c)| (a, (d, c))).collect();
    let rows = sqlx::query(&format!(
        r#"SELECT {} FROM "accounts" WHERE "isActive" = 1 ORDER BY "code" ASC"#,
        accounts::COLUMNS
    ))
    .fetch_all(&mut *conn)
    .await?;
    let mut out = Vec::new();
    for r in &rows {
        let account = accounts::from_row(r)?;
        let (d, c) = sums.get(&account.id).copied().unwrap_or((0, 0));
        if d == 0 && c == 0 {
            continue;
        }
        let (debit, credit) = (Money::from_cents(d), Money::from_cents(c));
        let balance = signed(Some(account.account_type), debit, credit)?;
        out.push(TrialBalanceRow { account, debit, credit, balance });
    }
    Ok(out)
}
