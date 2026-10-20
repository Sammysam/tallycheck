//! Account activity — the drill-down behind every report figure (D23, requested 2026-10-20).
//!
//! For one account and an optional date range: the balance brought forward from before the
//! range, every line in it in date order with a running balance, and the closing balance.
//! Balances are in the account's normal direction (debit-normal for assets and expenses,
//! credit-normal otherwise) and count every entry whatever its status (D1), so the closing
//! balance is exactly what the trial balance shows for the same dates.

use crate::accounts::{self, Account};
use crate::entries::normal_balance;
use app_core::money::Money;
use app_core::{AppError, AppResult};
use serde_json::{json, Value};
use sqlx::{Row, SqliteConnection};

#[derive(Debug, Clone)]
pub struct ActivityLine {
    pub entry_id: String,
    pub entry_number: String,
    pub date: String,
    pub description: String,
    pub reference: Option<String>,
    pub source_type: Option<String>,
    pub line_description: Option<String>,
    pub debit: Money,
    pub credit: Money,
    pub balance: Money,
}

#[derive(Debug, Clone)]
pub struct Activity {
    pub account: Account,
    pub date_from: Option<String>,
    pub date_to: Option<String>,
    pub opening: Money,
    pub lines: Vec<ActivityLine>,
    pub total_debit: Money,
    pub total_credit: Money,
    pub closing: Money,
}

fn overflow() -> AppError {
    AppError::internal("balance overflow")
}

impl Activity {
    pub fn to_json(&self) -> Value {
        let lines: Vec<Value> = self
            .lines
            .iter()
            .map(|l| {
                json!({
                    "entryId": l.entry_id,
                    "entryNumber": l.entry_number,
                    "date": l.date,
                    "description": l.description,
                    "reference": l.reference,
                    "sourceType": l.source_type,
                    "lineDescription": l.line_description,
                    "debit": l.debit.to_json_number(),
                    "credit": l.credit.to_json_number(),
                    "balance": l.balance.to_json_number(),
                })
            })
            .collect();
        json!({
            "account": self.account,
            "dateFrom": self.date_from,
            "dateTo": self.date_to,
            "openingBalance": self.opening.to_json_number(),
            "lines": lines,
            "totalDebit": self.total_debit.to_json_number(),
            "totalCredit": self.total_credit.to_json_number(),
            "closingBalance": self.closing.to_json_number(),
        })
    }
}

pub async fn account_activity(
    conn: &mut SqliteConnection,
    account_id: &str,
    date_from: Option<&str>,
    date_to: Option<&str>,
) -> AppResult<Activity> {
    let account = accounts::find_one(conn, account_id).await?;
    let kind = account.account_type;
    let opening = match date_from {
        Some(from) => {
            let (d, c): (i64, i64) = sqlx::query_as(
                r#"SELECT COALESCE(SUM(l."debit"), 0), COALESCE(SUM(l."credit"), 0)
                   FROM "journal_lines" l JOIN "journal_entries" e ON e."id" = l."journalEntryId"
                   WHERE l."accountId" = ?1 AND e."date" < ?2"#,
            )
            .bind(account_id)
            .bind(from)
            .fetch_one(&mut *conn)
            .await?;
            normal_balance(kind, Money::from_cents(d), Money::from_cents(c))?
        }
        None => Money::ZERO,
    };
    let rows = sqlx::query(
        r#"SELECT e."id", e."entryNumber", e."date", e."description", e."reference", e."sourceType",
                  l."description" AS "lineDescription", l."debit", l."credit"
           FROM "journal_lines" l JOIN "journal_entries" e ON e."id" = l."journalEntryId"
           WHERE l."accountId" = ?1 AND (?2 IS NULL OR e."date" >= ?2) AND (?3 IS NULL OR e."date" <= ?3)
           ORDER BY e."date", e."entryNumber", l.rowid"#,
    )
    .bind(account_id)
    .bind(date_from)
    .bind(date_to)
    .fetch_all(&mut *conn)
    .await?;

    let (mut running, mut total_debit, mut total_credit) = (opening, Money::ZERO, Money::ZERO);
    let mut lines = Vec::with_capacity(rows.len());
    for r in &rows {
        let debit = Money::from_cents(r.try_get("debit")?);
        let credit = Money::from_cents(r.try_get("credit")?);
        running = running.checked_add(normal_balance(kind, debit, credit)?).ok_or_else(overflow)?;
        total_debit = total_debit.checked_add(debit).ok_or_else(overflow)?;
        total_credit = total_credit.checked_add(credit).ok_or_else(overflow)?;
        lines.push(ActivityLine {
            entry_id: r.try_get("id")?,
            entry_number: r.try_get("entryNumber")?,
            date: r.try_get("date")?,
            description: r.try_get("description")?,
            reference: r.try_get("reference")?,
            source_type: r.try_get("sourceType")?,
            line_description: r.try_get("lineDescription")?,
            debit,
            credit,
            balance: running,
        });
    }
    Ok(Activity {
        account,
        date_from: date_from.map(str::to_string),
        date_to: date_to.map(str::to_string),
        opening,
        lines,
        total_debit,
        total_credit,
        closing: running,
    })
}
