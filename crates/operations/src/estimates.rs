//! D60: estimates / quotes. Priced exactly like an invoice (lines, discounts, tax codes,
//! currency) but nothing reaches the ledger. Statuses: DRAFT → SENT → ACCEPTED or DECLINED, set
//! by hand; "convert" turns an estimate into a DRAFT invoice once (status CONVERTED, linked by
//! `invoiceId`). Visibility follows the invoice rules: Admin / `invoices:all` see every estimate,
//! others the ones they made.

use crate::invoices::{self, build_lines, join_options, or_null, CreateInvoice, LineInput, PricedLine, Scope};
use crate::parties::{self, Party, PartyKind};
use crate::{money_str, opt, require_account};
use app_core::ids::new_id;
use app_core::money::Money;
use app_core::time::now_iso;
use app_core::{AppError, AppResult};
use database::{counters, Db, Tx};
use serde_json::{json, Map, Value};
use sqlx::sqlite::SqliteRow;
use sqlx::{Row, SqliteConnection};

pub const DRAFT: &str = "DRAFT";
pub const SENT: &str = "SENT";
pub const ACCEPTED: &str = "ACCEPTED";
pub const DECLINED: &str = "DECLINED";
pub const CONVERTED: &str = "CONVERTED";
pub const STATUSES: &[&str] = &[DRAFT, SENT, ACCEPTED, DECLINED, CONVERTED];
/// The statuses a person may set by hand (CONVERTED only comes from turning it into an invoice).
pub const SETTABLE: &[&str] = &[DRAFT, SENT, ACCEPTED, DECLINED];

#[derive(Debug, Clone)]
pub struct EstimateLine {
    pub id: String,
    pub description: String,
    pub quantity: Money,
    pub unit_price: Money,
    pub amount: Money,
    pub account_id: Option<String>,
    pub tax_code_id: Option<String>,
    pub tax_amount: Money,
    pub item_id: Option<String>,
    pub discount_amount: Money,
}

#[derive(Debug, Clone)]
pub struct Estimate {
    pub id: String,
    pub estimate_number: String,
    pub customer_id: Option<String>,
    pub date: String,
    pub expiry_date: Option<String>,
    pub status: String,
    pub notes: Option<String>,
    pub bill_to_name: Option<String>,
    pub bill_to_address: Option<String>,
    pub payment_options: Option<String>,
    pub signatory_name: Option<String>,
    pub signatory_title: Option<String>,
    pub payment_terms: Option<String>,
    pub total_amount: Money,
    pub tax_amount: Money,
    pub prices_include_tax: bool,
    pub currency: Option<String>,
    pub exchange_rate: i64,
    pub invoice_id: Option<String>,
    pub invoice_number: Option<String>,
    pub created_by_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub lines: Vec<EstimateLine>,
    pub customer: Option<Party>,
}

