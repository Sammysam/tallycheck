//! D37: import from file. The screen reads the file ([`crate::sheet`]), the user maps its columns
//! to ours, and the mapped rows arrive here as text. One import runs in one transaction:
//!
//! * BANK — statement lines into `bank_lines`, to be matched on the Bank Transactions screen.
//! * INVOICES — DRAFT invoices (rows with the same number are one invoice); a customer not yet in
//!   the list is created.
//! * CUSTOMERS / VENDORS — new names only; a name already in the list is skipped.
//!
//! Nothing is posted: invoices stay drafts and bank lines wait to be matched by a person.
//! Every created record is listed in `import_items`; the key of each outside record (bank line,
//! invoice number) is unique, so importing the same file again skips what is already there.
//! A preview runs the very same code and rolls back. Rows with errors are reported and skipped;
//! each row (or invoice) runs inside its own savepoint so a failure leaves nothing behind.

use crate::invoices::{self, CreateInvoice, LineInput};
use crate::parties::{self, PartyFields, PartyKind};
use app_core::ids::new_id;
use app_core::money::Money;
use app_core::time::now_iso;
use app_core::{AppError, AppResult};
use chrono::NaiveDate;
use database::{Db, Tx};
use serde::Serialize;
use serde_json::Value;
use sqlx::{Row, SqliteConnection};
use std::collections::HashMap;

pub const MAX_ROWS: usize = 10_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Bank,
    Invoices,
    Customers,
    Vendors,
}

impl Kind {
    pub fn parse(s: &str) -> Option<Kind> {
        match s {
            "BANK" => Some(Kind::Bank),
            "INVOICES" => Some(Kind::Invoices),
            "CUSTOMERS" => Some(Kind::Customers),
            "VENDORS" => Some(Kind::Vendors),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Bank => "BANK",
            Kind::Invoices => "INVOICES",
            Kind::Customers => "CUSTOMERS",
            Kind::Vendors => "VENDORS",
        }
    }

    /// The permission an import of this kind needs (the same as creating the records by hand).
    pub fn permission(self) -> &'static str {
        match self {
            Kind::Bank => "banking",
            Kind::Invoices => "invoices",
            Kind::Customers => "customers",
            Kind::Vendors => "vendors",
        }
    }

    fn noun(self) -> &'static str {
        match self {
            Kind::Bank => "bank statement",
            Kind::Invoices => "invoices",
            Kind::Customers => "customers",
            Kind::Vendors => "vendors",
        }
    }
}

/// How day, month and year are ordered in the file's dates. ISO dates (2026-10-05) and dates
/// with a month name (5 Oct 2026) are read whatever the order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DateOrder {
    Dmy,
    Mdy,
    Ymd,
}

impl DateOrder {
    pub fn parse(s: &str) -> Option<DateOrder> {
        match s {
            "DMY" => Some(DateOrder::Dmy),
            "MDY" => Some(DateOrder::Mdy),
            "YMD" => Some(DateOrder::Ymd),
            _ => None,
        }
    }

    fn example(self) -> &'static str {
        match self {
            DateOrder::Dmy => "DD/MM/YYYY",
            DateOrder::Mdy => "MM/DD/YYYY",
            DateOrder::Ymd => "YYYY-MM-DD",
        }
    }
}

const MONTHS: [&str; 12] = ["jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec"];

fn month_name(t: &str) -> Option<u32> {
    let t = t.to_ascii_lowercase();
    if t.len() < 3 || !t.bytes().all(|b| b.is_ascii_alphabetic()) {
        return None;
    }
    MONTHS.iter().position(|m| t.starts_with(m)).map(|i| i as u32 + 1)
}

/// A date as `YYYY-MM-DD`, or None if the text is not a real date in the given order.
pub fn parse_date(text: &str, order: DateOrder) -> Option<String> {
    let text = text.trim();
    // "2026-10-05T00:00:00" → its date part.
    let text = match text.split_once('T') {
        Some((d, _)) if d.len() == 10 && d.as_bytes()[4] == b'-' => d,
        _ => text,
    };
    let cleaned: String = text.chars().map(|c| if matches!(c, '/' | '.' | '-' | ',') { ' ' } else { c }).collect();
    let tokens: Vec<&str> = cleaned.split_whitespace().take(3).collect();
    let [a, b, c] = tokens[..] else { return None };
    let num = |t: &str| -> Option<u32> { t.bytes().all(|x| x.is_ascii_digit()).then(|| t.parse().ok()).flatten() };
    let year = |t: &str| -> Option<i32> {
        let y: i32 = num(t)? as i32;
        match t.len() {
            4 => Some(y),
            2 => Some(2000 + y),
            _ => None,
        }
    };
    let (y, m, d) = if a.len() == 4 && num(a).is_some() {
        (year(a)?, month_name(b).or_else(|| num(b))?, num(c)?)
    } else if let Some(m) = month_name(b) {
        (year(c)?, m, num(a)?)
    } else if let Some(m) = month_name(a) {
        (year(c)?, m, num(b)?)
    } else {
        match order {
            DateOrder::Dmy => (year(c)?, num(b)?, num(a)?),
            DateOrder::Mdy => (year(c)?, num(a)?, num(b)?),
            DateOrder::Ymd => (year(a)?, num(b)?, num(c)?),
        }
    };
    NaiveDate::from_ymd_opt(y, m, d).map(|d| d.format("%Y-%m-%d").to_string())
}

