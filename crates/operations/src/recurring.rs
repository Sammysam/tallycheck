//! D61: recurring invoices. "Make recurring" copies an invoice into a schedule (every N weeks,
//! months, quarters or years from a start date, optionally until an end date). While the app
//! runs, `run_due` makes a DRAFT invoice for every date that has come (dates missed while the
//! app was closed are made one by one, each with its own date); a person checks and posts them.
//! Every made invoice is listed in `recurring_invoice_items` (one per schedule and date), so a
//! date is never made twice. Visibility follows the invoice rules.

use crate::invoices::{self, CreateInvoice, LineInput, Scope};
use app_core::ids::new_id;
use app_core::money::Money;
use app_core::time::now_iso;
use app_core::{AppError, AppResult};
use chrono::{Days, Months, NaiveDate};
use database::Db;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::sqlite::SqliteRow;
use sqlx::{Row, SqliteConnection};

pub const FREQUENCIES: &[&str] = &["WEEKLY", "MONTHLY", "QUARTERLY", "YEARLY"];
/// At most this many invoices per schedule in one run, so a wrong start date years back cannot
/// flood the books; the rest follow on the next run.
const MAX_PER_RUN: usize = 60;

/// What a recurring invoice copies from its invoice. Money in cents.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Template {
    pub customer_id: Option<String>,
    pub bill_to_name: Option<String>,
    pub bill_to_address: Option<String>,
    pub payment_terms: Option<String>,
    /// Days from the invoice date to its due date, when the invoice had a due date but no terms.
    pub due_days: Option<i64>,
    pub notes: Option<String>,
    pub payment_options: Option<String>,
    pub signatory_name: Option<String>,
    pub signatory_title: Option<String>,
    pub prices_include_tax: bool,
    pub currency: Option<String>,
    pub exchange_rate: i64,
    pub lines: Vec<TemplateLine>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TemplateLine {
    pub description: String,
    pub quantity: i64,
    pub unit_price: i64,
    pub account_id: Option<String>,
    pub tax_code_id: Option<String>,
    pub item_id: Option<String>,
    pub discount: i64,
}

impl Template {
    fn from_invoice(inv: &invoices::Invoice) -> Template {
        let due_days = match (&inv.payment_terms, &inv.due_date) {
            (None, Some(due)) => days_between(&inv.date, due),
            _ => None,
        };
        Template {
            customer_id: inv.customer_id.clone(),
            bill_to_name: inv.bill_to_name.clone().or_else(|| inv.customer.as_ref().map(|c| c.name.clone())),
            bill_to_address: inv
                .bill_to_address
                .clone()
                .or_else(|| inv.customer.as_ref().and_then(|c| c.address.clone())),
            payment_terms: inv.payment_terms.clone(),
            due_days,
            notes: inv.notes.clone(),
            payment_options: inv.payment_options.clone(),
            signatory_name: inv.signatory_name.clone(),
            signatory_title: inv.signatory_title.clone(),
            prices_include_tax: inv.prices_include_tax,
            currency: inv.currency.clone(),
            exchange_rate: inv.exchange_rate,
            lines: inv
                .lines
                .iter()
                .map(|l| TemplateLine {
                    description: l.description.clone(),
                    quantity: l.quantity.cents(),
                    unit_price: l.unit_price.cents(),
                    account_id: l.account_id.clone(),
                    tax_code_id: l.tax_code_id.clone(),
                    item_id: l.item_id.clone(),
                    discount: l.discount_amount.cents(),
                })
                .collect(),
        }
    }

    /// The invoice to create for `date`.
    fn invoice_for(&self, date: &str) -> CreateInvoice {
        let due_date = self.due_days.and_then(|d| add_days(date, d));
        CreateInvoice {
            customer_id: self.customer_id.clone(),
            date: date.to_string(),
            due_date,
            payment_terms: self.payment_terms.clone(),
            po_number: None,
            notes: self.notes.clone(),
            bill_to_name: self.bill_to_name.clone(),
            bill_to_address: self.bill_to_address.clone(),
            payment_options: self.payment_options.as_ref().map(|p| p.split('\n').map(str::to_string).collect()),
            signatory_name: self.signatory_name.clone(),
            signatory_title: self.signatory_title.clone(),
            prices_include_tax: self.prices_include_tax,
            currency: self.currency.clone(),
            exchange_rate: Some(self.exchange_rate),
            lines: self
                .lines
                .iter()
                .map(|l| LineInput {
                    description: l.description.clone(),
                    quantity: Some(Money::from_cents(l.quantity)),
                    unit_price: Some(Money::from_cents(l.unit_price)),
                    account_id: l.account_id.clone(),
                    tax_code_id: l.tax_code_id.clone(),
                    item_id: l.item_id.clone(),
                    discount: Money::from_cents(l.discount),
                })
                .collect(),
        }
    }

