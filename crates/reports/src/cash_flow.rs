//! Cash flow statement (D24, requested 2026-10-20; the reference had none).
//!
//! Direct method, straight from the ledger. "Cash" is a chosen set of asset accounts (by
//! default the 10xx accounts: 1000 Cash and any bank accounts beside it). For every journal
//! entry in the period that touches a cash account, each of its other lines says where the cash
//! came from or went: because the entry balances,
//!     sum over cash lines (debit - credit) = sum over the other lines (credit - debit),
//! so a line's (credit - debit) is exactly the cash it brought in (or, negative, paid out).
//! Moves between two cash accounts have no other line and do not appear. Every entry counts,
//! reversed or not (D1), so the closing cash equals the cash accounts' trial balance.
//!
//! Sections follow the counter-account's type and (free-text) subtype:
//! * Investing: asset accounts whose subtype marks them long-term — it contains "fixed",
//!   "non_current", "noncurrent", "long_term" or "investment";
//! * Financing: equity, and liabilities whose subtype contains "long_term", "non_current",
//!   "noncurrent" or "loan";
//! * Operating: everything else — revenue, expenses, receivables, inventory, payables,
//!   salaries and statutory payables, and any account without such a subtype.

use app_core::money::Money;
use app_core::{AppError, AppResult};
use ledger::accounts;
use ledger::{Account, AccountType};
use serde_json::{json, Value};
use sqlx::{Row, SqliteConnection};
use std::collections::{BTreeMap, HashSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Section {
    Operating,
    Investing,
    Financing,
}

pub fn section_of(account: &Account) -> Section {
    let subtype = account.subtype.as_deref().unwrap_or("").to_ascii_lowercase();
    let marked = |words: &[&str]| words.iter().any(|w| subtype.contains(w));
    match account.account_type {
        AccountType::Asset if marked(&["fixed", "non_current", "noncurrent", "long_term", "investment"]) => {
            Section::Investing
        }
        AccountType::Liability if marked(&["long_term", "non_current", "noncurrent", "loan"]) => Section::Financing,
        AccountType::Equity => Section::Financing,
        _ => Section::Operating,
    }
}

fn overflow() -> AppError {
    AppError::internal("cash flow overflow")
}

/// The default cash accounts: active asset accounts whose code starts with "10".
pub async fn default_cash_accounts(conn: &mut SqliteConnection) -> AppResult<Vec<String>> {
    let ids: Vec<String> = sqlx::query_scalar(
        r#"SELECT "id" FROM "accounts" WHERE "type" = 'ASSET' AND "isActive" = 1 AND "code" LIKE '10%' ORDER BY "code""#,
    )
    .fetch_all(&mut *conn)
    .await?;
    Ok(ids)
}

/// `date_from` / `date_to` are `YYYY-MM-DD`, or empty for "no bound".
pub async fn cash_flow(
    conn: &mut SqliteConnection,
    date_from: &str,
    date_to: &str,
    cash_ids: &[String],
) -> AppResult<Value> {
    if cash_ids.is_empty() {
        return Err(AppError::bad_request("Choose at least one cash or bank account"));
    }
    let mut cash_accounts = Vec::new();
    for id in cash_ids {
        let a = accounts::find_one(conn, id).await?;
        if a.account_type != AccountType::Asset {
            return Err(AppError::bad_request(format!("{} {} is not an asset account", a.code, a.name)));
        }
        cash_accounts.push(a);
    }
    let cash: HashSet<&str> = cash_ids.iter().map(String::as_str).collect();
    let from = (!date_from.is_empty()).then_some(date_from);
    let to = (!date_to.is_empty()).then_some(date_to);

    // Cash at the start: every cash line dated before the period.
    let mut opening = Money::ZERO;
    let mut closing_check = Money::ZERO;
    for a in &cash_accounts {
        let (d, c): (i64, i64) = sqlx::query_as(
            r#"SELECT COALESCE(SUM(l."debit"), 0), COALESCE(SUM(l."credit"), 0)
               FROM "journal_lines" l JOIN "journal_entries" e ON e."id" = l."journalEntryId"
               WHERE l."accountId" = ?1 AND ?2 IS NOT NULL AND e."date" < ?2"#,
        )
        .bind(&a.id)
        .bind(from)
        .fetch_one(&mut *conn)
        .await?;
        opening = opening.checked_add(Money::from_cents(d - c)).ok_or_else(overflow)?;
        let (d, c): (i64, i64) = sqlx::query_as(
            r#"SELECT COALESCE(SUM(l."debit"), 0), COALESCE(SUM(l."credit"), 0)
               FROM "journal_lines" l JOIN "journal_entries" e ON e."id" = l."journalEntryId"
               WHERE l."accountId" = ?1 AND (?2 IS NULL OR e."date" <= ?2)"#,
        )
        .bind(&a.id)
        .bind(to)
        .fetch_one(&mut *conn)
        .await?;
        closing_check = closing_check.checked_add(Money::from_cents(d - c)).ok_or_else(overflow)?;
    }

    // Every line of every entry in the period that touches cash.
    // Numbered like ?1/?2 above: SQLite numbers a bare "?" differently when mixed with "?N".
    let placeholders = (3..3 + cash_ids.len()).map(|i| format!("?{i}")).collect::<Vec<_>>().join(",");
    let sql = format!(
        r#"SELECT l."accountId", l."debit", l."credit"
           FROM "journal_lines" l JOIN "journal_entries" e ON e."id" = l."journalEntryId"
           WHERE (?1 IS NULL OR e."date" >= ?1) AND (?2 IS NULL OR e."date" <= ?2)
             AND e."id" IN (SELECT "journalEntryId" FROM "journal_lines" WHERE "accountId" IN ({placeholders}))"#
    );
    let mut q = sqlx::query(&sql).bind(from).bind(to);
    for id in cash_ids {
        q = q.bind(id);
    }
    let rows = q.fetch_all(&mut *conn).await?;
    let mut by_account: BTreeMap<String, Money> = BTreeMap::new();
    for r in &rows {
        let account_id: String = r.try_get("accountId")?;
        if cash.contains(account_id.as_str()) {
            continue;
        }
        let flow = Money::from_cents(r.try_get::<i64, _>("credit")? - r.try_get::<i64, _>("debit")?);
        let slot = by_account.entry(account_id).or_insert(Money::ZERO);
        *slot = slot.checked_add(flow).ok_or_else(overflow)?;
    }

    let mut sections: BTreeMap<Section, (Vec<(Account, Money)>, Money)> = BTreeMap::new();
    for (id, amount) in by_account {
        if amount == Money::ZERO {
            continue;
        }
        let account = accounts::find_one(conn, &id).await?;
        let entry = sections.entry(section_of(&account)).or_insert_with(|| (Vec::new(), Money::ZERO));
        entry.1 = entry.1.checked_add(amount).ok_or_else(overflow)?;
        entry.0.push((account, amount));
    }
    let mut net = Money::ZERO;
    let mut section_json = |s: Section| -> AppResult<Value> {
        let (mut lines, total) = sections.remove(&s).unwrap_or_default();
        lines.sort_by(|a, b| a.0.code.cmp(&b.0.code));
        net = net.checked_add(total).ok_or_else(overflow)?;
        Ok(json!({
            "lines": lines.iter().map(|(a, m)| json!({ "account": a, "amount": m.to_json_number() })).collect::<Vec<_>>(),
            "total": total.to_json_number(),
        }))
    };
    let operating = section_json(Section::Operating)?;
    let investing = section_json(Section::Investing)?;
    let financing = section_json(Section::Financing)?;
    let closing = opening.checked_add(net).ok_or_else(overflow)?;
    if closing != closing_check {
        // Cannot happen while every entry balances; refuse rather than print a wrong statement.
        return Err(AppError::internal(format!("cash flow does not reconcile: {closing} vs {closing_check}")));
    }
    Ok(json!({
        "dateFrom": date_from,
        "dateTo": date_to,
        "cashAccounts": cash_accounts,
        "openingCash": opening.to_json_number(),
        "operating": operating,
        "investing": investing,
        "financing": financing,
        "netChange": net.to_json_number(),
        "closingCash": closing.to_json_number(),
    }))
}
