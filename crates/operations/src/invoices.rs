//! Invoices (reference `invoices.service.ts`). Posting debits 1100 Accounts Receivable for the
//! total and credits each line's revenue account; unposting and voiding reverse that entry.

use crate::parties::{self, Party, PartyKind};
use crate::{money_num, money_str, opt, overflow, require_account};
use app_core::ids::new_id;
use app_core::money::Money;
use app_core::time::now_iso;
use app_core::{AppError, AppResult};
use chrono::{Days, NaiveDate};
use database::{counters, Db, Tx};
use ledger::{entries, NewEntry, NewLine};
use serde_json::{json, Map, Value};
use sqlx::sqlite::SqliteRow;
use sqlx::{Row, SqliteConnection};
use std::collections::HashMap;

pub const DRAFT: &str = "DRAFT";
pub const POSTED: &str = "POSTED";
pub const PAID: &str = "PAID";
pub const VOID: &str = "VOID";

pub const PAYMENT_TERMS: &[&str] = &["DUE_ON_RECEIPT", "NET_15", "NET_30", "NET_60"];

fn term_days(terms: &str) -> Option<u64> {
    match terms {
        "DUE_ON_RECEIPT" => Some(0),
        "NET_15" => Some(15),
        "NET_30" => Some(30),
        "NET_60" => Some(60),
        _ => None,
    }
}

/// Who is asking: Admin or `invoices:all` sees everything; others only what they raised.
#[derive(Debug, Clone)]
pub struct Scope {
    pub user_id: Option<String>,
    pub sees_all: bool,
}

#[derive(Debug, Clone)]
pub struct InvoiceLine {
    pub id: String,
    pub invoice_id: String,
    pub description: String,
    /// Hundredths, like money: 250 = 2.50.
    pub quantity: Money,
    pub unit_price: Money,
    pub amount: Money,
    pub account_id: Option<String>,
    /// D26: the line's tax code and its tax, rounded half-up.
    pub tax_code_id: Option<String>,
    pub tax_amount: Money,
    /// D27: the tax split by standard code (several for a tax group).
    pub taxes: Vec<crate::tax::TaxPart>,
}

impl InvoiceLine {
    /// The line without tax: what revenue is credited with.
    pub fn net(&self, prices_include_tax: bool) -> AppResult<Money> {
        if prices_include_tax {
            self.amount.checked_sub(self.tax_amount).ok_or_else(overflow)
        } else {
            Ok(self.amount)
        }
    }
}

#[derive(Debug, Clone)]
pub struct Invoice {
    pub id: String,
    pub invoice_number: String,
    pub po_number: Option<String>,
    pub customer_id: Option<String>,
    pub date: String,
    pub due_date: Option<String>,
    pub status: String,
    pub notes: Option<String>,
    pub bill_to_name: Option<String>,
    pub bill_to_address: Option<String>,
    pub payment_options: Option<String>,
    pub signatory_name: Option<String>,
    pub signatory_title: Option<String>,
    pub payment_terms: Option<String>,
    pub total_amount: Money,
    pub amount_paid: Money,
    /// Taken off by posted credit notes (D21).
    pub amount_credited: Money,
    /// D26: total tax on the lines, and whether the line amounts already include it.
    pub tax_amount: Money,
    pub prices_include_tax: bool,
    /// D30: the invoice's currency (None = the home currency), its exchange rate (home per one
    /// unit, in hundred-millionths) and its figures in the home currency as booked.
    pub currency: Option<String>,
    pub exchange_rate: i64,
    pub base_total: Money,
    pub base_amount_paid: Money,
    pub base_amount_credited: Money,
    pub journal_entry_id: Option<String>,
    pub created_by_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub lines: Vec<InvoiceLine>,
    pub customer: Option<Party>,
}

/// Which TypeORM response shape to reproduce.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    /// Just inserted: no `customer`; lines echo the sent price and amount as numbers.
    Created,
    /// A draft PATCH that replaced the lines: fresh lines, and `totalAmount` as a number.
    UpdatedLines,
    /// Read back from the database: every numeric column a 2-dp string.
    Read,
}