    /// The total of one invoice made from it, worked out as an invoice would (tax included).
    fn total_hint(&self) -> i64 {
        self.lines
            .iter()
            .map(|l| {
                Money::line_amount(Money::from_cents(l.quantity), Money::from_cents(l.unit_price))
                    .map(|m| m.cents() - l.discount)
                    .unwrap_or(0)
            })
            .sum()
    }
}

fn parse(d: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(d, "%Y-%m-%d").ok()
}

fn days_between(from: &str, to: &str) -> Option<i64> {
    Some((parse(to)? - parse(from)?).num_days()).filter(|d| *d >= 0)
}

fn add_days(date: &str, days: i64) -> Option<String> {
    let d = parse(date)?.checked_add_days(Days::new(u64::try_from(days).ok()?))?;
    Some(d.format("%Y-%m-%d").to_string())
}

/// Date number `index` of a schedule, counted from its start date: months are added to the
/// start date itself (clamped to the month's last day), so 31 Jan monthly gives 28 Feb, 31 Mar.
pub fn nth_date(start: &str, frequency: &str, every: i64, index: i64) -> Option<String> {
    let start = parse(start)?;
    let steps = u32::try_from(every.checked_mul(index)?).ok()?;
    let d = match frequency {
        "WEEKLY" => start.checked_add_days(Days::new(u64::from(steps) * 7))?,
        "MONTHLY" => start.checked_add_months(Months::new(steps))?,
        "QUARTERLY" => start.checked_add_months(Months::new(steps.checked_mul(3)?))?,
        "YEARLY" => start.checked_add_months(Months::new(steps.checked_mul(12)?))?,
        _ => return None,
    };
    Some(d.format("%Y-%m-%d").to_string())
}

#[derive(Debug, Clone)]
pub struct Recurring {
    pub id: String,
    pub name: String,
    pub source_invoice_id: Option<String>,
    pub template: Template,
    pub frequency: String,
    pub every: i64,
    pub start_date: String,
    pub end_date: Option<String>,
    pub next_date: Option<String>,
    pub date_index: i64,
    pub status: String,
    pub last_error: Option<String>,
    pub last_run_at: Option<String>,
    pub created_by_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    /// Invoices it has made, and how many of them are still drafts.
    pub made: i64,
    pub drafts: i64,
    pub customer_name: Option<String>,
}

impl Recurring {
    pub fn to_json(&self) -> Value {
        json!({
            "id": self.id,
            "name": self.name,
            "sourceInvoiceId": self.source_invoice_id,
            "customerId": self.template.customer_id,
            "customerName": self.customer_name.clone().or_else(|| self.template.bill_to_name.clone()),
            "currency": self.template.currency,
            "amount": crate::money_str(Money::from_cents(self.template.total_hint())),
            "pricesIncludeTax": self.template.prices_include_tax,
            "hasTax": self.template.lines.iter().any(|l| l.tax_code_id.is_some()),
            "lineCount": self.template.lines.len(),
            "frequency": self.frequency,
            "every": self.every,
            "startDate": self.start_date,
            "endDate": self.end_date,
            "nextDate": self.next_date,
            "status": self.status,
            "lastError": self.last_error,
            "lastRunAt": self.last_run_at,
            "invoicesMade": self.made,
            "draftsWaiting": self.drafts,
            "createdById": self.created_by_id,
            "createdAt": self.created_at,
            "updatedAt": self.updated_at,
        })
    }
}

