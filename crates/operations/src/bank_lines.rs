//! D37: imported bank statement lines and how each one is accounted for.
//!
//! A line is UNMATCHED until a person decides what it is:
//! * **match** it to a payment or expense already recorded on that account (same money, same
//!   direction — the app suggests those within a week of the line's date);
//! * **record a payment** for an open invoice (money in) or bill (money out) — the payment is
//!   created from the line's date, amount and account, through the normal payment rules;
//! * **record an expense** (money out) — created and posted in one step, as the Expenses screen
//!   would, because the person has just confirmed it;
//! * **record a transfer** to or from another of the company's accounts (D39: a Shopify payout
//!   reaching the bank, cash banked) — one journal entry; the matching line on the other account's
//!   statement is then suggested against the same entry;
//! * or **ignore** it (an opening balance row…).
//!
//! An imported line may carry a hint (D39): "record as an expense on account X" or "transfer to
//! account X", which the screen offers as a one-click suggestion (Shopify fees and payouts).
//!
//! Unmatching only frees the line; the payment or expense stays (void it on its own screen).
//! The suggestions are only suggestions: nothing is matched without a click.

use crate::expenses::{self, CreateExpense};
use crate::payments::{self, CreatePayment};
use crate::{bills, invoices, money_num, overflow};
use app_core::ids::new_id;
use app_core::money::Money;
use app_core::time::now_iso;
use app_core::{AppError, AppResult};
use database::{Db, Tx};
use ledger::{entries, NewEntry, NewLine};
use serde_json::{json, Value};
use sqlx::sqlite::SqliteRow;
use sqlx::{Row, SqliteConnection};

/// How far from the line's date a recorded payment or expense is suggested.
const SUGGEST_DAYS: i64 = 7;
const MAX_SUGGESTIONS: usize = 4;

#[derive(Debug, Clone)]
pub struct BankLine {
    pub id: String,
    pub import_id: Option<String>,
    pub account_id: String,
    pub date: String,
    pub description: String,
    pub reference: Option<String>,
    /// Account currency, cents: positive = money in.
    pub amount: Money,
    pub status: String,
    pub match_type: Option<String>,
    pub match_id: Option<String>,
    /// D39: a suggested action (EXPENSE / TRANSFER) and its account.
    pub hint_action: Option<String>,
    pub hint_account_id: Option<String>,
}

impl BankLine {
    fn money_in(&self) -> bool {
        self.amount > Money::ZERO
    }

    fn size(&self) -> AppResult<Money> {
        if self.amount < Money::ZERO {
            Money::ZERO.checked_sub(self.amount).ok_or_else(overflow)
        } else {
            Ok(self.amount)
        }
    }
}

const COLUMNS: &str = r#""id","importId","accountId","date","description","reference","amount","status","matchType","matchId",
    "hintAction","hintAccountId""#;

fn from_row(r: &SqliteRow) -> AppResult<BankLine> {
    Ok(BankLine {
        id: r.try_get("id")?,
        import_id: r.try_get("importId")?,
        account_id: r.try_get("accountId")?,
        date: r.try_get("date")?,
        description: r.try_get("description")?,
        reference: r.try_get("reference")?,
        amount: Money::from_cents(r.try_get("amount")?),
        status: r.try_get("status")?,
        match_type: r.try_get("matchType")?,
        match_id: r.try_get("matchId")?,
        hint_action: r.try_get("hintAction")?,
        hint_account_id: r.try_get("hintAccountId")?,
    })
}

/// A statement line to store (an import, or a connector such as Shopify).
pub struct NewBankLine<'a> {
    pub import_id: &'a str,
    pub account_id: &'a str,
    pub date: &'a str,
    pub description: &'a str,
    pub reference: Option<&'a str>,
    pub amount: Money,
    /// D39: (EXPENSE | TRANSFER, account id).
    pub hint: Option<(&'a str, &'a str)>,
}

/// Stores a new UNMATCHED line; returns its id.
pub async fn insert(tx: &mut Tx, l: NewBankLine<'_>) -> AppResult<String> {
    let id = new_id();
    let now = now_iso();
    sqlx::query(
        r#"INSERT INTO "bank_lines" ("id","importId","accountId","date","description","reference","amount","hintAction",
             "hintAccountId","createdAt","updatedAt")
           VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?10)"#,
    )
    .bind(&id)
    .bind(l.import_id)
    .bind(l.account_id)
    .bind(l.date)
    .bind(l.description)
    .bind(l.reference)
    .bind(l.amount.cents())
    .bind(l.hint.map(|h| h.0))
    .bind(l.hint.map(|h| h.1))
    .bind(&now)
    .execute(&mut **tx)
    .await?;
    Ok(id)
}