/// Drops a currency label in front of an amount: "RS. 1,000" → "1,000", "$ 5" → "5". The dot of
/// an abbreviation goes with it; a dot with no label before it is a decimal point (".50").
fn strip_label(t: &str) -> &str {
    let rest = t.trim_start_matches(|c: char| c.is_alphabetic() || matches!(c, '$' | '£' | '€' | '¥' | '₹' | '₨'));
    let rest = if rest.len() != t.len() { rest.strip_prefix('.').unwrap_or(rest) } else { rest };
    rest.trim_start()
}

/// An amount as exact cents. Accepts thousands separators, a currency label ("Rs", "LKR", "$"),
/// a minus sign before or after, brackets for a negative, and "DR" / "CR" after it (DR = out).
/// Empty text is `Ok(None)`.
pub fn parse_amount(text: &str) -> Result<Option<Money>, String> {
    let mut t = text.trim().to_ascii_uppercase();
    if t.is_empty() {
        return Ok(None);
    }
    let original = text.trim().to_string();
    let mut negative = false;
    if let Some(rest) = t.strip_suffix("DR") {
        negative = true;
        t = rest.trim().to_string();
    } else if let Some(rest) = t.strip_suffix("CR") {
        t = rest.trim().to_string();
    }
    t = strip_label(&t).to_string();
    if t.starts_with('(') && t.ends_with(')') {
        negative = !negative;
        t = strip_label(&t[1..t.len() - 1]).to_string();
    }
    let kept: String = t.chars().filter(|c| c.is_ascii_digit() || matches!(c, '.' | '-' | '+')).collect();
    let mut body = kept.as_str();
    if let Some(rest) = body.strip_suffix('-') {
        negative = !negative;
        body = rest;
    }
    if let Some(rest) = body.strip_prefix('-') {
        negative = !negative;
        body = rest;
    } else if let Some(rest) = body.strip_prefix('+') {
        body = rest;
    }
    if body.is_empty() || body.contains(['-', '+']) {
        return Err(format!("\"{original}\" is not an amount"));
    }
    // "100.000" is fine; "100.005" is not a money amount.
    let body = match body.split_once('.') {
        Some((i, f)) if f.len() > 2 => {
            if f[2..].bytes().any(|b| b != b'0') {
                return Err(format!("\"{original}\" has more than 2 decimal places"));
            }
            format!("{i}.{}", &f[..2])
        }
        _ => body.to_string(),
    };
    let m = Money::parse_decimal(&body).ok_or_else(|| format!("\"{original}\" is not an amount"))?;
    if !m.fits_currency() {
        return Err(format!("\"{original}\" is too large"));
    }
    Ok(Some(if negative { Money::ZERO.checked_sub(m).unwrap_or(m) } else { m }))
}

pub struct Request {
    pub kind: Kind,
    pub source: String,
    pub file_name: Option<String>,
    /// BANK: the bank (or card / cash) account the statement belongs to.
    pub account_id: Option<String>,
    /// INVOICES: the revenue account for lines without one.
    pub default_account_id: Option<String>,
    pub date_order: DateOrder,
    /// Each row: our field name → the text in the mapped column.
    pub rows: Vec<HashMap<String, String>>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RowResult {
    /// 1-based position among the data rows.
    pub row: usize,
    /// NEW, DUPLICATE or ERROR.
    pub status: &'static str,
    pub label: String,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Outcome {
    /// The import's id once committed with something in it.
    pub id: Option<String>,
    /// A one-line summary (also the activity log's detail).
    pub name: String,
    pub created: i64,
    pub skipped: i64,
    pub errors: i64,
    pub rows: Vec<RowResult>,
}

fn field<'a>(row: &'a HashMap<String, String>, name: &str) -> &'a str {
    row.get(name).map(|s| s.trim()).unwrap_or("")
}