impl Invoice {
    pub fn to_json(&self, view: View) -> Value {
        let fresh = view != View::Read;
        let lines: Vec<Value> = self
            .lines
            .iter()
            .map(|l| {
                json!({
                    "id": l.id,
                    "invoiceId": l.invoice_id,
                    "description": l.description,
                    "quantity": money_str(l.quantity),
                    "unitPrice": if fresh { money_num(l.unit_price) } else { money_str(l.unit_price) },
                    "amount": if fresh { money_num(l.amount) } else { money_str(l.amount) },
                    "accountId": opt(&l.account_id),
                    "taxCodeId": opt(&l.tax_code_id),
                    "taxAmount": if fresh { money_num(l.tax_amount) } else { money_str(l.tax_amount) },
                    "taxes": l.taxes.iter().map(|t| json!({
                        "taxCodeId": t.tax_code_id,
                        "taxAmount": money_str(t.amount),
                    })).collect::<Vec<_>>(),
                })
            })
            .collect();
        let mut m = Map::new();
        m.insert("id".into(), json!(self.id));
        m.insert("invoiceNumber".into(), json!(self.invoice_number));
        m.insert("poNumber".into(), opt(&self.po_number));
        m.insert("customerId".into(), opt(&self.customer_id));
        if view != View::Created {
            m.insert("customer".into(), serde_json::to_value(&self.customer).unwrap_or(Value::Null));
        }
        m.insert("date".into(), json!(self.date));
        m.insert("dueDate".into(), opt(&self.due_date));
        m.insert("status".into(), json!(self.status));
        m.insert("notes".into(), opt(&self.notes));
        m.insert("billToName".into(), opt(&self.bill_to_name));
        m.insert("billToAddress".into(), opt(&self.bill_to_address));
        m.insert("paymentOptions".into(), opt(&self.payment_options));
        m.insert("signatoryName".into(), opt(&self.signatory_name));
        m.insert("signatoryTitle".into(), opt(&self.signatory_title));
        m.insert("paymentTerms".into(), opt(&self.payment_terms));
        m.insert(
            "totalAmount".into(),
            if view == View::UpdatedLines { money_num(self.total_amount) } else { money_str(self.total_amount) },
        );
        m.insert("amountPaid".into(), money_str(self.amount_paid));
        m.insert("amountCredited".into(), money_str(self.amount_credited));
        m.insert("taxAmount".into(), money_str(self.tax_amount));
        m.insert(
            "subtotal".into(),
            money_str(self.total_amount.checked_sub(self.tax_amount).unwrap_or(self.total_amount)),
        );
        m.insert("pricesIncludeTax".into(), json!(self.prices_include_tax));
        m.insert("currency".into(), opt(&self.currency));
        m.insert("exchangeRate".into(), crate::fx::rate_json(self.exchange_rate));
        m.insert("baseTotal".into(), money_str(self.base_total));
        m.insert("baseAmountPaid".into(), money_str(self.base_amount_paid));
        m.insert("baseAmountCredited".into(), money_str(self.base_amount_credited));
        m.insert("journalEntryId".into(), opt(&self.journal_entry_id));
        m.insert("createdById".into(), opt(&self.created_by_id));
        m.insert("lines".into(), Value::Array(lines));
        m.insert("createdAt".into(), json!(self.created_at));
        m.insert("updatedAt".into(), json!(self.updated_at));
        Value::Object(m)
    }
}

const COLUMNS: &str = r#"i."id", i."invoiceNumber", i."poNumber", i."customerId", i."date", i."dueDate", i."status", i."notes",
    i."billToName", i."billToAddress", i."paymentOptions", i."signatoryName", i."signatoryTitle", i."paymentTerms",
    i."totalAmount", i."amountPaid", i."amountCredited", i."taxAmount", i."pricesIncludeTax", i."currency", i."exchangeRate", i."baseTotal", i."baseAmountPaid",
    i."baseAmountCredited", i."journalEntryId", i."createdById", i."createdAt", i."updatedAt""#;

fn from_row(r: &SqliteRow) -> AppResult<Invoice> {
    Ok(Invoice {
        id: r.try_get("id")?,
        invoice_number: r.try_get("invoiceNumber")?,
        po_number: r.try_get("poNumber")?,
        customer_id: r.try_get("customerId")?,
        date: r.try_get("date")?,
        due_date: r.try_get("dueDate")?,
        status: r.try_get("status")?,
        notes: r.try_get("notes")?,
        bill_to_name: r.try_get("billToName")?,
        bill_to_address: r.try_get("billToAddress")?,
        payment_options: r.try_get("paymentOptions")?,
        signatory_name: r.try_get("signatoryName")?,
        signatory_title: r.try_get("signatoryTitle")?,
        payment_terms: r.try_get("paymentTerms")?,
        total_amount: Money::from_cents(r.try_get("totalAmount")?),
        amount_paid: Money::from_cents(r.try_get("amountPaid")?),
        amount_credited: Money::from_cents(r.try_get("amountCredited")?),
        tax_amount: Money::from_cents(r.try_get("taxAmount")?),
        prices_include_tax: r.try_get::<i64, _>("pricesIncludeTax")? == 1,
        currency: r.try_get("currency")?,
        exchange_rate: r.try_get("exchangeRate")?,
        base_total: Money::from_cents(r.try_get("baseTotal")?),
        base_amount_paid: Money::from_cents(r.try_get("baseAmountPaid")?),
        base_amount_credited: Money::from_cents(r.try_get("baseAmountCredited")?),
        journal_entry_id: r.try_get("journalEntryId")?,
        created_by_id: r.try_get("createdById")?,
        created_at: r.try_get("createdAt")?,
        updated_at: r.try_get("updatedAt")?,
        lines: Vec::new(),
        customer: None,
    })
}

async fn attach(conn: &mut SqliteConnection, invoices: &mut [Invoice]) -> AppResult<()> {
    let mut customers: HashMap<String, Option<Party>> = HashMap::new();
    for inv in invoices.iter_mut() {
        let rows = sqlx::query(
            r#"SELECT "id","invoiceId","description","quantity","unitPrice","amount","accountId","taxCodeId","taxAmount"
               FROM "invoice_lines" WHERE "invoiceId" = ?1 ORDER BY rowid"#,
        )
        .bind(&inv.id)
        .fetch_all(&mut *conn)
        .await?;
        inv.lines = rows
            .iter()
            .map(|r| {
                Ok(InvoiceLine {
                    id: r.try_get("id")?,
                    invoice_id: r.try_get("invoiceId")?,
                    description: r.try_get("description")?,
                    quantity: Money::from_cents(r.try_get("quantity")?),
                    unit_price: Money::from_cents(r.try_get("unitPrice")?),
                    amount: Money::from_cents(r.try_get("amount")?),
                    account_id: r.try_get("accountId")?,
                    tax_code_id: r.try_get("taxCodeId")?,
                    tax_amount: Money::from_cents(r.try_get("taxAmount")?),
                    taxes: Vec::new(),
                })
            })
            .collect::<AppResult<Vec<_>>>()?;
        let ids: Vec<String> = inv.lines.iter().map(|l| l.id.clone()).collect();
        let mut parts = crate::tax::load_parts(&mut *conn, "INVOICE_LINE", &ids).await?;
        for l in &mut inv.lines {
            l.taxes = parts.remove(&l.id).unwrap_or_default();
        }
        if let Some(cid) = inv.customer_id.clone() {
            if !customers.contains_key(&cid) {
                let c = parties::get(&mut *conn, PartyKind::Customer, &cid).await?;
                customers.insert(cid.clone(), c);
            }
            inv.customer = customers.get(&cid).cloned().flatten();
        }
    }
    Ok(())
}