const COLUMNS: &str = r#"r."id", r."name", r."sourceInvoiceId", r."template", r."frequency", r."every", r."startDate",
    r."endDate", r."nextDate", r."dateIndex", r."status", r."lastError", r."lastRunAt", r."createdById", r."createdAt",
    r."updatedAt",
    (SELECT COUNT(*) FROM "recurring_invoice_items" x WHERE x."recurringId" = r."id") AS "made",
    (SELECT COUNT(*) FROM "recurring_invoice_items" x JOIN "invoices" i ON i."id" = x."invoiceId"
      WHERE x."recurringId" = r."id" AND i."status" = 'DRAFT') AS "drafts",
    (SELECT c."name" FROM "customers" c WHERE c."id" = json_extract(r."template", '$.customerId')) AS "customerName""#;

fn from_row(r: &SqliteRow) -> AppResult<Recurring> {
    let template: String = r.try_get("template")?;
    Ok(Recurring {
        id: r.try_get("id")?,
        name: r.try_get("name")?,
        source_invoice_id: r.try_get("sourceInvoiceId")?,
        template: serde_json::from_str(&template).map_err(AppError::internal)?,
        frequency: r.try_get("frequency")?,
        every: r.try_get("every")?,
        start_date: r.try_get("startDate")?,
        end_date: r.try_get("endDate")?,
        next_date: r.try_get("nextDate")?,
        date_index: r.try_get("dateIndex")?,
        status: r.try_get("status")?,
        last_error: r.try_get("lastError")?,
        last_run_at: r.try_get("lastRunAt")?,
        created_by_id: r.try_get("createdById")?,
        created_at: r.try_get("createdAt")?,
        updated_at: r.try_get("updatedAt")?,
        made: r.try_get("made")?,
        drafts: r.try_get("drafts")?,
        customer_name: r.try_get("customerName")?,
    })
}