impl Estimate {
    pub fn to_json(&self) -> Value {
        let lines: Vec<Value> = self
            .lines
            .iter()
            .map(|l| {
                json!({
                    "id": l.id,
                    "estimateId": self.id,
                    "description": l.description,
                    "quantity": money_str(l.quantity),
                    "unitPrice": money_str(l.unit_price),
                    "amount": money_str(l.amount),
                    "accountId": opt(&l.account_id),
                    "taxCodeId": opt(&l.tax_code_id),
                    "taxAmount": money_str(l.tax_amount),
                    "itemId": opt(&l.item_id),
                    "discountAmount": money_str(l.discount_amount),
                })
            })
            .collect();
        let mut m = Map::new();
        m.insert("id".into(), json!(self.id));
        m.insert("estimateNumber".into(), json!(self.estimate_number));
        m.insert("customerId".into(), opt(&self.customer_id));
        m.insert("customer".into(), serde_json::to_value(&self.customer).unwrap_or(Value::Null));
        m.insert("date".into(), json!(self.date));
        m.insert("expiryDate".into(), opt(&self.expiry_date));
        m.insert("status".into(), json!(self.status));
        m.insert("notes".into(), opt(&self.notes));
        m.insert("billToName".into(), opt(&self.bill_to_name));
        m.insert("billToAddress".into(), opt(&self.bill_to_address));
        m.insert("paymentOptions".into(), opt(&self.payment_options));
        m.insert("signatoryName".into(), opt(&self.signatory_name));
        m.insert("signatoryTitle".into(), opt(&self.signatory_title));
        m.insert("paymentTerms".into(), opt(&self.payment_terms));
        m.insert("totalAmount".into(), money_str(self.total_amount));
        m.insert("taxAmount".into(), money_str(self.tax_amount));
        m.insert(
            "subtotal".into(),
            money_str(self.total_amount.checked_sub(self.tax_amount).unwrap_or(self.total_amount)),
        );
        m.insert("pricesIncludeTax".into(), json!(self.prices_include_tax));
        m.insert("currency".into(), opt(&self.currency));
        m.insert("exchangeRate".into(), crate::fx::rate_json(self.exchange_rate));
        m.insert("invoiceId".into(), opt(&self.invoice_id));
        m.insert("invoiceNumber".into(), opt(&self.invoice_number));
        m.insert("createdById".into(), opt(&self.created_by_id));
        m.insert("lines".into(), Value::Array(lines));
        m.insert("createdAt".into(), json!(self.created_at));
        m.insert("updatedAt".into(), json!(self.updated_at));
        Value::Object(m)
    }
}

const COLUMNS: &str = r#"e."id", e."estimateNumber", e."customerId", e."date", e."expiryDate", e."status", e."notes",
    e."billToName", e."billToAddress", e."paymentOptions", e."signatoryName", e."signatoryTitle", e."paymentTerms",
    e."totalAmount", e."taxAmount", e."pricesIncludeTax", e."currency", e."exchangeRate", e."invoiceId",
    (SELECT i."invoiceNumber" FROM "invoices" i WHERE i."id" = e."invoiceId") AS "invoiceNumber",
    e."createdById", e."createdAt", e."updatedAt""#;

fn from_row(r: &SqliteRow) -> AppResult<Estimate> {
    Ok(Estimate {
        id: r.try_get("id")?,
        estimate_number: r.try_get("estimateNumber")?,
        customer_id: r.try_get("customerId")?,
        date: r.try_get("date")?,
        expiry_date: r.try_get("expiryDate")?,
        status: r.try_get("status")?,
        notes: r.try_get("notes")?,
        bill_to_name: r.try_get("billToName")?,
        bill_to_address: r.try_get("billToAddress")?,
        payment_options: r.try_get("paymentOptions")?,
        signatory_name: r.try_get("signatoryName")?,
        signatory_title: r.try_get("signatoryTitle")?,
        payment_terms: r.try_get("paymentTerms")?,
        total_amount: Money::from_cents(r.try_get("totalAmount")?),
        tax_amount: Money::from_cents(r.try_get("taxAmount")?),
        prices_include_tax: r.try_get::<i64, _>("pricesIncludeTax")? == 1,
        currency: r.try_get("currency")?,
        exchange_rate: r.try_get("exchangeRate")?,
        invoice_id: r.try_get("invoiceId")?,
        invoice_number: r.try_get("invoiceNumber")?,
        created_by_id: r.try_get("createdById")?,
        created_at: r.try_get("createdAt")?,
        updated_at: r.try_get("updatedAt")?,
        lines: Vec::new(),
        customer: None,
    })
}