pub(crate) async fn load(conn: &mut SqliteConnection, id: &str) -> AppResult<Option<Invoice>> {
    let row = sqlx::query(&format!(r#"SELECT {COLUMNS} FROM "invoices" i WHERE i."id" = ?1"#))
        .bind(id)
        .fetch_optional(&mut *conn)
        .await?;
    let Some(row) = row else { return Ok(None) };
    let mut v = vec![from_row(&row)?];
    attach(conn, &mut v).await?;
    Ok(v.pop())
}

fn not_found(id: &str) -> AppError {
    AppError::NotFound(format!("Invoice {id} not found"))
}

/// Reference `findOne` + `assertVisible`: another user's invoice is hidden behind a 404.
pub async fn find_one(conn: &mut SqliteConnection, id: &str, scope: &Scope) -> AppResult<Invoice> {
    let inv = load(conn, id).await?.ok_or_else(|| not_found(id))?;
    if !scope.sees_all && inv.created_by_id.as_deref() != scope.user_id.as_deref() {
        return Err(not_found(id));
    }
    Ok(inv)
}

/// Trims and collapses blanks to null, so the printer falls back to defaults.
fn or_null(v: Option<&str>) -> Option<String> {
    v.map(str::trim).filter(|t| !t.is_empty()).map(str::to_string)
}

/// Terms win over an explicit dueDate only when no dueDate was supplied.
pub fn resolve_due_date(date: &str, due_date: Option<&str>, terms: Option<&str>) -> Option<String> {
    if let Some(d) = due_date.filter(|d| !d.is_empty()) {
        return Some(d.to_string());
    }
    let days = term_days(terms?)?;
    let start = NaiveDate::parse_from_str(date, "%Y-%m-%d").ok()?;
    start.checked_add_days(Days::new(days)).map(|d| d.format("%Y-%m-%d").to_string())
}

fn join_options(options: &[String]) -> Option<String> {
    let joined = options.iter().map(|o| o.trim()).filter(|o| !o.is_empty()).collect::<Vec<_>>().join("\n");
    or_null(Some(&joined))
}

#[derive(Debug, Clone)]
pub struct LineInput {
    pub description: String,
    pub quantity: Option<Money>,
    pub unit_price: Option<Money>,
    pub account_id: Option<String>,
    /// D26: optional tax code for the line.
    pub tax_code_id: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct CreateInvoice {
    pub customer_id: Option<String>,
    pub date: String,
    pub due_date: Option<String>,
    pub payment_terms: Option<String>,
    pub po_number: Option<String>,
    pub notes: Option<String>,
    pub bill_to_name: Option<String>,
    pub bill_to_address: Option<String>,
    pub payment_options: Option<Vec<String>>,
    pub signatory_name: Option<String>,
    pub signatory_title: Option<String>,
    pub lines: Vec<LineInput>,
    /// D26: the line amounts already include tax.
    pub prices_include_tax: bool,
    /// D30: a foreign currency (None or the home currency = home) and its exchange rate.
    pub currency: Option<String>,
    pub exchange_rate: Option<i64>,
}

/// A line ready to store: its input plus resolved quantity, unit price, amount and tax.
struct PricedLine {
    input: LineInput,
    quantity: Money,
    unit_price: Money,
    amount: Money,
    tax: Money,
    parts: Vec<crate::tax::TaxPart>,
}

/// Blank quantity means one; blank price means the line carries no money. Returns the lines,
/// the invoice total (tax included) and the tax.
async fn build_lines(
    conn: &mut SqliteConnection,
    inputs: &[LineInput],
    prices_include_tax: bool,
) -> AppResult<(Vec<PricedLine>, Money, Money)> {
    let mut out = Vec::new();
    let (mut total, mut tax_total) = (Money::ZERO, Money::ZERO);
    for l in inputs {
        let quantity = l.quantity.unwrap_or(Money::from_cents(100));
        let unit_price = l.unit_price.unwrap_or(Money::ZERO);
        let amount = Money::line_amount(quantity, unit_price).ok_or_else(overflow)?;
        let (parts, tax) = match &l.tax_code_id {
            Some(code) => crate::tax::line_tax(conn, code, amount, prices_include_tax).await?,
            None => (Vec::new(), Money::ZERO),
        };
        let gross = if prices_include_tax { amount } else { amount.checked_add(tax).ok_or_else(overflow)? };
        total = total.checked_add(gross).ok_or_else(overflow)?;
        tax_total = tax_total.checked_add(tax).ok_or_else(overflow)?;
        out.push(PricedLine { input: l.clone(), quantity, unit_price, amount, tax, parts });
    }
    Ok((out, total, tax_total))
}

/// Stores the lines; returns the invoice total (tax included) and its tax.
async fn insert_lines(
    tx: &mut Tx,
    invoice_id: &str,
    inputs: &[LineInput],
    prices_include_tax: bool,
) -> AppResult<(Money, Money)> {
    for l in inputs {
        if let Some(a) = &l.account_id {
            require_account(tx, a).await?;
        }
    }
    let (lines, total, tax_total) = build_lines(tx, inputs, prices_include_tax).await?;
    for PricedLine { input: l, quantity, unit_price, amount, tax, parts } in lines {
        let line_id = new_id();
        sqlx::query(
            r#"INSERT INTO "invoice_lines" ("id","invoiceId","description","quantity","unitPrice","amount","accountId","taxCodeId","taxAmount")
               VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)"#,
        )
        .bind(&line_id)
        .bind(invoice_id)
        .bind(&l.description)
        .bind(quantity.cents())
        .bind(unit_price.cents())
        .bind(amount.cents())
        .bind(&l.account_id)
        .bind(&l.tax_code_id)
        .bind(tax.cents())
        .execute(&mut **tx)
        .await?;
        crate::tax::save_parts(tx, "INVOICE_LINE", &line_id, &parts).await?;
    }
    Ok((total, tax_total))
}

pub async fn create_in(tx: &mut Tx, dto: CreateInvoice, created_by: Option<&str>) -> AppResult<Invoice> {
    // A one-off invoice needs no customer record, but it must say who it is billed to.
    if dto.customer_id.is_none() && or_null(dto.bill_to_name.as_deref()).is_none() {
        return Err(AppError::bad_request("Select a customer or enter a Bill To name"));
    }
    if let Some(c) = &dto.customer_id {
        parties::find_one(tx, PartyKind::Customer, c).await?;
    }
    let number = counters::next_formatted(tx, "INV", 4).await?;
    let id = new_id();
    let now = now_iso();
    let due = resolve_due_date(&dto.date, dto.due_date.as_deref(), dto.payment_terms.as_deref());
    sqlx::query(
        r#"INSERT INTO "invoices" ("id","invoiceNumber","poNumber","customerId","date","dueDate","status","notes","billToName",
             "billToAddress","paymentOptions","signatoryName","signatoryTitle","paymentTerms","totalAmount","amountPaid",
             "createdById","createdAt","updatedAt")
           VALUES (?1,?2,?3,?4,?5,?6,'DRAFT',?7,?8,?9,?10,?11,?12,?13,0,0,?14,?15,?15)"#,
    )
    .bind(&id)
    .bind(&number)
    .bind(&dto.po_number)
    .bind(&dto.customer_id)
    .bind(&dto.date)
    .bind(&due)
    .bind(&dto.notes)
    .bind(or_null(dto.bill_to_name.as_deref()))
    .bind(or_null(dto.bill_to_address.as_deref()))
    .bind(dto.payment_options.as_deref().and_then(join_options))
    .bind(or_null(dto.signatory_name.as_deref()))
    .bind(or_null(dto.signatory_title.as_deref()))
    .bind(or_null(dto.payment_terms.as_deref()))
    .bind(created_by)
    .bind(&now)
    .execute(&mut **tx)
    .await?;
    let (currency, rate) = crate::fx::resolve(tx, dto.currency.as_deref(), dto.exchange_rate).await?;
    let (total, tax) = insert_lines(tx, &id, &dto.lines, dto.prices_include_tax).await?;
    sqlx::query(
        r#"UPDATE "invoices" SET "totalAmount" = ?1, "taxAmount" = ?2, "pricesIncludeTax" = ?3, "currency" = ?4,
             "exchangeRate" = ?5 WHERE "id" = ?6"#,
    )
    .bind(total.cents())
    .bind(tax.cents())
    .bind(dto.prices_include_tax)
    .bind(&currency)
    .bind(rate)
    .bind(&id)
    .execute(&mut **tx)
    .await?;
    crate::fx::remember_rate(tx, &currency, rate).await?;
    let mut inv = load(tx, &id).await?.ok_or_else(|| AppError::internal("created invoice vanished"))?;
    let before = inv.clone();
    inv.base_total = booked(tx, &inv).await?.total;
    save_header(tx, &before, &inv).await?;
    load(tx, &id).await?.ok_or_else(|| AppError::internal("created invoice vanished"))
}

pub async fn create(db: &Db, dto: CreateInvoice, created_by: Option<&str>) -> AppResult<Invoice> {
    let mut tx = db.begin().await?;
    let inv = create_in(&mut tx, dto, created_by).await?;
    tx.commit().await?;
    Ok(inv)
}

/// A PATCH body after validation: `None` = not sent, `Some(None)` = sent as null.
#[derive(Debug, Clone, Default)]
pub struct InvoicePatch {
    pub customer_id: Option<Option<String>>,
    pub date: Option<Option<String>>,
    pub due_date: Option<Option<String>>,
    pub payment_terms: Option<Option<String>>,
    pub po_number: Option<Option<String>>,
    pub notes: Option<Option<String>>,
    pub bill_to_name: Option<Option<String>>,
    pub bill_to_address: Option<Option<String>>,
    pub payment_options: Option<Option<Vec<String>>>,
    pub signatory_name: Option<Option<String>>,
    pub signatory_title: Option<Option<String>>,
    pub lines: Option<Option<Vec<LineInput>>>,
    /// D26.
    pub prices_include_tax: Option<bool>,
    /// D30.
    pub currency: Option<Option<String>>,
    pub exchange_rate: Option<i64>,
}

impl InvoicePatch {
    /// Anything other than the PO number was sent (null counts, as `!== undefined` did).
    fn touches_more_than_po(&self) -> bool {
        self.customer_id.is_some()
            || self.date.is_some()
            || self.due_date.is_some()
            || self.payment_terms.is_some()
            || self.notes.is_some()
            || self.bill_to_name.is_some()
            || self.bill_to_address.is_some()
            || self.payment_options.is_some()
            || self.signatory_name.is_some()
            || self.signatory_title.is_some()
            || self.lines.is_some()
            || self.prices_include_tax.is_some()
            || self.currency.is_some()
            || self.exchange_rate.is_some()
    }
}

/// Returns the invoice and the response view to use.
pub async fn update(db: &Db, id: &str, patch: InvoicePatch, scope: &Scope) -> AppResult<(Invoice, View)> {
    let mut tx = db.begin().await?;
    let mut inv = find_one(&mut tx, id, scope).await?;
    if inv.status == VOID {
        return Err(AppError::bad_request("Cannot edit a voided invoice"));
    }
    let before = inv.clone();
    if inv.status != DRAFT {
        // Once posted only the PO number moves: it carries no accounting meaning.
        if patch.touches_more_than_po() {
            return Err(AppError::bad_request(
                "Only the PO number can be changed once an invoice is posted — void it and raise a new one instead",
            ));
        }
        if let Some(po) = patch.po_number {
            inv.po_number = or_null(po.as_deref());
        }
        save_header(&mut tx, &before, &inv).await?;
        let out = load(&mut tx, id).await?.ok_or_else(|| not_found(id))?;
        tx.commit().await?;
        return Ok((out, View::Read));
    }

    if let Some(c) = patch.customer_id {
        let c = c.filter(|s| !s.is_empty());
        if let Some(cid) = &c {
            parties::find_one(&mut tx, PartyKind::Customer, cid).await?;
        }
        inv.customer_id = c;
    }
    if let Some(Some(d)) = &patch.date {
        inv.date = d.clone();
    }
    if let Some(t) = &patch.payment_terms {
        inv.payment_terms = or_null(t.as_deref());
    }
    if patch.due_date.is_some() || patch.payment_terms.is_some() || patch.date.is_some() {
        let terms = patch.payment_terms.clone().flatten().or_else(|| inv.payment_terms.clone());
        let due = patch.due_date.clone().flatten();
        if let Some(d) = resolve_due_date(&inv.date, due.as_deref(), terms.as_deref()) {
            inv.due_date = Some(d);
        }
    }
    if let Some(po) = &patch.po_number {
        inv.po_number = or_null(po.as_deref());
    }
    if let Some(n) = &patch.notes {
        inv.notes = n.clone();
    }
    if let Some(v) = &patch.bill_to_name {
        inv.bill_to_name = or_null(v.as_deref());
    }
    if let Some(v) = &patch.bill_to_address {
        inv.bill_to_address = or_null(v.as_deref());
    }
    if let Some(v) = &patch.payment_options {
        inv.payment_options = v.as_deref().and_then(join_options);
    }
    if let Some(v) = &patch.signatory_name {
        inv.signatory_name = or_null(v.as_deref());
    }
    if let Some(v) = &patch.signatory_title {
        inv.signatory_title = or_null(v.as_deref());
    }
    if inv.customer_id.is_none() && inv.bill_to_name.is_none() {
        return Err(AppError::bad_request("Select a customer or enter a Bill To name"));
    }
    let mut replaced = false;
    if let Some(include) = patch.prices_include_tax {
        inv.prices_include_tax = include;
    }
    // New lines, or the existing ones again when only "prices include tax" changed.
    let relines = match &patch.lines {
        Some(Some(lines)) => Some(lines.clone()),
        _ if patch.prices_include_tax.is_some() => Some(
            inv.lines
                .iter()
                .map(|l| LineInput {
                    description: l.description.clone(),
                    quantity: Some(l.quantity),
                    unit_price: Some(l.unit_price),
                    account_id: l.account_id.clone(),
                    tax_code_id: l.tax_code_id.clone(),
                })
                .collect(),
        ),
        _ => None,
    };
    if let Some(lines) = relines {
        if lines.is_empty() {
            return Err(AppError::bad_request("An invoice needs at least one line"));
        }
        let old: Vec<String> = inv.lines.iter().map(|l| l.id.clone()).collect();
        crate::tax::delete_parts(&mut tx, "INVOICE_LINE", &old).await?;
        sqlx::query(r#"DELETE FROM "invoice_lines" WHERE "invoiceId" = ?1"#).bind(id).execute(&mut *tx).await?;
        let (total, tax) = insert_lines(&mut tx, id, &lines, inv.prices_include_tax).await?;
        inv.total_amount = total;
        inv.tax_amount = tax;
        replaced = true;
    }
    // D30: a draft may change currency or rate; its home-currency total follows.
    if patch.currency.is_some() || patch.exchange_rate.is_some() {
        let code = match &patch.currency {
            Some(c) => c.clone(),
            None => inv.currency.clone(),
        };
        let rate = patch.exchange_rate.or((code == inv.currency).then_some(inv.exchange_rate));
        (inv.currency, inv.exchange_rate) = crate::fx::resolve(&mut tx, code.as_deref(), rate).await?;
        crate::fx::remember_rate(&mut tx, &inv.currency, inv.exchange_rate).await?;
    }
    if replaced {
        inv = Invoice { lines: load(&mut tx, id).await?.ok_or_else(|| not_found(id))?.lines, ..inv };
    }
    inv.base_total = booked(&mut tx, &inv).await?.total;
    save_header(&mut tx, &before, &inv).await?;
    let out = load(&mut tx, id).await?.ok_or_else(|| not_found(id))?;
    tx.commit().await?;
    Ok((out, if replaced { View::UpdatedLines } else { View::Read }))
}

/// Writes the header columns; `updatedAt` moves only when a column changed (TypeORM).
async fn save_header(tx: &mut Tx, before: &Invoice, after: &Invoice) -> AppResult<()> {
    let changed = before.po_number != after.po_number
        || before.customer_id != after.customer_id
        || before.date != after.date
        || before.due_date != after.due_date
        || before.status != after.status
        || before.notes != after.notes
        || before.bill_to_name != after.bill_to_name
        || before.bill_to_address != after.bill_to_address
        || before.payment_options != after.payment_options
        || before.signatory_name != after.signatory_name
        || before.signatory_title != after.signatory_title
        || before.payment_terms != after.payment_terms
        || before.total_amount != after.total_amount
        || before.amount_paid != after.amount_paid
        || before.tax_amount != after.tax_amount
        || before.prices_include_tax != after.prices_include_tax
        || before.currency != after.currency
        || before.exchange_rate != after.exchange_rate
        || before.base_total != after.base_total
        || before.base_amount_paid != after.base_amount_paid
        || before.base_amount_credited != after.base_amount_credited
        || before.journal_entry_id != after.journal_entry_id;
    if !changed {
        return Ok(());
    }
    sqlx::query(
        r#"UPDATE "invoices" SET "poNumber"=?1,"customerId"=?2,"date"=?3,"dueDate"=?4,"status"=?5,"notes"=?6,"billToName"=?7,
             "billToAddress"=?8,"paymentOptions"=?9,"signatoryName"=?10,"signatoryTitle"=?11,"paymentTerms"=?12,
             "totalAmount"=?13,"amountPaid"=?14,"journalEntryId"=?15,"updatedAt"=?16,"taxAmount"=?18,"pricesIncludeTax"=?19,
             "currency"=?20,"exchangeRate"=?21,"baseTotal"=?22,"baseAmountPaid"=?23,"baseAmountCredited"=?24
             WHERE "id"=?17"#,
    )
    .bind(&after.po_number)
    .bind(&after.customer_id)
    .bind(&after.date)
    .bind(&after.due_date)
    .bind(&after.status)
    .bind(&after.notes)
    .bind(&after.bill_to_name)
    .bind(&after.bill_to_address)
    .bind(&after.payment_options)
    .bind(&after.signatory_name)
    .bind(&after.signatory_title)
    .bind(&after.payment_terms)
    .bind(after.total_amount.cents())
    .bind(after.amount_paid.cents())
    .bind(&after.journal_entry_id)
    .bind(now_iso())
    .bind(&after.id)
    .bind(after.tax_amount.cents())
    .bind(after.prices_include_tax)
    .bind(&after.currency)
    .bind(after.exchange_rate)
    .bind(after.base_total.cents())
    .bind(after.base_amount_paid.cents())
    .bind(after.base_amount_credited.cents())
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// Deletes a DRAFT outright — it never reached the ledger.
pub async fn remove(db: &Db, id: &str, scope: &Scope) -> AppResult<()> {
    let mut tx = db.begin().await?;
    let inv = find_one(&mut tx, id, scope).await?;
    if inv.status != DRAFT {
        return Err(AppError::bad_request(if inv.status == VOID {
            "A voided invoice is kept for the record and cannot be deleted".to_string()
        } else {
            format!(
                "Invoice {} is {} and has posted to the ledger — void it instead of deleting",
                inv.invoice_number, inv.status
            )
        }));
    }
    let lines: Vec<String> = inv.lines.iter().map(|l| l.id.clone()).collect();
    crate::tax::delete_parts(&mut tx, "INVOICE_LINE", &lines).await?;
    sqlx::query(r#"DELETE FROM "invoices" WHERE "id" = ?1"#).bind(id).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(())
}

async fn post_in(tx: &mut Tx, id: &str, scope: &Scope) -> AppResult<Invoice> {
    let mut inv = find_one(tx, id, scope).await?;
    if inv.status != DRAFT {
        return Err(AppError::bad_request(format!("Invoice is already {}", inv.status)));
    }
    let missing = inv.lines.iter().filter(|l| l.account_id.is_none()).count();
    if missing > 0 {
        return Err(AppError::bad_request(format!(
            "Choose a revenue account for every line before posting — {missing} still blank"
        )));
    }
    let ar = ledger::accounts::find_by_code(tx, "1100").await?;
    // In the home currency (D30): each line's net and each tax part converted at the rate.
    let figures = booked(tx, &inv).await?;
    let mut lines = vec![NewLine { account_id: ar.id, debit: figures.total, credit: Money::ZERO, description: None }];
    // Revenue gets each line without its tax; the tax goes to the code's sales account (D26).
    for (l, net) in inv.lines.iter().zip(&figures.nets) {
        lines.push(NewLine {
            account_id: l.account_id.clone().unwrap_or_default(),
            debit: Money::ZERO,
            credit: *net,
            description: Some(l.description.clone()),
        });
    }
    // D27: each part of each line's tax to its own code's account.
    lines.extend(figures.taxes.into_iter().map(|(account_id, tax)| NewLine {
        account_id,
        debit: Money::ZERO,
        credit: tax,
        description: Some("Tax".into()),
    }));
    let entry = entries::post(
        tx,
        NewEntry {
            date: inv.date.clone(),
            description: format!("Invoice {}", inv.invoice_number),
            reference: Some(inv.invoice_number.clone()),
            source_type: Some("INVOICE".into()),
            source_id: Some(inv.id.clone()),
            lines,
        },
    )
    .await?;
    let before = inv.clone();
    inv.status = POSTED.into();
    inv.base_total = figures.total;
    inv.journal_entry_id = Some(entry.id);
    save_header(tx, &before, &inv).await?;
    load(tx, id).await?.ok_or_else(|| not_found(id))
}

async fn unpost_in(tx: &mut Tx, id: &str, scope: &Scope, today: &str) -> AppResult<Invoice> {
    let mut inv = find_one(tx, id, scope).await?;
    if inv.status != POSTED {
        return Err(AppError::bad_request(format!(
            "Only a posted invoice can go back to draft — this one is {}",
            inv.status
        )));
    }
    if inv.amount_paid > Money::ZERO {
        return Err(AppError::bad_request("Payments have been received — reverse them before returning this to draft"));
    }
    if inv.amount_credited > Money::ZERO {
        return Err(AppError::bad_request("Credit notes have been issued — void them before returning this to draft"));
    }
    let before = inv.clone();
    if let Some(je) = inv.journal_entry_id.take() {
        entries::reverse(tx, &je, today).await?;
    }
    inv.status = DRAFT.into();
    save_header(tx, &before, &inv).await?;
    load(tx, id).await?.ok_or_else(|| not_found(id))
}

async fn void_in(tx: &mut Tx, id: &str, scope: &Scope, today: &str) -> AppResult<Invoice> {
    let mut inv = find_one(tx, id, scope).await?;
    if inv.status == VOID {
        return Err(AppError::bad_request("Invoice is already voided"));
    }
    if inv.status == PAID {
        return Err(AppError::bad_request("Cannot void a paid invoice — reverse the payment first"));
    }
    if inv.amount_paid > Money::ZERO {
        return Err(AppError::bad_request("Cannot void a part-paid invoice — reverse the payments first"));
    }
    if inv.amount_credited > Money::ZERO {
        return Err(AppError::bad_request("Cannot void an invoice with credit notes — void the credit notes first"));
    }
    let before = inv.clone();
    if let Some(je) = &inv.journal_entry_id {
        entries::reverse(tx, je, today).await?;
    }
    inv.status = VOID.into();
    save_header(tx, &before, &inv).await?;
    load(tx, id).await?.ok_or_else(|| not_found(id))
}

pub async fn post(db: &Db, id: &str, scope: &Scope) -> AppResult<Invoice> {
    let mut tx = db.begin().await?;
    let out = post_in(&mut tx, id, scope).await?;
    tx.commit().await?;
    Ok(out)
}

pub async fn unpost(db: &Db, id: &str, scope: &Scope, today: &str) -> AppResult<Invoice> {
    let mut tx = db.begin().await?;
    let out = unpost_in(&mut tx, id, scope, today).await?;
    tx.commit().await?;
    Ok(out)
}

pub async fn void(db: &Db, id: &str, scope: &Scope, today: &str) -> AppResult<Invoice> {
    let mut tx = db.begin().await?;
    let out = void_in(&mut tx, id, scope, today).await?;
    tx.commit().await?;
    Ok(out)
}

/// Sets the status by hand, running the ledger work each move implies. PAID follows payments.
pub async fn change_status(db: &Db, id: &str, target: &str, scope: &Scope, today: &str) -> AppResult<Invoice> {
    let mut tx = db.begin().await?;
    let inv = find_one(&mut tx, id, scope).await?;
    let out = if inv.status == target {
        inv
    } else if target == PAID {
        return Err(AppError::bad_request(
            "PAID follows the money — record the payment on the Payments page and the invoice marks itself paid",
        ));
    } else if inv.status == PAID {
        return Err(AppError::bad_request("This invoice is paid — reverse the payment before changing its status"));
    } else if inv.status == VOID {
        return Err(AppError::bad_request("A voided invoice is final — copy it to a new draft instead"));
    } else if target == VOID {
        void_in(&mut tx, id, scope, today).await?
    } else if target == POSTED {
        post_in(&mut tx, id, scope).await?
    } else {
        unpost_in(&mut tx, id, scope, today).await?
    };
    tx.commit().await?;
    Ok(out)
}

/// Copies an invoice into a fresh DRAFT dated `today` — same lines and print details.
pub async fn duplicate(db: &Db, id: &str, scope: &Scope, today: &str) -> AppResult<Invoice> {
    let mut tx = db.begin().await?;
    let src = find_one(&mut tx, id, scope).await?;
    let dto = CreateInvoice {
        customer_id: src.customer_id.clone(),
        date: today.to_string(),
        due_date: None,
        payment_terms: src.payment_terms.clone(),
        po_number: src.po_number.clone(),
        notes: src.notes.clone(),
        bill_to_name: src.bill_to_name.clone().or_else(|| src.customer.as_ref().map(|c| c.name.clone())),
        bill_to_address: src.bill_to_address.clone().or_else(|| src.customer.as_ref().and_then(|c| c.address.clone())),
        payment_options: src.payment_options.as_ref().map(|p| p.split('\n').map(str::to_string).collect()),
        signatory_name: src.signatory_name.clone(),
        signatory_title: src.signatory_title.clone(),
        prices_include_tax: src.prices_include_tax,
        currency: src.currency.clone(),
        exchange_rate: Some(src.exchange_rate),
        lines: src
            .lines
            .iter()
            .map(|l| LineInput {
                description: l.description.clone(),
                quantity: Some(l.quantity),
                unit_price: Some(l.unit_price),
                account_id: l.account_id.clone(),
                tax_code_id: l.tax_code_id.clone(),
            })
            .collect(),
    };
    let inv = create_in(&mut tx, dto, scope.user_id.as_deref()).await?;
    tx.commit().await?;
    Ok(inv)
}

#[derive(Debug, Clone, Default)]
pub struct InvoiceQuery {
    pub page: i64,
    pub limit: i64,
    pub q: Option<String>,
    pub status: Option<String>,
    pub customer_id: Option<String>,
    pub date_from: Option<String>,
    pub date_to: Option<String>,
}

/// Reference `findAll`: newest first, filters combine with AND, free text over invoice number,
/// PO number, Bill To name and customer name (case-insensitive for ASCII, like ILIKE).
pub async fn find_all(db: &Db, query: &InvoiceQuery, scope: &Scope) -> AppResult<(Vec<Invoice>, i64)> {
    let mut wh = String::from(" WHERE 1 = 1");
    let mut binds: Vec<Option<String>> = Vec::new();
    if !scope.sees_all {
        wh.push_str(r#" AND i."createdById" = ?"#);
        binds.push(scope.user_id.clone());
    }
    for (col, val) in [(r#"i."status""#, &query.status), (r#"i."customerId""#, &query.customer_id)] {
        if let Some(v) = val {
            wh.push_str(&format!(" AND {col} = ?"));
            binds.push(Some(v.clone()));
        }
    }
    if let Some(v) = &query.date_from {
        wh.push_str(r#" AND i."date" >= ?"#);
        binds.push(Some(v.clone()));
    }
    if let Some(v) = &query.date_to {
        wh.push_str(r#" AND i."date" <= ?"#);
        binds.push(Some(v.clone()));
    }
    if let Some(q) = query.q.as_deref().map(str::trim).filter(|q| !q.is_empty()) {
        wh.push_str(
            r#" AND (i."invoiceNumber" LIKE ? OR i."poNumber" LIKE ? OR i."billToName" LIKE ? OR c."name" LIKE ?)"#,
        );
        let pattern = format!("%{q}%");
        for _ in 0..4 {
            binds.push(Some(pattern.clone()));
        }
    }
    let from = r#" FROM "invoices" i LEFT JOIN "customers" c ON c."id" = i."customerId""#;
    let mut conn = db.read().acquire().await?;
    let count_sql = format!("SELECT COUNT(*){from}{wh}");
    let mut cq = sqlx::query_scalar::<_, i64>(&count_sql);
    for b in &binds {
        cq = cq.bind(b);
    }
    let total = cq.fetch_one(&mut *conn).await?;
    let sql = format!(r#"SELECT {COLUMNS}{from}{wh} ORDER BY i."date" DESC, i."invoiceNumber" DESC LIMIT ? OFFSET ?"#);
    let mut q = sqlx::query(&sql);
    for b in &binds {
        q = q.bind(b);
    }
    let rows = q.bind(query.limit).bind((query.page - 1).saturating_mul(query.limit)).fetch_all(&mut *conn).await?;
    let mut invoices = rows.iter().map(from_row).collect::<AppResult<Vec<_>>>()?;
    attach(&mut conn, &mut invoices).await?;
    Ok((invoices, total))
}

/// For payments: the invoice regardless of scope (the reference's payments service ignored it).
pub(crate) async fn load_any(conn: &mut SqliteConnection, id: &str) -> AppResult<Invoice> {
    load(conn, id).await?.ok_or_else(|| not_found(id))
}

/// An invoice's figures in the home currency (D30): each line's net and each tax part
/// converted at the invoice's rate, and their sum — what the receivable is booked at.
pub(crate) struct Booked {
    pub total: Money,
    pub nets: Vec<Money>,
    pub taxes: Vec<(String, Money)>,
}

pub(crate) async fn booked(conn: &mut SqliteConnection, inv: &Invoice) -> AppResult<Booked> {
    let rate = inv.exchange_rate;
    let mut total = Money::ZERO;
    let mut nets = Vec::with_capacity(inv.lines.len());
    for l in &inv.lines {
        let net = crate::fx::to_base(l.net(inv.prices_include_tax)?, rate)?;
        total = total.checked_add(net).ok_or_else(overflow)?;
        nets.push(net);
    }
    let mut parts = Vec::new();
    for p in inv.lines.iter().flat_map(|l| l.taxes.clone()) {
        parts.push(crate::tax::TaxPart { amount: crate::fx::to_base(p.amount, rate)?, ..p });
    }
    let taxes = crate::tax::by_account(conn, parts, true).await?;
    for (_, t) in &taxes {
        total = total.checked_add(*t).ok_or_else(overflow)?;
    }
    Ok(Booked { total, nets, taxes })
}

impl Invoice {
    /// What is still owed in the home currency, as booked (D30).
    pub fn base_outstanding(&self) -> AppResult<Money> {
        self.base_total
            .checked_sub(self.base_amount_paid)
            .and_then(|m| m.checked_sub(self.base_amount_credited))
            .ok_or_else(overflow)
    }

    /// What the customer still owes: total - payments - credit notes.
    pub fn outstanding(&self) -> AppResult<Money> {
        self.total_amount
            .checked_sub(self.amount_paid)
            .and_then(|m| m.checked_sub(self.amount_credited))
            .ok_or_else(overflow)
    }
}

/// Status once payments or credits change: PAID when nothing is owed, POSTED again when
/// something is (after a void), otherwise unchanged. Used for invoices and bills alike.
pub(crate) fn settled_status(status: &str, total: Money, paid: Money, credited: Money) -> AppResult<&str> {
    let covered = paid.checked_add(credited).ok_or_else(overflow)?;
    Ok(match status {
        POSTED if covered >= total => PAID,
        PAID if covered < total => POSTED,
        other => other,
    })
}

/// Credits so far, in the invoice's currency and as booked in the home currency (D30).
pub(crate) async fn set_credited(
    tx: &mut Tx,
    id: &str,
    amount_credited: Money,
    base_credited: Money,
    status: &str,
) -> AppResult<()> {
    sqlx::query(
        r#"UPDATE "invoices" SET "amountCredited" = ?1, "baseAmountCredited" = ?2, "status" = ?3, "updatedAt" = ?4
           WHERE "id" = ?5"#,
    )
    .bind(amount_credited.cents())
    .bind(base_credited.cents())
    .bind(status)
    .bind(now_iso())
    .bind(id)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// Payments so far, in the invoice's currency and as booked in the home currency (D30).
pub(crate) async fn set_paid(
    tx: &mut Tx,
    inv: &Invoice,
    amount_paid: Money,
    base_paid: Money,
    status: &str,
) -> AppResult<()> {
    let mut after = inv.clone();
    after.amount_paid = amount_paid;
    after.base_amount_paid = base_paid;
    after.status = status.to_string();
    save_header(tx, inv, &after).await
}