fn norm(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

/// "12,500.00" / "-250.00", for the preview's row labels.
fn money_text(m: Money) -> String {
    let plain = m.to_db_string();
    let (sign, digits) = plain.strip_prefix('-').map(|d| ("-", d)).unwrap_or(("", plain.as_str()));
    let (whole, frac) = digits.split_once('.').unwrap_or((digits, "00"));
    let mut grouped = String::new();
    for (i, c) in whole.chars().enumerate() {
        if i > 0 && (whole.len() - i) % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(c);
    }
    format!("{sign}{grouped}.{frac}")
}

async fn savepoint(tx: &mut Tx, sql: &str) -> AppResult<()> {
    sqlx::query(sql).execute(&mut **tx).await?;
    Ok(())
}

/// Runs `f` inside a savepoint: kept on success, undone on failure.
macro_rules! in_savepoint {
    ($tx:expr, $body:expr) => {{
        savepoint($tx, "SAVEPOINT import_row").await?;
        let r = $body;
        if r.is_ok() {
            savepoint($tx, "RELEASE import_row").await?;
        } else {
            savepoint($tx, "ROLLBACK TO import_row").await?;
            savepoint($tx, "RELEASE import_row").await?;
        }
        r
    }};
}

fn record_table(record_type: &str) -> &'static str {
    match record_type {
        "BANK_LINE" => "bank_lines",
        "INVOICE" => "invoices",
        "CUSTOMER" => "customers",
        _ => "vendors",
    }
}