async fn attach(conn: &mut SqliteConnection, estimates: &mut [Estimate]) -> AppResult<()> {
    for est in estimates.iter_mut() {
        let rows = sqlx::query(
            r#"SELECT "id","description","quantity","unitPrice","amount","accountId","taxCodeId","taxAmount","itemId","discountAmount"
               FROM "estimate_lines" WHERE "estimateId" = ?1 ORDER BY rowid"#,
        )
        .bind(&est.id)
        .fetch_all(&mut *conn)
        .await?;
        est.lines = rows
            .iter()
            .map(|r| {
                Ok(EstimateLine {
                    id: r.try_get("id")?,
                    description: r.try_get("description")?,
                    quantity: Money::from_cents(r.try_get("quantity")?),
                    unit_price: Money::from_cents(r.try_get("unitPrice")?),
                    amount: Money::from_cents(r.try_get("amount")?),
                    account_id: r.try_get("accountId")?,
                    tax_code_id: r.try_get("taxCodeId")?,
                    tax_amount: Money::from_cents(r.try_get("taxAmount")?),
                    item_id: r.try_get("itemId")?,
                    discount_amount: Money::from_cents(r.try_get("discountAmount")?),
                })
            })
            .collect::<AppResult<Vec<_>>>()?;
        if let Some(cid) = est.customer_id.clone() {
            est.customer = parties::get(&mut *conn, PartyKind::Customer, &cid).await?;
        }
    }
    Ok(())
}