async fn load(conn: &mut SqliteConnection, id: &str) -> AppResult<Option<Recurring>> {
    let row =
        sqlx::query(sqlx::AssertSqlSafe(format!(r#"SELECT {COLUMNS} FROM "recurring_invoices" r WHERE r."id" = ?1"#)))
            .bind(id)
            .fetch_optional(&mut *conn)
            .await?;
    row.as_ref().map(from_row).transpose()
}

fn not_found(id: &str) -> AppError {
    AppError::NotFound(format!("Recurring invoice {id} not found"))
}

pub async fn find_one(conn: &mut SqliteConnection, id: &str, scope: &Scope) -> AppResult<Recurring> {
    let r = load(conn, id).await?.ok_or_else(|| not_found(id))?;
    if !scope.sees_all && r.created_by_id.as_deref() != scope.user_id.as_deref() {
        return Err(not_found(id));
    }
    Ok(r)
}

pub async fn find_all(db: &Db, scope: &Scope) -> AppResult<Vec<Recurring>> {
    let mut conn = db.read().acquire().await?;
    let sql = format!(
        r#"SELECT {COLUMNS} FROM "recurring_invoices" r WHERE (?1 OR r."createdById" = ?2)
           ORDER BY CASE r."status" WHEN 'ACTIVE' THEN 0 WHEN 'PAUSED' THEN 1 ELSE 2 END, r."nextDate", r."name""#
    );
    let rows =
        sqlx::query(sqlx::AssertSqlSafe(sql)).bind(scope.sees_all).bind(&scope.user_id).fetch_all(&mut *conn).await?;
    rows.iter().map(from_row).collect()
}

/// How many invoices made by schedules the person can see are still waiting as drafts.
pub async fn drafts_waiting(db: &Db, scope: &Scope) -> AppResult<i64> {
    let n = sqlx::query_scalar::<_, i64>(
        r#"SELECT COUNT(*) FROM "recurring_invoice_items" x
           JOIN "recurring_invoices" r ON r."id" = x."recurringId"
           JOIN "invoices" i ON i."id" = x."invoiceId"
           WHERE i."status" = 'DRAFT' AND (?1 OR r."createdById" = ?2)"#,
    )
    .bind(scope.sees_all)
    .bind(&scope.user_id)
    .fetch_one(db.read())
    .await?;
    Ok(n)
}

/// The schedule settings a person chooses.
#[derive(Debug, Clone, Default)]
pub struct ScheduleInput {
    pub name: Option<String>,
    pub frequency: String,
    pub every: i64,
    pub start_date: String,
    pub end_date: Option<String>,
}

fn check_schedule(s: &ScheduleInput) -> AppResult<()> {
    if !FREQUENCIES.contains(&s.frequency.as_str()) {
        return Err(AppError::bad_request("frequency must be one of WEEKLY, MONTHLY, QUARTERLY, YEARLY"));
    }
    if !(1..=99).contains(&s.every) {
        return Err(AppError::bad_request("every must be between 1 and 99"));
    }
    if let Some(end) = &s.end_date {
        if end < &s.start_date {
            return Err(AppError::bad_request("The end date cannot be before the start date"));
        }
    }
    Ok(())
}

/// The first date on or after `from` (index and date), or None once past the end date.
fn first_from(s: &ScheduleInput, from: &str) -> AppResult<(i64, Option<String>)> {
    let mut index = 0;
    loop {
        let d = nth_date(&s.start_date, &s.frequency, s.every, index)
            .ok_or_else(|| AppError::bad_request("The schedule runs past the last date the app can count"))?;
        if s.end_date.as_deref().is_some_and(|e| d.as_str() > e) {
            return Ok((index, None));
        }
        if d.as_str() >= from {
            return Ok((index, Some(d)));
        }
        index += 1;
    }
}

/// Makes a schedule from an invoice (any status except void). Its first invoice is on the start
/// date; a start date in the past makes the missed invoices at the next run, as for a closed app.
pub async fn create(db: &Db, invoice_id: &str, s: ScheduleInput, scope: &Scope) -> AppResult<Recurring> {
    check_schedule(&s)?;
    let mut tx = db.begin().await?;
    let inv = invoices::find_one(&mut tx, invoice_id, scope).await?;
    if inv.status == invoices::VOID {
        return Err(AppError::bad_request("A voided invoice cannot be made recurring"));
    }
    let template = Template::from_invoice(&inv);
    let name = s
        .name
        .clone()
        .map(|n| n.trim().to_string())
        .filter(|n| !n.is_empty())
        .or_else(|| template.bill_to_name.clone())
        .unwrap_or_else(|| inv.invoice_number.clone());
    let id = new_id();
    let now = now_iso();
    sqlx::query(
        r#"INSERT INTO "recurring_invoices" ("id","name","sourceInvoiceId","template","frequency","every","startDate","endDate",
             "nextDate","dateIndex","status","createdById","createdAt","updatedAt")
           VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?7,0,'ACTIVE',?9,?10,?10)"#,
    )
    .bind(&id)
    .bind(&name)
    .bind(&inv.id)
    .bind(serde_json::to_string(&template).map_err(AppError::internal)?)
    .bind(&s.frequency)
    .bind(s.every)
    .bind(&s.start_date)
    .bind(&s.end_date)
    .bind(&scope.user_id)
    .bind(&now)
    .execute(&mut *tx)
    .await?;
    let r = load(&mut tx, &id).await?.ok_or_else(|| not_found(&id))?;
    tx.commit().await?;
    Ok(r)
}

/// Changes the schedule (and with `invoice_id`, takes a new copy of that invoice as the
/// template). The next date is worked out again from today: dates already made are not made
/// again, and an earlier start date does not bring back dates before today.
pub async fn update(
    db: &Db,
    id: &str,
    s: ScheduleInput,
    invoice_id: Option<&str>,
    scope: &Scope,
    today: &str,
) -> AppResult<Recurring> {
    check_schedule(&s)?;
    let mut tx = db.begin().await?;
    let r = find_one(&mut tx, id, scope).await?;
    let template = match invoice_id {
        Some(inv_id) => {
            let inv = invoices::find_one(&mut tx, inv_id, scope).await?;
            if inv.status == invoices::VOID {
                return Err(AppError::bad_request("A voided invoice cannot be used"));
            }
            Template::from_invoice(&inv)
        }
        None => r.template.clone(),
    };
    let (index, next) = first_from(&s, today)?;
    let status = if r.status == "PAUSED" {
        "PAUSED"
    } else if next.is_none() {
        "ENDED"
    } else {
        "ACTIVE"
    };
    let name = s.name.clone().map(|n| n.trim().to_string()).filter(|n| !n.is_empty()).unwrap_or(r.name.clone());
    sqlx::query(
        r#"UPDATE "recurring_invoices" SET "name" = ?1, "template" = ?2, "frequency" = ?3, "every" = ?4, "startDate" = ?5,
             "endDate" = ?6, "nextDate" = ?7, "dateIndex" = ?8, "status" = ?9, "lastError" = NULL,
             "sourceInvoiceId" = COALESCE(?10, "sourceInvoiceId"), "updatedAt" = ?11
           WHERE "id" = ?12"#,
    )
    .bind(&name)
    .bind(serde_json::to_string(&template).map_err(AppError::internal)?)
    .bind(&s.frequency)
    .bind(s.every)
    .bind(&s.start_date)
    .bind(&s.end_date)
    .bind(&next)
    .bind(index)
    .bind(status)
    .bind(invoice_id)
    .bind(now_iso())
    .bind(id)
    .execute(&mut *tx)
    .await?;
    let r = load(&mut tx, id).await?.ok_or_else(|| not_found(id))?;
    tx.commit().await?;
    Ok(r)
}

/// Pausing skips every date until it is resumed; resuming continues from today (paused dates
/// are not made afterwards).
pub async fn set_paused(db: &Db, id: &str, paused: bool, scope: &Scope, today: &str) -> AppResult<Recurring> {
    let mut tx = db.begin().await?;
    let r = find_one(&mut tx, id, scope).await?;
    if r.status == "ENDED" {
        return Err(AppError::bad_request("This schedule has ended; change its end date to start it again"));
    }
    let (index, next, status) = if paused {
        (r.date_index, r.next_date.clone(), "PAUSED")
    } else {
        let s = ScheduleInput {
            name: None,
            frequency: r.frequency.clone(),
            every: r.every,
            start_date: r.start_date.clone(),
            end_date: r.end_date.clone(),
        };
        let (index, next) = first_from(&s, today)?;
        // Never step back before a date that was already made.
        let (index, next) = if index < r.date_index { (r.date_index, r.next_date.clone()) } else { (index, next) };
        let status = if next.is_none() { "ENDED" } else { "ACTIVE" };
        (index, next, status)
    };
    sqlx::query(
        r#"UPDATE "recurring_invoices" SET "status" = ?1, "nextDate" = ?2, "dateIndex" = ?3, "updatedAt" = ?4 WHERE "id" = ?5"#,
    )
    .bind(status)
    .bind(&next)
    .bind(index)
    .bind(now_iso())
    .bind(id)
    .execute(&mut *tx)
    .await?;
    let r = load(&mut tx, id).await?.ok_or_else(|| not_found(id))?;
    tx.commit().await?;
    Ok(r)
}

/// Stops and removes a schedule. The invoices it made stay as they are.
pub async fn remove(db: &Db, id: &str, scope: &Scope) -> AppResult<()> {
    let mut tx = db.begin().await?;
    find_one(&mut tx, id, scope).await?;
    sqlx::query(r#"DELETE FROM "recurring_invoices" WHERE "id" = ?1"#).bind(id).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(())
}

/// What one schedule did in a run.
#[derive(Debug, Clone)]
pub struct RunReport {
    pub recurring_id: String,
    pub name: String,
    pub created_by_id: Option<String>,
    pub invoice_numbers: Vec<String>,
    pub error: Option<String>,
}

/// Makes the DRAFT invoices of every active schedule whose next date is on or before `today`,
/// each schedule in its own transaction. A schedule that cannot make its invoice (say its
/// customer was deleted) keeps its date, records the reason in `lastError` and is tried again
/// on the next run; the others go on.
pub async fn run_due(db: &Db, today: &str) -> AppResult<Vec<RunReport>> {
    let ids: Vec<String> = sqlx::query_scalar(
        r#"SELECT "id" FROM "recurring_invoices" WHERE "status" = 'ACTIVE' AND "nextDate" IS NOT NULL AND "nextDate" <= ?1
           ORDER BY "nextDate""#,
    )
    .bind(today)
    .fetch_all(db.read())
    .await?;
    let mut reports = Vec::new();
    for id in ids {
        match run_one(db, &id, today).await {
            Ok(Some(report)) => reports.push(report),
            Ok(None) => {}
            Err(e) => {
                let message = e.user_message();
                sqlx::query(r#"UPDATE "recurring_invoices" SET "lastError" = ?1, "lastRunAt" = ?2 WHERE "id" = ?3"#)
                    .bind(&message)
                    .bind(now_iso())
                    .bind(&id)
                    .execute(db.write())
                    .await?;
                let (name, created_by_id): (String, Option<String>) =
                    sqlx::query_as(r#"SELECT "name", "createdById" FROM "recurring_invoices" WHERE "id" = ?1"#)
                        .bind(&id)
                        .fetch_one(db.read())
                        .await?;
                reports.push(RunReport {
                    recurring_id: id,
                    name,
                    created_by_id,
                    invoice_numbers: Vec::new(),
                    error: Some(message),
                });
            }
        }
    }
    Ok(reports)
}

async fn run_one(db: &Db, id: &str, today: &str) -> AppResult<Option<RunReport>> {
    let mut tx = db.begin().await?;
    let Some(r) = load(&mut tx, id).await? else { return Ok(None) };
    if r.status != "ACTIVE" {
        return Ok(None);
    }
    let mut index = r.date_index;
    let mut next = r.next_date.clone();
    let mut made = Vec::new();
    while let Some(date) = next.clone() {
        if date.as_str() > today || made.len() >= MAX_PER_RUN {
            break;
        }
        if r.end_date.as_deref().is_some_and(|e| date.as_str() > e) {
            next = None;
            break;
        }
        let already: Option<String> = sqlx::query_scalar(
            r#"SELECT "invoiceId" FROM "recurring_invoice_items" WHERE "recurringId" = ?1 AND "occurrenceDate" = ?2"#,
        )
        .bind(id)
        .bind(&date)
        .fetch_optional(&mut *tx)
        .await?;
        if already.is_none() {
            let inv = invoices::create_in(&mut tx, r.template.invoice_for(&date), r.created_by_id.as_deref()).await?;
            sqlx::query(r#"INSERT INTO "recurring_invoice_items" ("recurringId","occurrenceDate","invoiceId") VALUES (?1,?2,?3)"#)
                .bind(id)
                .bind(&date)
                .bind(&inv.id)
                .execute(&mut *tx)
                .await?;
            made.push(inv.invoice_number);
        }
        index += 1;
        next = nth_date(&r.start_date, &r.frequency, r.every, index);
        if next.is_none() {
            break;
        }
    }
    if let (Some(n), Some(e)) = (&next, &r.end_date) {
        if n > e {
            next = None;
        }
    }
    let status = if next.is_none() { "ENDED" } else { "ACTIVE" };
    sqlx::query(
        r#"UPDATE "recurring_invoices" SET "nextDate" = ?1, "dateIndex" = ?2, "status" = ?3, "lastError" = NULL,
             "lastRunAt" = ?4, "updatedAt" = ?4 WHERE "id" = ?5"#,
    )
    .bind(&next)
    .bind(index)
    .bind(status)
    .bind(now_iso())
    .bind(id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(Some(RunReport {
        recurring_id: id.to_string(),
        name: r.name,
        created_by_id: r.created_by_id,
        invoice_numbers: made,
        error: None,
    }))
}

#[cfg(test)]
mod date_tests {
    use super::nth_date;

    #[test]
    fn months_count_from_the_start_date_and_never_drift() {
        let dates: Vec<_> = (0..4).filter_map(|i| nth_date("2026-01-31", "MONTHLY", 1, i)).collect();
        assert_eq!(dates, ["2026-01-31", "2026-02-28", "2026-03-31", "2026-04-30"]);
        assert_eq!(nth_date("2026-01-15", "WEEKLY", 2, 3).as_deref(), Some("2026-02-26"));
        assert_eq!(nth_date("2026-11-30", "QUARTERLY", 1, 1).as_deref(), Some("2027-02-28"));
        assert_eq!(nth_date("2028-02-29", "YEARLY", 1, 1).as_deref(), Some("2029-02-28"));
        assert_eq!(nth_date("2026-01-01", "DAILY", 1, 1), None);
    }
}