/// The record an outside key was imported as, if it still exists. A key whose record has since
/// been deleted (a draft invoice removed by hand) is forgotten, so the row can come in again.
async fn already_imported(conn: &mut SqliteConnection, kind: Kind, key: &str) -> AppResult<Option<String>> {
    let row: Option<(String, String, String)> = sqlx::query_as(
        r#"SELECT "id", "recordType", "recordId" FROM "import_items" WHERE "kind" = ?1 AND "externalKey" = ?2"#,
    )
    .bind(kind.as_str())
    .bind(key)
    .fetch_optional(&mut *conn)
    .await?;
    let Some((item_id, record_type, record_id)) = row else { return Ok(None) };
    let exists: Option<i64> =
        sqlx::query_scalar(&format!(r#"SELECT 1 FROM "{}" WHERE "id" = ?1"#, record_table(&record_type)))
            .bind(&record_id)
            .fetch_optional(&mut *conn)
            .await?;
    if exists.is_some() {
        return Ok(Some(record_id));
    }
    sqlx::query(r#"DELETE FROM "import_items" WHERE "id" = ?1"#).bind(&item_id).execute(&mut *conn).await?;
    Ok(None)
}

async fn remember(
    tx: &mut Tx,
    import_id: &str,
    kind: Kind,
    key: &str,
    record_type: &str,
    record_id: &str,
) -> AppResult<()> {
    sqlx::query(
        r#"INSERT INTO "import_items" ("id","importId","kind","externalKey","recordType","recordId") VALUES (?1,?2,?3,?4,?5,?6)"#,
    )
    .bind(new_id())
    .bind(import_id)
    .bind(kind.as_str())
    .bind(key)
    .bind(record_type)
    .bind(record_id)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

pub async fn run(db: &Db, req: Request, created_by: Option<&str>, commit: bool) -> AppResult<Outcome> {
    if req.rows.is_empty() {
        return Err(AppError::bad_request("The file has no rows to import"));
    }
    if req.rows.len() > MAX_ROWS {
        return Err(AppError::bad_request(format!("Import at most {MAX_ROWS} rows at a time")));
    }
    let source = req.source.trim();
    if source.is_empty() {
        return Err(AppError::bad_request("Name where this file comes from (for example the bank's name)"));
    }
    let mut tx = db.begin().await?;
    let id = new_id();
    let now = now_iso();
    let account_id = match req.kind {
        Kind::Bank => {
            let a = req
                .account_id
                .as_deref()
                .filter(|a| !a.is_empty())
                .ok_or_else(|| AppError::bad_request("Choose the bank account this statement belongs to"))?;
            let account = ledger::accounts::find_one(&mut tx, a).await?;
            if !account.is_active {
                return Err(AppError::bad_request(format!("{} {} is inactive", account.code, account.name)));
            }
            Some(account.id)
        }
        _ => None,
    };
    sqlx::query(
        r#"INSERT INTO "imports" ("id","kind","source","fileName","accountId","createdById","createdAt","updatedAt")
           VALUES (?1,?2,?3,?4,?5,?6,?7,?7)"#,
    )
    .bind(&id)
    .bind(req.kind.as_str())
    .bind(source)
    .bind(req.file_name.as_deref().filter(|f| !f.is_empty()))
    .bind(&account_id)
    .bind(created_by)
    .bind(&now)
    .execute(&mut *tx)
    .await?;

    let rows = match req.kind {
        Kind::Bank => bank(&mut tx, &id, account_id.as_deref().unwrap_or_default(), &req).await?,
        Kind::Invoices => invoices(&mut tx, &id, &req, created_by).await?,
        Kind::Customers => parties(&mut tx, &id, PartyKind::Customer, &req).await?,
        Kind::Vendors => parties(&mut tx, &id, PartyKind::Vendor, &req).await?,
    };
    let count = |s: &str| rows.iter().filter(|r| r.status == s).count() as i64;
    let (created, skipped, errors) = (count("NEW"), count("DUPLICATE"), count("ERROR"));
    let mut name = format!("{}: {created} new", req.kind.noun());
    if skipped > 0 {
        name.push_str(&format!(", {skipped} already there"));
    }
    if errors > 0 {
        name.push_str(&format!(", {errors} with errors"));
    }
    name = name[..1].to_uppercase() + &name[1..];
    sqlx::query(r#"UPDATE "imports" SET "created" = ?1, "skipped" = ?2 WHERE "id" = ?3"#)
        .bind(created)
        .bind(skipped)
        .bind(&id)
        .execute(&mut *tx)
        .await?;
    let keep = commit && created > 0;
    if keep {
        tx.commit().await?;
    } else {
        tx.rollback().await?;
    }
    Ok(Outcome { id: keep.then_some(id), name, created, skipped, errors, rows })
}

/// The message for a row that failed; an unexpected (internal) failure is logged as well.
fn user_message(e: &AppError) -> String {
    if let AppError::Internal(detail) = e {
        tracing::error!("import row failed: {detail}");
    }
    e.user_message()
}

fn error(row: usize, label: String, message: String) -> RowResult {
    RowResult { row, status: "ERROR", label, message: Some(message) }
}

async fn bank(tx: &mut Tx, import_id: &str, account_id: &str, req: &Request) -> AppResult<Vec<RowResult>> {
    let mut out = Vec::new();
    // Two identical lines on one statement (two coffees on the same day) are both real: the
    // key counts repeats, so the same file imported again matches line for line.
    let mut seen: HashMap<String, usize> = HashMap::new();
    for (i, r) in req.rows.iter().enumerate() {
        let n = i + 1;
        let (date_text, reference) = (field(r, "date"), field(r, "reference"));
        let description = match field(r, "description") {
            "" => reference,
            d => d,
        };
        let label = format!("{date_text} · {description}");
        let Some(date) = parse_date(date_text, req.date_order) else {
            out.push(error(
                n,
                label,
                if date_text.is_empty() {
                    "No date".into()
                } else {
                    format!("\"{date_text}\" is not a date ({})", req.date_order.example())
                },
            ));
            continue;
        };
        if description.is_empty() {
            out.push(error(n, label, "No description".into()));
            continue;
        }
        let amount = if !field(r, "amount").is_empty() {
            parse_amount(field(r, "amount"))
        } else {
            match (parse_amount(field(r, "moneyIn")), parse_amount(field(r, "moneyOut"))) {
                (Err(e), _) | (_, Err(e)) => Err(e),
                (Ok(a), Ok(b)) => {
                    let (a, b) = (a.unwrap_or(Money::ZERO), b.unwrap_or(Money::ZERO));
                    // "Money out" columns are sometimes written negative already.
                    let out_abs = if b < Money::ZERO { Money::ZERO.checked_sub(b).unwrap_or(b) } else { b };
                    Ok(a.checked_sub(out_abs))
                }
            }
        };
        let amount = match amount {
            Err(e) => {
                out.push(error(n, label, e));
                continue;
            }
            Ok(None) | Ok(Some(Money::ZERO)) => {
                out.push(error(n, label, "No amount".into()));
                continue;
            }
            Ok(Some(a)) => a,
        };
        let label = format!("{date} · {description} · {}", money_text(amount));
        let base = format!("{account_id}|{date}|{}|{}|{}", amount.cents(), norm(description), norm(reference));
        let occurrence = seen.entry(base.clone()).or_insert(0);
        *occurrence += 1;
        let key = format!("{base}|{occurrence}");
        if already_imported(tx, Kind::Bank, &key).await?.is_some() {
            out.push(RowResult { row: n, status: "DUPLICATE", label, message: Some("Already imported".into()) });
            continue;
        }
        let line_id = new_id();
        let now = now_iso();
        let saved: AppResult<()> = in_savepoint!(tx, {
            async {
                sqlx::query(
                    r#"INSERT INTO "bank_lines" ("id","importId","accountId","date","description","reference","amount","createdAt","updatedAt")
                       VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?8)"#,
                )
                .bind(&line_id)
                .bind(import_id)
                .bind(account_id)
                .bind(&date)
                .bind(description)
                .bind((!reference.is_empty()).then_some(reference))
                .bind(amount.cents())
                .bind(&now)
                .execute(&mut **tx)
                .await?;
                remember(tx, import_id, Kind::Bank, &key, "BANK_LINE", &line_id).await
            }
            .await
        });
        out.push(match saved {
            Ok(()) => RowResult { row: n, status: "NEW", label, message: None },
            Err(e) => error(n, label, user_message(&e)),
        });
    }
    Ok(out)
}

async fn parties(tx: &mut Tx, import_id: &str, kind: PartyKind, req: &Request) -> AppResult<Vec<RowResult>> {
    let (table, list, record_type) = match kind {
        PartyKind::Customer => ("customers", "Customers", "CUSTOMER"),
        PartyKind::Vendor => ("vendors", "Vendors", "VENDOR"),
    };
    let mut out = Vec::new();
    for (i, r) in req.rows.iter().enumerate() {
        let n = i + 1;
        let name = field(r, "name");
        let label = name.to_string();
        if name.is_empty() {
            out.push(error(n, "(no name)".into(), "No name".into()));
            continue;
        }
        let email = field(r, "email");
        if !email.is_empty() && !(email.contains('@') && email.contains('.') && !email.contains(' ')) {
            out.push(error(n, label, format!("\"{email}\" is not an email address")));
            continue;
        }
        // An earlier row of this file is already inserted, so it counts here too.
        let exists: Option<i64> = sqlx::query_scalar(&format!(
            r#"SELECT 1 FROM "{table}" WHERE lower(trim("name")) = lower(trim(?1)) LIMIT 1"#
        ))
        .bind(name)
        .fetch_optional(&mut **tx)
        .await?;
        if exists.is_some() {
            out.push(RowResult { row: n, status: "DUPLICATE", label, message: Some(format!("Already in {list}")) });
            continue;
        }
        let opt = |f: &str| -> Option<Option<String>> {
            let v = field(r, f);
            Some((!v.is_empty()).then(|| v.to_string()))
        };
        let fields = PartyFields {
            name: Some(name.to_string()),
            email: opt("email"),
            phone: opt("phone"),
            address: opt("address"),
            tax_id: opt("taxId"),
        };
        let saved: AppResult<String> = in_savepoint!(tx, {
            async {
                let p = parties::create_in(tx, kind, fields).await?;
                remember(tx, import_id, req.kind, &new_id(), record_type, &p.id).await?;
                Ok(p.id)
            }
            .await
        });
        out.push(match saved {
            Ok(_) => RowResult { row: n, status: "NEW", label, message: None },
            Err(e) => error(n, label, user_message(&e)),
        });
    }
    Ok(out)
}

async fn find_by_name(conn: &mut SqliteConnection, sql: &str, name: &str) -> AppResult<Option<String>> {
    Ok(sqlx::query_scalar(sql).bind(name).fetch_optional(conn).await?)
}

async fn invoices(tx: &mut Tx, import_id: &str, req: &Request, created_by: Option<&str>) -> AppResult<Vec<RowResult>> {
    // Rows with the same invoice number are lines of one invoice; without a number, each row is
    // an invoice of its own. The file's order is kept.
    let mut groups: Vec<(String, Vec<usize>)> = Vec::new();
    let mut index: HashMap<String, usize> = HashMap::new();
    for (i, r) in req.rows.iter().enumerate() {
        let number = field(r, "number");
        let key = if number.is_empty() { format!("row:{i}") } else { format!("no:{}", norm(number)) };
        match index.get(&key) {
            Some(&g) => groups[g].1.push(i),
            None => {
                index.insert(key.clone(), groups.len());
                groups.push((key, vec![i]));
            }
        }
    }
    let mut out = Vec::new();
    for (key, rows) in groups {
        let first = &req.rows[rows[0]];
        let n = rows[0] + 1;
        let (number, customer) = (field(first, "number"), field(first, "customer"));
        let label = format!(
            "{}{customer} · {} line{}",
            if number.is_empty() { String::new() } else { format!("{number} · ") },
            rows.len(),
            if rows.len() == 1 { "" } else { "s" }
        );
        match invoice(tx, import_id, req, &key, &rows, created_by).await {
            Ok(Some((invoice_number, new_customer))) => out.push(RowResult {
                row: n,
                status: "NEW",
                label,
                message: Some(format!(
                    "Draft {invoice_number}{}",
                    if new_customer { format!(", new customer \"{customer}\"") } else { String::new() }
                )),
            }),
            Ok(None) => out.push(RowResult {
                row: n,
                status: "DUPLICATE",
                label,
                message: Some(format!("Invoice {number} was imported before")),
            }),
            Err(e) => out.push(error(n, label, user_message(&e))),
        }
    }
    Ok(out)
}

/// One invoice from its rows: `Ok(None)` when it was imported before, otherwise its number and
/// whether its customer was created.
async fn invoice(
    tx: &mut Tx,
    import_id: &str,
    req: &Request,
    key: &str,
    rows: &[usize],
    created_by: Option<&str>,
) -> AppResult<Option<(String, bool)>> {
    let first = &req.rows[rows[0]];
    let bad = |m: String| AppError::bad_request(m);
    if !key.starts_with("row:") && already_imported(tx, Kind::Invoices, key).await?.is_some() {
        return Ok(None);
    }
    let customer = field(first, "customer");
    if customer.is_empty() {
        return Err(bad("No customer".into()));
    }
    let date_text = field(first, "date");
    let date = parse_date(date_text, req.date_order).ok_or_else(|| {
        bad(if date_text.is_empty() {
            "No date".into()
        } else {
            format!("\"{date_text}\" is not a date ({})", req.date_order.example())
        })
    })?;
    let due_text = field(first, "dueDate");
    let due_date = match due_text {
        "" => None,
        t => Some(
            parse_date(t, req.date_order)
                .ok_or_else(|| bad(format!("Due date \"{t}\" is not a date ({})", req.date_order.example())))?,
        ),
    };
    let mut lines = Vec::new();
    for &i in rows {
        let r = &req.rows[i];
        let at = if rows.len() > 1 { format!("Row {}: ", i + 1) } else { String::new() };
        let description = field(r, "description");
        if description.is_empty() {
            return Err(bad(format!("{at}No description")));
        }
        let quantity = parse_amount(field(r, "quantity")).map_err(|e| bad(format!("{at}Quantity {e}")))?;
        if quantity.is_some_and(|q| q <= Money::ZERO) {
            return Err(bad(format!("{at}Quantity must be more than 0")));
        }
        let unit_price = parse_amount(field(r, "unitPrice"))
            .map_err(|e| bad(format!("{at}Price {e}")))?
            .ok_or_else(|| bad(format!("{at}No price")))?;
        let tax_code_id = match field(r, "taxCode") {
            "" => None,
            t => Some(
                find_by_name(
                    tx,
                    r#"SELECT "id" FROM "tax_codes" WHERE lower(trim("name")) = lower(trim(?1)) AND "isActive" = 1"#,
                    t,
                )
                .await?
                .ok_or_else(|| bad(format!("{at}No active tax code named \"{t}\"")))?,
            ),
        };
        let account_id = match field(r, "account") {
            "" => req.default_account_id.clone().filter(|a| !a.is_empty()),
            code => {
                let code = code.split_whitespace().next().unwrap_or(code);
                Some(
                    find_by_name(tx, r#"SELECT "id" FROM "accounts" WHERE "code" = ?1"#, code)
                        .await?
                        .ok_or_else(|| bad(format!("{at}No account with code {code}")))?,
                )
            }
        };
        lines.push(LineInput {
            description: description.to_string(),
            quantity,
            unit_price: Some(unit_price),
            account_id,
            tax_code_id,
        });
    }
    in_savepoint!(tx, {
        async {
            let existing = find_by_name(
                tx,
                r#"SELECT "id" FROM "customers" WHERE lower(trim("name")) = lower(trim(?1)) ORDER BY "isActive" DESC LIMIT 1"#,
                customer,
            )
            .await?;
            let new_customer = existing.is_none();
            let customer_id = match existing {
                Some(id) => id,
                None => {
                    let p = parties::create_in(
                        tx,
                        PartyKind::Customer,
                        PartyFields {
                            name: Some(customer.to_string()),
                            email: None,
                            phone: None,
                            address: None,
                            tax_id: None,
                        },
                    )
                    .await?;
                    remember(tx, import_id, Kind::Invoices, &new_id(), "CUSTOMER", &p.id).await?;
                    p.id
                }
            };
            let inv = invoices::create_in(
                tx,
                CreateInvoice {
                    customer_id: Some(customer_id),
                    date,
                    due_date,
                    lines,
                    ..Default::default()
                },
                created_by,
            )
            .await?;
            let item_key = if key.starts_with("row:") { new_id() } else { key.to_string() };
            remember(tx, import_id, Kind::Invoices, &item_key, "INVOICE", &inv.id).await?;
            Ok(Some((inv.invoice_number, new_customer)))
        }
        .await
    })
}

// ─── History and undo ───────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportRecord {
    pub id: String,
    pub kind: String,
    pub source: String,
    pub file_name: Option<String>,
    pub account_id: Option<String>,
    pub account_name: Option<String>,
    pub created: i64,
    pub skipped: i64,
    pub status: String,
    pub created_by: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    /// For the activity log's detail.
    pub name: String,
}

const RECORD_SQL: &str = r#"SELECT i."id", i."kind", i."source", i."fileName", i."accountId",
       CASE WHEN a."id" IS NULL THEN NULL ELSE a."code" || ' ' || a."name" END AS "accountName",
       i."created", i."skipped", i."status", u."username", i."createdAt", i."updatedAt"
  FROM "imports" i LEFT JOIN "accounts" a ON a."id" = i."accountId" LEFT JOIN "users" u ON u."id" = i."createdById""#;

fn record(r: &sqlx::sqlite::SqliteRow) -> AppResult<ImportRecord> {
    let kind: String = r.try_get("kind")?;
    let source: String = r.try_get("source")?;
    let noun = Kind::parse(&kind).map(Kind::noun).unwrap_or("records");
    Ok(ImportRecord {
        name: format!("{source} ({noun})"),
        id: r.try_get("id")?,
        kind,
        source,
        file_name: r.try_get("fileName")?,
        account_id: r.try_get("accountId")?,
        account_name: r.try_get("accountName")?,
        created: r.try_get("created")?,
        skipped: r.try_get("skipped")?,
        status: r.try_get("status")?,
        created_by: r.try_get("username")?,
        created_at: r.try_get("createdAt")?,
        updated_at: r.try_get("updatedAt")?,
    })
}

pub async fn list(db: &Db) -> AppResult<Vec<ImportRecord>> {
    let rows =
        sqlx::query(&format!(r#"{RECORD_SQL} ORDER BY i."createdAt" DESC LIMIT 100"#)).fetch_all(db.read()).await?;
    rows.iter().map(record).collect()
}

pub async fn find_one(conn: &mut SqliteConnection, id: &str) -> AppResult<ImportRecord> {
    let row = sqlx::query(&format!(r#"{RECORD_SQL} WHERE i."id" = ?1"#)).bind(id).fetch_optional(conn).await?;
    match row {
        Some(r) => record(&r),
        None => Err(AppError::NotFound(format!("Import with ID {id} not found"))),
    }
}

/// Removes what an import created, as long as nobody has used it yet: bank lines not matched,
/// invoices still drafts, customers / vendors on no document.
pub async fn undo(db: &Db, id: &str) -> AppResult<ImportRecord> {
    let mut tx = db.begin().await?;
    let imp = find_one(&mut tx, id).await?;
    if imp.status == "UNDONE" {
        return Err(AppError::bad_request("This import was already undone"));
    }
    let items: Vec<(String, String)> =
        sqlx::query_as(r#"SELECT "recordType", "recordId" FROM "import_items" WHERE "importId" = ?1"#)
            .bind(id)
            .fetch_all(&mut *tx)
            .await?;
    let ids = |t: &str| items.iter().filter(|(rt, _)| rt == t).map(|(_, id)| id.clone()).collect::<Vec<_>>();

    let mut matched = 0;
    for line in ids("BANK_LINE") {
        let status: Option<String> = sqlx::query_scalar(r#"SELECT "status" FROM "bank_lines" WHERE "id" = ?1"#)
            .bind(&line)
            .fetch_optional(&mut *tx)
            .await?;
        if status.as_deref() == Some("MATCHED") {
            matched += 1;
            continue;
        }
        sqlx::query(r#"DELETE FROM "bank_lines" WHERE "id" = ?1"#).bind(&line).execute(&mut *tx).await?;
    }
    if matched > 0 {
        return Err(AppError::bad_request(format!(
            "{matched} of its bank transactions {} matched. Unmatch {} on the Bank Transactions screen first.",
            if matched == 1 { "is" } else { "are" },
            if matched == 1 { "it" } else { "them" }
        )));
    }
    for inv_id in ids("INVOICE") {
        let Some(inv) = invoices::load(&mut tx, &inv_id).await? else { continue };
        if inv.status != invoices::DRAFT {
            return Err(AppError::bad_request(format!(
                "Invoice {} is no longer a draft ({}). Void it on the Invoices screen instead.",
                inv.invoice_number, inv.status
            )));
        }
        let lines: Vec<String> = inv.lines.iter().map(|l| l.id.clone()).collect();
        crate::tax::delete_parts(&mut tx, "INVOICE_LINE", &lines).await?;
        sqlx::query(r#"DELETE FROM "invoices" WHERE "id" = ?1"#).bind(&inv_id).execute(&mut *tx).await?;
    }
    for (record_type, table, label) in [("CUSTOMER", "customers", "Customer"), ("VENDOR", "vendors", "Vendor")] {
        for party in ids(record_type) {
            let res =
                sqlx::query(&format!(r#"DELETE FROM "{table}" WHERE "id" = ?1"#)).bind(&party).execute(&mut *tx).await;
            if let Err(sqlx::Error::Database(e)) = &res {
                if e.is_foreign_key_violation() {
                    let name: String = sqlx::query_scalar(&format!(r#"SELECT "name" FROM "{table}" WHERE "id" = ?1"#))
                        .bind(&party)
                        .fetch_one(&mut *tx)
                        .await?;
                    return Err(AppError::bad_request(format!(
                        "{label} \"{name}\" is now used on other records, so this import cannot be undone"
                    )));
                }
            }
            res?;
        }
    }
    sqlx::query(r#"DELETE FROM "import_items" WHERE "importId" = ?1"#).bind(id).execute(&mut *tx).await?;
    sqlx::query(r#"UPDATE "imports" SET "status" = 'UNDONE', "updatedAt" = ?1 WHERE "id" = ?2"#)
        .bind(now_iso())
        .bind(id)
        .execute(&mut *tx)
        .await?;
    let out = find_one(&mut tx, id).await?;
    tx.commit().await?;
    Ok(out)
}

// ─── Saved column layouts ───────────────────────────────────────────────────

pub const LAYOUT_KINDS: &[&str] = &["BANK", "INVOICES", "CUSTOMERS", "VENDORS", "OPENING"];

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Layout {
    pub id: String,
    pub kind: String,
    pub name: String,
    pub layout: Value,
    pub updated_at: String,
}

pub async fn layouts(db: &Db, kind: Option<&str>) -> AppResult<Vec<Layout>> {
    let rows: Vec<(String, String, String, String, String)> = sqlx::query_as(
        r#"SELECT "id","kind","name","layout","updatedAt" FROM "import_layouts"
           WHERE ?1 IS NULL OR "kind" = ?1 ORDER BY "name" COLLATE NOCASE"#,
    )
    .bind(kind)
    .fetch_all(db.read())
    .await?;
    rows.into_iter()
        .map(|(id, kind, name, layout, updated_at)| {
            Ok(Layout { id, kind, name, layout: serde_json::from_str(&layout).unwrap_or(Value::Null), updated_at })
        })
        .collect()
}

/// Saves a layout under its name, replacing one of the same kind and name.
pub async fn save_layout(db: &Db, kind: &str, name: &str, layout: &Value) -> AppResult<Layout> {
    let mut tx = db.begin().await?;
    let now = now_iso();
    sqlx::query(
        r#"INSERT INTO "import_layouts" ("id","kind","name","layout","updatedAt") VALUES (?1,?2,?3,?4,?5)
           ON CONFLICT ("kind","name") DO UPDATE SET "layout" = excluded."layout", "updatedAt" = excluded."updatedAt""#,
    )
    .bind(new_id())
    .bind(kind)
    .bind(name)
    .bind(layout.to_string())
    .bind(&now)
    .execute(&mut *tx)
    .await?;
    let (id, updated_at): (String, String) =
        sqlx::query_as(r#"SELECT "id","updatedAt" FROM "import_layouts" WHERE "kind" = ?1 AND "name" = ?2"#)
            .bind(kind)
            .bind(name)
            .fetch_one(&mut *tx)
            .await?;
    tx.commit().await?;
    Ok(Layout { id, kind: kind.into(), name: name.into(), layout: layout.clone(), updated_at })
}

pub async fn delete_layout(db: &Db, id: &str) -> AppResult<()> {
    let done = sqlx::query(r#"DELETE FROM "import_layouts" WHERE "id" = ?1"#).bind(id).execute(db.write()).await?;
    if done.rows_affected() == 0 {
        return Err(AppError::NotFound(format!("Layout with ID {id} not found")));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_in_every_common_form() {
        let d = |s, o| parse_date(s, o);
        assert_eq!(d("05/10/2026", DateOrder::Dmy).as_deref(), Some("2026-10-05"));
        assert_eq!(d("10/05/2026", DateOrder::Mdy).as_deref(), Some("2026-10-05"));
        assert_eq!(d("2026-10-05", DateOrder::Dmy).as_deref(), Some("2026-10-05"));
        assert_eq!(d("2026-10-05T00:00:00", DateOrder::Mdy).as_deref(), Some("2026-10-05"));
        assert_eq!(d("5 Oct 2026", DateOrder::Mdy).as_deref(), Some("2026-10-05"));
        assert_eq!(d("05-OCT-26", DateOrder::Dmy).as_deref(), Some("2026-10-05"));
        assert_eq!(d("Oct 5, 2026", DateOrder::Dmy).as_deref(), Some("2026-10-05"));
        assert_eq!(d("05.10.2026 14:30", DateOrder::Dmy).as_deref(), Some("2026-10-05"));
        assert_eq!(d("31/02/2026", DateOrder::Dmy), None);
        assert_eq!(d("13/13/2026", DateOrder::Mdy), None);
        assert_eq!(d("yesterday", DateOrder::Dmy), None);
        assert_eq!(d("", DateOrder::Dmy), None);
    }

    #[test]
    fn amounts_as_banks_write_them() {
        let a = |s: &str| parse_amount(s).map(|m| m.map(|m| m.cents()));
        assert_eq!(a("1,234.50"), Ok(Some(123450)));
        assert_eq!(a("-50,000.00"), Ok(Some(-5000000)));
        assert_eq!(a("(250.00)"), Ok(Some(-25000)));
        assert_eq!(a("Rs. 1,000"), Ok(Some(100000)));
        assert_eq!(a("LKR 18337.13"), Ok(Some(1833713)));
        assert_eq!(a("1,500.00 DR"), Ok(Some(-150000)));
        assert_eq!(a("1,500.00 CR"), Ok(Some(150000)));
        assert_eq!(a("75.5-"), Ok(Some(-7550)));
        assert_eq!(a("100.000"), Ok(Some(10000)));
        assert_eq!(a(""), Ok(None));
        assert!(a("100.005").is_err());
        assert!(a("abc").is_err());
        assert!(a("1-2").is_err());
    }
}