async fn load(conn: &mut SqliteConnection, id: &str) -> AppResult<Option<Estimate>> {
    let row = sqlx::query(sqlx::AssertSqlSafe(format!(r#"SELECT {COLUMNS} FROM "estimates" e WHERE e."id" = ?1"#)))
        .bind(id)
        .fetch_optional(&mut *conn)
        .await?;
    let Some(row) = row else { return Ok(None) };
    let mut v = vec![from_row(&row)?];
    attach(conn, &mut v).await?;
    Ok(v.pop())
}

fn not_found(id: &str) -> AppError {
    AppError::NotFound(format!("Estimate {id} not found"))
}

/// Another user's estimate is hidden behind a 404, as for invoices.
pub async fn find_one(conn: &mut SqliteConnection, id: &str, scope: &Scope) -> AppResult<Estimate> {
    let est = load(conn, id).await?.ok_or_else(|| not_found(id))?;
    if !scope.sees_all && est.created_by_id.as_deref() != scope.user_id.as_deref() {
        return Err(not_found(id));
    }
    Ok(est)
}

/// What an estimate is made from: the invoice fields, with a "valid until" date instead of a
/// due date (payment terms are kept for the invoice it may become).
#[derive(Debug, Clone, Default)]
pub struct EstimateInput {
    pub customer_id: Option<String>,
    pub date: String,
    pub expiry_date: Option<String>,
    pub payment_terms: Option<String>,
    pub notes: Option<String>,
    pub bill_to_name: Option<String>,
    pub bill_to_address: Option<String>,
    pub payment_options: Option<Vec<String>>,
    pub signatory_name: Option<String>,
    pub signatory_title: Option<String>,
    pub lines: Vec<LineInput>,
    pub prices_include_tax: bool,
    pub currency: Option<String>,
    pub exchange_rate: Option<i64>,
}

async fn check(tx: &mut Tx, dto: &EstimateInput) -> AppResult<()> {
    if dto.customer_id.is_none() && or_null(dto.bill_to_name.as_deref()).is_none() {
        return Err(AppError::bad_request("Select a customer or enter a name to quote"));
    }
    if let Some(c) = &dto.customer_id {
        parties::find_one(tx, PartyKind::Customer, c).await?;
    }
    if dto.lines.is_empty() {
        return Err(AppError::bad_request("An estimate needs at least one line"));
    }
    if let Some(exp) = dto.expiry_date.as_deref().filter(|e| !e.is_empty()) {
        if exp < dto.date.as_str() {
            return Err(AppError::bad_request("The estimate cannot expire before its date"));
        }
    }
    Ok(())
}

/// Stores the lines, priced as an invoice would price them; returns the total and its tax.
async fn insert_lines(
    tx: &mut Tx,
    estimate_id: &str,
    inputs: &[LineInput],
    prices_include_tax: bool,
) -> AppResult<(Money, Money)> {
    let mut inputs = inputs.to_vec();
    for l in &mut inputs {
        // A product's income account is used unless one was chosen, as on invoices.
        if let Some(item_id) = &l.item_id {
            let item = crate::stock::find_one(tx, item_id).await?;
            if l.account_id.is_none() {
                l.account_id = item.income_account_id.clone();
            }
        }
        if let Some(a) = &l.account_id {
            require_account(tx, a).await?;
        }
    }
    let (lines, total, tax_total) = build_lines(tx, &inputs, prices_include_tax).await?;
    for PricedLine { input: l, quantity, unit_price, amount, tax, .. } in lines {
        sqlx::query(
            r#"INSERT INTO "estimate_lines" ("id","estimateId","description","quantity","unitPrice","amount","accountId","taxCodeId",
                 "taxAmount","itemId","discountAmount")
               VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)"#,
        )
        .bind(new_id())
        .bind(estimate_id)
        .bind(&l.description)
        .bind(quantity.cents())
        .bind(unit_price.cents())
        .bind(amount.cents())
        .bind(&l.account_id)
        .bind(&l.tax_code_id)
        .bind(tax.cents())
        .bind(&l.item_id)
        .bind(l.discount.cents())
        .execute(&mut **tx)
        .await?;
    }
    Ok((total, tax_total))
}

/// Writes every field of the estimate from `dto` and replaces its lines.
async fn write(tx: &mut Tx, id: &str, dto: &EstimateInput) -> AppResult<()> {
    let (currency, rate) = crate::fx::resolve(tx, dto.currency.as_deref(), dto.exchange_rate).await?;
    sqlx::query(r#"DELETE FROM "estimate_lines" WHERE "estimateId" = ?1"#).bind(id).execute(&mut **tx).await?;
    let (total, tax) = insert_lines(tx, id, &dto.lines, dto.prices_include_tax).await?;
    sqlx::query(
        r#"UPDATE "estimates" SET "customerId" = ?1, "date" = ?2, "expiryDate" = ?3, "notes" = ?4, "billToName" = ?5,
             "billToAddress" = ?6, "paymentOptions" = ?7, "signatoryName" = ?8, "signatoryTitle" = ?9, "paymentTerms" = ?10,
             "totalAmount" = ?11, "taxAmount" = ?12, "pricesIncludeTax" = ?13, "currency" = ?14, "exchangeRate" = ?15,
             "updatedAt" = ?16
           WHERE "id" = ?17"#,
    )
    .bind(&dto.customer_id)
    .bind(&dto.date)
    .bind(dto.expiry_date.as_deref().filter(|e| !e.is_empty()))
    .bind(or_null(dto.notes.as_deref()))
    .bind(or_null(dto.bill_to_name.as_deref()))
    .bind(or_null(dto.bill_to_address.as_deref()))
    .bind(dto.payment_options.as_deref().and_then(join_options))
    .bind(or_null(dto.signatory_name.as_deref()))
    .bind(or_null(dto.signatory_title.as_deref()))
    .bind(or_null(dto.payment_terms.as_deref()))
    .bind(total.cents())
    .bind(tax.cents())
    .bind(dto.prices_include_tax)
    .bind(&currency)
    .bind(rate)
    .bind(now_iso())
    .bind(id)
    .execute(&mut **tx)
    .await?;
    crate::fx::remember_rate(tx, &currency, rate).await?;
    Ok(())
}

pub async fn create(db: &Db, dto: EstimateInput, created_by: Option<&str>) -> AppResult<Estimate> {
    let mut tx = db.begin().await?;
    check(&mut tx, &dto).await?;
    let id = new_id();
    let number = counters::next_formatted(&mut tx, "EST", 4).await?;
    let now = now_iso();
    sqlx::query(
        r#"INSERT INTO "estimates" ("id","estimateNumber","date","status","createdById","createdAt","updatedAt")
           VALUES (?1,?2,?3,'DRAFT',?4,?5,?5)"#,
    )
    .bind(&id)
    .bind(&number)
    .bind(&dto.date)
    .bind(created_by)
    .bind(&now)
    .execute(&mut *tx)
    .await?;
    write(&mut tx, &id, &dto).await?;
    let est = load(&mut tx, &id).await?.ok_or_else(|| AppError::internal("created estimate vanished"))?;
    tx.commit().await?;
    Ok(est)
}

fn editable(est: &Estimate) -> AppResult<()> {
    match est.status.as_str() {
        DRAFT | SENT => Ok(()),
        CONVERTED => Err(AppError::bad_request(format!(
            "{} was turned into invoice {}; change the invoice instead",
            est.estimate_number,
            est.invoice_number.as_deref().unwrap_or("")
        ))),
        _ => Err(AppError::bad_request(format!(
            "{} is {}; set it back to Draft or Sent to change it",
            est.estimate_number,
            est.status.to_lowercase()
        ))),
    }
}

/// Replaces the whole estimate (the screen always sends every field). Only while DRAFT or SENT.
pub async fn update(db: &Db, id: &str, dto: EstimateInput, scope: &Scope) -> AppResult<Estimate> {
    let mut tx = db.begin().await?;
    let est = find_one(&mut tx, id, scope).await?;
    editable(&est)?;
    check(&mut tx, &dto).await?;
    write(&mut tx, id, &dto).await?;
    let est = load(&mut tx, id).await?.ok_or_else(|| not_found(id))?;
    tx.commit().await?;
    Ok(est)
}

/// Sets DRAFT, SENT, ACCEPTED or DECLINED. A converted estimate keeps its status.
pub async fn set_status(db: &Db, id: &str, status: &str, scope: &Scope) -> AppResult<Estimate> {
    if !SETTABLE.contains(&status) {
        return Err(AppError::bad_request("status must be one of DRAFT, SENT, ACCEPTED, DECLINED"));
    }
    let mut tx = db.begin().await?;
    let est = find_one(&mut tx, id, scope).await?;
    if est.status == CONVERTED {
        return Err(AppError::bad_request(format!(
            "{} was already turned into invoice {}",
            est.estimate_number,
            est.invoice_number.as_deref().unwrap_or("")
        )));
    }
    sqlx::query(r#"UPDATE "estimates" SET "status" = ?1, "updatedAt" = ?2 WHERE "id" = ?3"#)
        .bind(status)
        .bind(now_iso())
        .bind(id)
        .execute(&mut *tx)
        .await?;
    let est = load(&mut tx, id).await?.ok_or_else(|| not_found(id))?;
    tx.commit().await?;
    Ok(est)
}

/// Turns the estimate into a DRAFT invoice dated `today` with the same customer, lines, tax,
/// discounts, currency and print details, and marks the estimate CONVERTED. Once only; a
/// declined estimate must be set back first. Returns the estimate and the new invoice.
pub async fn convert(db: &Db, id: &str, scope: &Scope, today: &str) -> AppResult<(Estimate, invoices::Invoice)> {
    let mut tx = db.begin().await?;
    let est = find_one(&mut tx, id, scope).await?;
    match est.status.as_str() {
        CONVERTED => {
            return Err(AppError::bad_request(format!(
                "{} was already turned into invoice {}",
                est.estimate_number,
                est.invoice_number.as_deref().unwrap_or("")
            )))
        }
        DECLINED => {
            return Err(AppError::bad_request(format!(
                "{} was declined; set it back to Sent or Accepted first",
                est.estimate_number
            )))
        }
        _ => {}
    }
    let dto = CreateInvoice {
        customer_id: est.customer_id.clone(),
        date: today.to_string(),
        due_date: None,
        payment_terms: est.payment_terms.clone(),
        po_number: None,
        notes: est.notes.clone(),
        bill_to_name: est.bill_to_name.clone().or_else(|| est.customer.as_ref().map(|c| c.name.clone())),
        bill_to_address: est.bill_to_address.clone().or_else(|| est.customer.as_ref().and_then(|c| c.address.clone())),
        payment_options: est.payment_options.as_ref().map(|p| p.split('\n').map(str::to_string).collect()),
        signatory_name: est.signatory_name.clone(),
        signatory_title: est.signatory_title.clone(),
        prices_include_tax: est.prices_include_tax,
        currency: est.currency.clone(),
        exchange_rate: Some(est.exchange_rate),
        lines: est
            .lines
            .iter()
            .map(|l| LineInput {
                description: l.description.clone(),
                quantity: Some(l.quantity),
                unit_price: Some(l.unit_price),
                account_id: l.account_id.clone(),
                tax_code_id: l.tax_code_id.clone(),
                item_id: l.item_id.clone(),
                discount: l.discount_amount,
            })
            .collect(),
    };
    let inv = invoices::create_in(&mut tx, dto, scope.user_id.as_deref()).await?;
    sqlx::query(r#"UPDATE "estimates" SET "status" = 'CONVERTED', "invoiceId" = ?1, "updatedAt" = ?2 WHERE "id" = ?3"#)
        .bind(&inv.id)
        .bind(now_iso())
        .bind(id)
        .execute(&mut *tx)
        .await?;
    let est = load(&mut tx, id).await?.ok_or_else(|| not_found(id))?;
    tx.commit().await?;
    Ok((est, inv))
}

/// Deletes an estimate that has not become an invoice (estimates never reach the ledger).
pub async fn remove(db: &Db, id: &str, scope: &Scope) -> AppResult<()> {
    let mut tx = db.begin().await?;
    let est = find_one(&mut tx, id, scope).await?;
    if est.status == CONVERTED {
        return Err(AppError::bad_request(format!(
            "{} was turned into invoice {} and is kept with it",
            est.estimate_number,
            est.invoice_number.as_deref().unwrap_or("")
        )));
    }
    sqlx::query(r#"DELETE FROM "estimates" WHERE "id" = ?1"#).bind(id).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(())
}

#[derive(Debug, Clone, Default)]
pub struct EstimateQuery {
    pub page: i64,
    pub limit: i64,
    pub q: Option<String>,
    pub status: Option<String>,
}

/// Newest first; free text over the estimate number, the name quoted and the customer's name.
pub async fn find_all(db: &Db, query: &EstimateQuery, scope: &Scope) -> AppResult<(Vec<Estimate>, i64)> {
    let mut wh = String::from(" WHERE 1 = 1");
    let mut binds: Vec<Option<String>> = Vec::new();
    if !scope.sees_all {
        wh.push_str(r#" AND e."createdById" = ?"#);
        binds.push(scope.user_id.clone());
    }
    if let Some(s) = &query.status {
        wh.push_str(r#" AND e."status" = ?"#);
        binds.push(Some(s.clone()));
    }
    if let Some(q) = query.q.as_deref().map(str::trim).filter(|q| !q.is_empty()) {
        wh.push_str(r#" AND (e."estimateNumber" LIKE ? OR e."billToName" LIKE ? OR c."name" LIKE ?)"#);
        let pattern = format!("%{q}%");
        for _ in 0..3 {
            binds.push(Some(pattern.clone()));
        }
    }
    let from = r#" FROM "estimates" e LEFT JOIN "customers" c ON c."id" = e."customerId""#;
    let mut conn = db.read().acquire().await?;
    let count_sql = format!("SELECT COUNT(*){from}{wh}");
    let mut cq = sqlx::query_scalar::<_, i64>(sqlx::AssertSqlSafe(&*count_sql));
    for b in &binds {
        cq = cq.bind(b);
    }
    let total = cq.fetch_one(&mut *conn).await?;
    let sql = format!(r#"SELECT {COLUMNS}{from}{wh} ORDER BY e."date" DESC, e."estimateNumber" DESC LIMIT ? OFFSET ?"#);
    let mut q = sqlx::query(sqlx::AssertSqlSafe(&*sql));
    for b in &binds {
        q = q.bind(b);
    }
    let rows = q.bind(query.limit).bind((query.page - 1).saturating_mul(query.limit)).fetch_all(&mut *conn).await?;
    let mut list = rows.iter().map(from_row).collect::<AppResult<Vec<_>>>()?;
    attach(&mut conn, &mut list).await?;
    Ok((list, total))
}