pub async fn find_one(conn: &mut SqliteConnection, id: &str) -> AppResult<BankLine> {
    let row = sqlx::query(&format!(r#"SELECT {COLUMNS} FROM "bank_lines" WHERE "id" = ?1"#))
        .bind(id)
        .fetch_optional(conn)
        .await?;
    match row {
        Some(r) => from_row(&r),
        None => Err(AppError::NotFound(format!("Bank transaction with ID {id} not found"))),
    }
}

async fn set(tx: &mut Tx, id: &str, status: &str, match_type: Option<&str>, match_id: Option<&str>) -> AppResult<()> {
    sqlx::query(
        r#"UPDATE "bank_lines" SET "status" = ?1, "matchType" = ?2, "matchId" = ?3, "updatedAt" = ?4 WHERE "id" = ?5"#,
    )
    .bind(status)
    .bind(match_type)
    .bind(match_id)
    .bind(now_iso())
    .bind(id)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

async fn unmatched(tx: &mut Tx, id: &str) -> AppResult<BankLine> {
    let line = find_one(tx, id).await?;
    if line.status != "UNMATCHED" {
        return Err(AppError::bad_request(format!(
            "This bank transaction is already {}",
            if line.status == "MATCHED" { "matched" } else { "ignored" }
        )));
    }
    Ok(line)
}

/// The account's currency (None = home), which every document matched to a line must share.
async fn account_currency(conn: &mut SqliteConnection, account_id: &str) -> AppResult<Option<String>> {
    Ok(ledger::accounts::find_one(conn, account_id).await?.currency)
}

// ─── Candidates ─────────────────────────────────────────────────────────────

/// A recorded payment's money as it moved through its account: in the account's own currency
/// for a foreign-currency account (D34), otherwise in the home currency.
fn payment_money(p: &payments::Payment, foreign_account: bool) -> Money {
    if foreign_account {
        p.amount
    } else {
        p.base_amount
    }
}

fn payment_money_in(p: &payments::Payment) -> bool {
    p.kind == "AR"
}

/// Already matched by another line. A transfer entry touches two accounts, so it is matched once
/// per account (`account_id`); payments and expenses once in all.
async fn matched_elsewhere(
    conn: &mut SqliteConnection,
    match_type: &str,
    id: &str,
    account_id: &str,
) -> AppResult<bool> {
    let hit: Option<i64> = sqlx::query_scalar(
        r#"SELECT 1 FROM "bank_lines" WHERE "status" = 'MATCHED' AND "matchType" = ?1 AND "matchId" = ?2
             AND (?1 <> 'TRANSFER' OR "accountId" = ?3)"#,
    )
    .bind(match_type)
    .bind(id)
    .bind(account_id)
    .fetch_optional(conn)
    .await?;
    Ok(hit.is_some())
}

/// D39: posted transfer entries that move `size` into (money in) or out of the account.
async fn transfers_near(
    conn: &mut SqliteConnection,
    line: &BankLine,
    size: Money,
) -> AppResult<Vec<(String, String, String)>> {
    let side = if line.money_in() { "debit" } else { "credit" };
    let rows: Vec<(String, String, String)> = sqlx::query_as(&format!(
        r#"SELECT e."id", e."entryNumber", e."date" FROM "journal_entries" e
             JOIN "journal_lines" l ON l."journalEntryId" = e."id"
            WHERE e."sourceType" = 'BANK_TRANSFER' AND e."status" = 'POSTED' AND l."accountId" = ?1 AND l."{side}" = ?2
              AND e."date" BETWEEN date(?3, '-{SUGGEST_DAYS} days') AND date(?3, '+{SUGGEST_DAYS} days')
            ORDER BY abs(julianday(e."date") - julianday(?3))"#
    ))
    .bind(&line.account_id)
    .bind(size.cents())
    .bind(&line.date)
    .fetch_all(&mut *conn)
    .await?;
    Ok(rows)
}

async fn suggestions(conn: &mut SqliteConnection, line: &BankLine, currency: &Option<String>) -> AppResult<Vec<Value>> {
    let size = line.size()?;
    let foreign = currency.is_some();
    let mut out = Vec::new();

    // Already recorded: payments and expenses on this account, same money, near the date.
    let near = |table: &str| {
        format!(
            r#"SELECT "id" FROM "{table}" WHERE "paymentAccountId" = ?1 AND "status" = 'POSTED'
                 AND "date" BETWEEN date(?2, '-{SUGGEST_DAYS} days') AND date(?2, '+{SUGGEST_DAYS} days')
               ORDER BY abs(julianday("date") - julianday(?2))"#
        )
    };
    let ids: Vec<String> =
        sqlx::query_scalar(&near("payments")).bind(&line.account_id).bind(&line.date).fetch_all(&mut *conn).await?;
    for id in ids {
        let p = payments::find_one(conn, &id).await?;
        if payment_money_in(&p) == line.money_in()
            && payment_money(&p, foreign) == size
            && !matched_elsewhere(conn, "PAYMENT", &p.id, &line.account_id).await?
        {
            out.push(json!({ "type": "PAYMENT", "id": p.id, "number": p.payment_number, "date": p.date,
                             "amount": money_num(size), "exact": true }));
        }
    }
    if !line.money_in() && !foreign {
        let ids: Vec<String> =
            sqlx::query_scalar(&near("expenses")).bind(&line.account_id).bind(&line.date).fetch_all(&mut *conn).await?;
        for id in ids {
            let e = expenses::find_one(conn, &id).await?;
            if e.amount == size && !matched_elsewhere(conn, "EXPENSE", &e.id, &line.account_id).await? {
                out.push(json!({ "type": "EXPENSE", "id": e.id, "number": e.expense_number, "date": e.date,
                                 "party": e.description, "amount": money_num(size), "exact": true }));
            }
        }
    }

    if !foreign {
        for (id, number, date) in transfers_near(conn, line, size).await? {
            if !matched_elsewhere(conn, "TRANSFER", &id, &line.account_id).await? {
                out.push(json!({ "type": "TRANSFER", "id": id, "number": number, "date": date,
                                 "amount": money_num(size), "exact": true }));
            }
        }
    }

    // Open documents in the account's currency: the exact amount owed first, then those whose
    // customer / vendor is named in the bank text (part payments).
    let text = format!("{} {}", line.description, line.reference.clone().unwrap_or_default()).to_lowercase();
    let mut docs: Vec<(bool, Value)> = Vec::new();
    for (owed, party, mut v) in open_documents(conn, line.money_in(), currency).await? {
        let exact = owed == size;
        let named = party.len() >= 3 && text.contains(&party.to_lowercase()) && size <= owed;
        if exact || named {
            v["exact"] = json!(exact);
            docs.push((exact, v));
        }
    }
    docs.sort_by_key(|(exact, _)| !*exact);
    out.extend(docs.into_iter().map(|(_, v)| v));
    out.truncate(MAX_SUGGESTIONS);
    Ok(out)
}

/// Posted invoices (money in) or bills (money out) in `currency` with something still owed:
/// (owed, customer / vendor name, JSON for the screen).
async fn open_documents(
    conn: &mut SqliteConnection,
    money_in: bool,
    currency: &Option<String>,
) -> AppResult<Vec<(Money, String, Value)>> {
    let mut out = Vec::new();
    if money_in {
        let ids: Vec<String> = sqlx::query_scalar(
            r#"SELECT "id" FROM "invoices" WHERE "status" = 'POSTED' ORDER BY "date", "invoiceNumber""#,
        )
        .fetch_all(&mut *conn)
        .await?;
        for id in ids {
            let inv = invoices::load_any(conn, &id).await?;
            let owed = inv.outstanding()?;
            if &inv.currency != currency || owed <= Money::ZERO {
                continue;
            }
            let party =
                inv.customer.as_ref().map(|c| c.name.clone()).or_else(|| inv.bill_to_name.clone()).unwrap_or_default();
            let v = json!({ "type": "INVOICE", "id": inv.id, "number": inv.invoice_number, "date": inv.date,
                            "party": party, "amount": money_num(owed) });
            out.push((owed, party, v));
        }
    } else {
        let ids: Vec<String> =
            sqlx::query_scalar(r#"SELECT "id" FROM "bills" WHERE "status" = 'POSTED' ORDER BY "date", "billNumber""#)
                .fetch_all(&mut *conn)
                .await?;
        for id in ids {
            let b = bills::find_one(conn, &id).await?;
            let owed = b.outstanding()?;
            if &b.currency != currency || owed <= Money::ZERO {
                continue;
            }
            let party = b.vendor.as_ref().map(|v| v.name.clone()).unwrap_or_default();
            let v = json!({ "type": "BILL", "id": b.id, "number": b.bill_number, "date": b.date,
                            "party": party, "amount": money_num(owed) });
            out.push((owed, party, v));
        }
    }
    Ok(out)
}

/// Every open invoice or bill a line could pay, for the "Payment for…" dialog.
pub async fn documents(db: &Db, id: &str) -> AppResult<Value> {
    let mut conn = db.read().acquire().await?;
    let line = find_one(&mut conn, id).await?;
    let currency = account_currency(&mut conn, &line.account_id).await?;
    let docs = open_documents(&mut conn, line.money_in(), &currency).await?;
    Ok(Value::Array(docs.into_iter().map(|(_, _, v)| v).collect()))
}

async fn match_label(conn: &mut SqliteConnection, line: &BankLine) -> AppResult<Value> {
    let (Some(t), Some(id)) = (&line.match_type, &line.match_id) else { return Ok(Value::Null) };
    let (number, status, journal): (Option<String>, Option<String>, Option<String>) = match t.as_str() {
        "PAYMENT" => {
            sqlx::query_as(r#"SELECT "paymentNumber","status","journalEntryId" FROM "payments" WHERE "id" = ?1"#)
        }
        "TRANSFER" => sqlx::query_as(r#"SELECT "entryNumber","status","id" FROM "journal_entries" WHERE "id" = ?1"#),
        _ => sqlx::query_as(r#"SELECT "expenseNumber","status","journalEntryId" FROM "expenses" WHERE "id" = ?1"#),
    }
    .bind(id)
    .fetch_optional(conn)
    .await?
    .unwrap_or((None, None, None));
    Ok(json!({ "type": t, "id": id, "number": number, "status": status, "journalEntryId": journal }))
}

fn to_json(line: &BankLine) -> Value {
    json!({
        "id": line.id,
        "importId": line.import_id,
        "accountId": line.account_id,
        "date": line.date,
        "description": line.description,
        "reference": line.reference,
        "amount": money_num(line.amount),
        "status": line.status,
        "hint": match (&line.hint_action, &line.hint_account_id) {
            (Some(a), Some(acc)) => json!({ "action": a, "accountId": acc }),
            _ => Value::Null,
        },
    })
}

/// The account's lines, newest first; unmatched ones carry suggestions, matched ones what they
/// were matched to. `status` None = all.
pub async fn list(db: &Db, account_id: &str, status: Option<&str>) -> AppResult<Value> {
    let mut conn = db.read().acquire().await?;
    let currency = account_currency(&mut conn, account_id).await?;
    let rows = sqlx::query(&format!(
        r#"SELECT {COLUMNS} FROM "bank_lines" WHERE "accountId" = ?1 AND (?2 IS NULL OR "status" = ?2)
           ORDER BY "date" DESC, "createdAt" DESC LIMIT 1000"#
    ))
    .bind(account_id)
    .bind(status)
    .fetch_all(&mut *conn)
    .await?;
    let mut out = Vec::new();
    for r in &rows {
        let line = from_row(r)?;
        let mut v = to_json(&line);
        if line.status == "UNMATCHED" {
            v["suggestions"] = Value::Array(suggestions(&mut conn, &line, &currency).await?);
        } else if line.status == "MATCHED" {
            v["match"] = match_label(&mut conn, &line).await?;
        }
        out.push(v);
    }
    let counts: Vec<(String, i64)> =
        sqlx::query_as(r#"SELECT "status", count(*) FROM "bank_lines" WHERE "accountId" = ?1 GROUP BY "status""#)
            .bind(account_id)
            .fetch_all(&mut *conn)
            .await?;
    let count = |s: &str| counts.iter().find(|(k, _)| k == s).map(|(_, n)| *n).unwrap_or(0);
    Ok(json!({
        "currency": currency,
        "data": out,
        "counts": { "UNMATCHED": count("UNMATCHED"), "MATCHED": count("MATCHED"), "IGNORED": count("IGNORED") },
    }))
}

/// Accounts that have imported lines, with how many still wait to be matched.
pub async fn accounts(db: &Db) -> AppResult<Value> {
    let rows: Vec<(String, i64, i64)> = sqlx::query_as(
        r#"SELECT "accountId", count(*), sum("status" = 'UNMATCHED') FROM "bank_lines" GROUP BY "accountId""#,
    )
    .fetch_all(db.read())
    .await?;
    Ok(Value::Array(
        rows.into_iter()
            .map(|(id, total, open)| json!({ "accountId": id, "total": total, "unmatched": open }))
            .collect(),
    ))
}

async fn finish(mut tx: Tx, id: &str) -> AppResult<Value> {
    let line = find_one(&mut tx, id).await?;
    let mut v = to_json(&line);
    v["match"] = match_label(&mut tx, &line).await?;
    tx.commit().await?;
    Ok(v)
}

/// Matches the line to a payment or expense already recorded on its account.
pub async fn match_record(db: &Db, id: &str, match_type: &str, record_id: &str) -> AppResult<Value> {
    let mut tx = db.begin().await?;
    let line = unmatched(&mut tx, id).await?;
    let size = line.size()?;
    let foreign = account_currency(&mut tx, &line.account_id).await?.is_some();
    let (account, money, money_in, status, number) = match match_type {
        "PAYMENT" => {
            let p = payments::find_one(&mut tx, record_id).await?;
            (p.payment_account_id.clone(), payment_money(&p, foreign), payment_money_in(&p), p.status, p.payment_number)
        }
        "EXPENSE" => {
            let e = expenses::find_one(&mut tx, record_id).await?;
            (e.payment_account_id, e.amount, false, e.status, e.expense_number)
        }
        "TRANSFER" => {
            let found = transfers_near(&mut tx, &line, size).await?;
            if !found.iter().any(|(id, _, _)| id == record_id) {
                return Err(AppError::bad_request(
                    "That entry is not a transfer of this amount through this account within a week of the line",
                ));
            }
            if matched_elsewhere(&mut tx, "TRANSFER", record_id, &line.account_id).await? {
                return Err(AppError::bad_request("That transfer is already matched to another line on this account"));
            }
            set(&mut tx, id, "MATCHED", Some("TRANSFER"), Some(record_id)).await?;
            return finish(tx, id).await;
        }
        _ => return Err(AppError::bad_request("type must be PAYMENT, EXPENSE or TRANSFER")),
    };
    if status != "POSTED" {
        return Err(AppError::bad_request(format!("{number} is {status}; only a posted record can be matched")));
    }
    if account != line.account_id {
        return Err(AppError::bad_request(format!("{number} went through a different account")));
    }
    if money_in != line.money_in() || money != size {
        return Err(AppError::bad_request(format!(
            "{number} is for {}{}, not the {} {} on the statement",
            if money_in { "money in of " } else { "money out of " },
            money.to_db_string(),
            if line.money_in() { "money in of" } else { "money out of" },
            size.to_db_string()
        )));
    }
    if matched_elsewhere(&mut tx, match_type, record_id, &line.account_id).await? {
        return Err(AppError::bad_request(format!("{number} is already matched to another bank transaction")));
    }
    set(&mut tx, id, "MATCHED", Some(match_type), Some(record_id)).await?;
    finish(tx, id).await
}

/// Records a payment for an open invoice (money in) or bill (money out) from the line.
pub async fn pay(
    db: &Db,
    id: &str,
    invoice_id: Option<String>,
    bill_id: Option<String>,
    payment_method: String,
) -> AppResult<Value> {
    let mut tx = db.begin().await?;
    let line = unmatched(&mut tx, id).await?;
    let currency = account_currency(&mut tx, &line.account_id).await?;
    let (kind, customer_id, vendor_id, doc_currency) = match (&invoice_id, &bill_id, line.money_in()) {
        (Some(i), None, true) => {
            let inv = invoices::load_any(&mut tx, i).await?;
            ("AR", inv.customer_id.clone(), None, inv.currency)
        }
        (None, Some(b), false) => {
            let bill = bills::find_one(&mut tx, b).await?;
            ("AP", None, Some(bill.vendor_id.clone()), bill.currency)
        }
        (_, _, true) => return Err(AppError::bad_request("Money in is matched to an invoice")),
        (_, _, false) => return Err(AppError::bad_request("Money out is matched to a bill")),
    };
    if doc_currency != currency {
        let home = "the home currency".to_string();
        return Err(AppError::bad_request(format!(
            "The document is in {} and this account in {}. Record this payment on the Payments screen with its rate.",
            doc_currency.unwrap_or_else(|| home.clone()),
            currency.unwrap_or(home)
        )));
    }
    let p = payments::create_in(
        &mut tx,
        CreatePayment {
            kind: kind.into(),
            invoice_id,
            bill_id,
            customer_id,
            vendor_id,
            date: line.date.clone(),
            amount: line.size()?,
            payment_method,
            payment_account_id: line.account_id.clone(),
            notes: Some(format!("From the bank statement: {}", line.description)),
            withholding_tax_code_id: None,
            withholding_amount: Money::ZERO,
            exchange_rate: None,
            payable_account_id: None,
        },
    )
    .await?;
    set(&mut tx, id, "MATCHED", Some("PAYMENT"), Some(&p.id)).await?;
    finish(tx, id).await
}

/// Records and posts an expense from a money-out line.
pub async fn expense(
    db: &Db,
    id: &str,
    expense_account_id: String,
    tax_code_id: Option<String>,
    description: Option<String>,
) -> AppResult<Value> {
    let mut tx = db.begin().await?;
    let line = unmatched(&mut tx, id).await?;
    if line.money_in() {
        return Err(AppError::bad_request("Money in is not an expense"));
    }
    if account_currency(&mut tx, &line.account_id).await?.is_some() {
        return Err(AppError::bad_request(
            "Expenses are recorded in the home currency. Use a journal entry for this foreign-currency account.",
        ));
    }
    let e = expenses::create_in(
        &mut tx,
        CreateExpense {
            date: line.date.clone(),
            description: description.filter(|d| !d.trim().is_empty()).unwrap_or_else(|| line.description.clone()),
            amount: line.size()?,
            expense_account_id,
            payment_account_id: line.account_id.clone(),
            receipt_url: None,
            tax_code_id,
        },
    )
    .await?;
    expenses::post_in(&mut tx, &e.id).await?;
    set(&mut tx, id, "MATCHED", Some("EXPENSE"), Some(&e.id)).await?;
    finish(tx, id).await
}

/// D39: records the line as a transfer between this account and `other` (money in: from it;
/// money out: to it) — one balanced journal entry, matched to the line.
pub async fn transfer(db: &Db, id: &str, other: &str, note: Option<String>) -> AppResult<Value> {
    let mut tx = db.begin().await?;
    let line = unmatched(&mut tx, id).await?;
    let here = ledger::accounts::find_one(&mut tx, &line.account_id).await?;
    let there = ledger::accounts::find_one(&mut tx, other).await?;
    if here.id == there.id {
        return Err(AppError::bad_request("Choose the other account of the transfer"));
    }
    if here.currency.is_some() || there.currency.is_some() {
        return Err(AppError::bad_request(
            "Transfers with a foreign-currency account are entered as a journal entry with its rate",
        ));
    }
    let size = line.size()?;
    let (debit, credit) = if line.money_in() { (&here, &there) } else { (&there, &here) };
    let entry = entries::post(
        &mut tx,
        NewEntry {
            date: line.date.clone(),
            description: note
                .filter(|n| !n.trim().is_empty())
                .unwrap_or_else(|| format!("Transfer from {} to {}: {}", credit.name, debit.name, line.description)),
            reference: line.reference.clone(),
            source_type: Some("BANK_TRANSFER".into()),
            source_id: Some(line.id.clone()),
            lines: vec![
                NewLine { account_id: debit.id.clone(), debit: size, credit: Money::ZERO, description: None },
                NewLine { account_id: credit.id.clone(), debit: Money::ZERO, credit: size, description: None },
            ],
        },
    )
    .await?;
    set(&mut tx, id, "MATCHED", Some("TRANSFER"), Some(&entry.id)).await?;
    finish(tx, id).await
}

/// IGNORE, RESTORE (ignored → unmatched) or UNMATCH (matched → unmatched; the record stays).
pub async fn change(db: &Db, id: &str, action: &str) -> AppResult<Value> {
    let mut tx = db.begin().await?;
    let line = find_one(&mut tx, id).await?;
    let (from, to) = match action {
        "IGNORE" => ("UNMATCHED", "IGNORED"),
        "RESTORE" => ("IGNORED", "UNMATCHED"),
        "UNMATCH" => ("MATCHED", "UNMATCHED"),
        _ => return Err(AppError::internal("unknown bank line action")),
    };
    if line.status != from {
        return Err(AppError::bad_request(format!("This bank transaction is {}", line.status.to_lowercase())));
    }
    set(&mut tx, id, to, None, None).await?;
    finish(tx, id).await
}
