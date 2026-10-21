//! Bills (reference `bills.service.ts`). Posting debits each line's account and credits the
//! bill's payable account — 2000 Accounts Payable unless another liability is chosen (D33) —
//! for the total; voiding reverses that entry.
//!
//! Approved deviations D13–D15: bills accumulate partial AP payments in `amountPaid` (a column
//! the reference lacked) and become PAID once nothing is outstanding; a DRAFT bill cannot be
//! paid; a part-paid bill cannot be voided. `amountPaid` is kept internal so the JSON shape of a
//! bill stays exactly the reference's.

use crate::parties::{self, Party, PartyKind};
use crate::{money_num, money_str, opt, overflow, require_account};
use app_core::ids::new_id;
use app_core::money::Money;
use app_core::time::now_iso;
use app_core::{AppError, AppResult};
use database::{counters, Db, Tx};
use ledger::{entries, NewEntry, NewLine};
use serde_json::{json, Map, Value};
use sqlx::sqlite::SqliteRow;
use sqlx::{Row, SqliteConnection};

#[derive(Debug, Clone)]
pub struct BillLine {
    pub id: String,
    pub bill_id: String,
    pub description: String,
    pub quantity: Money,
    pub unit_price: Money,
    pub amount: Money,
    pub account_id: String,
    /// D26: the line's tax code and its (input) tax, rounded half-up.
    pub tax_code_id: Option<String>,
    pub tax_amount: Money,
    /// D27: the tax split by standard code.
    pub taxes: Vec<crate::tax::TaxPart>,
}

#[derive(Debug, Clone)]
pub struct Bill {
    pub id: String,
    pub bill_number: String,
    pub vendor_id: String,
    pub date: String,
    pub due_date: Option<String>,
    pub status: String,
    pub notes: Option<String>,
    pub total_amount: Money,
    /// Sum of AP payments recorded against the bill (D13).
    pub amount_paid: Money,
    /// Taken off by posted debit notes (D21). Internal, like the amount paid.
    pub amount_credited: Money,
    /// D26: total tax on the lines, and whether the line amounts already include it.
    pub tax_amount: Money,
    pub prices_include_tax: bool,
    /// D30: the bill's currency (None = home), its exchange rate and its home-currency figures.
    pub currency: Option<String>,
    pub exchange_rate: i64,
    pub base_total: Money,
    pub base_amount_paid: Money,
    pub base_amount_credited: Money,
    pub journal_entry_id: Option<String>,
    /// D33: the liability the bill is owed on (None = 2000 Accounts Payable).
    pub payable_account_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub lines: Vec<BillLine>,
    pub vendor: Option<Party>,
}

/// A bill's figures in the home currency (D30), as `invoices::booked`.
pub(crate) async fn booked(conn: &mut SqliteConnection, bill: &Bill) -> AppResult<crate::invoices::Booked> {
    let rate = bill.exchange_rate;
    let mut total = Money::ZERO;
    let mut nets = Vec::with_capacity(bill.lines.len());
    for l in &bill.lines {
        let net =
            if bill.prices_include_tax { l.amount.checked_sub(l.tax_amount).ok_or_else(overflow)? } else { l.amount };
        let net = crate::fx::to_base(net, rate)?;
        total = total.checked_add(net).ok_or_else(overflow)?;
        nets.push(net);
    }
    let mut parts = Vec::new();
    for p in bill.lines.iter().flat_map(|l| l.taxes.clone()) {
        parts.push(crate::tax::TaxPart { amount: crate::fx::to_base(p.amount, rate)?, ..p });
    }
    let taxes = crate::tax::by_account(conn, parts, false).await?;
    for (_, t) in &taxes {
        total = total.checked_add(*t).ok_or_else(overflow)?;
    }
    Ok(crate::invoices::Booked { total, nets, taxes })
}

impl Bill {
    /// What is still owed in the home currency, as booked (D30).
    pub fn base_outstanding(&self) -> AppResult<Money> {
        self.base_total
            .checked_sub(self.base_amount_paid)
            .and_then(|m| m.checked_sub(self.base_amount_credited))
            .ok_or_else(overflow)
    }

    /// `created`: the insert echo (no `vendor`, sent price/amount as numbers).
    pub fn to_json(&self, created: bool) -> Value {
        let lines: Vec<Value> = self
            .lines
            .iter()
            .map(|l| {
                json!({
                    "id": l.id,
                    "billId": l.bill_id,
                    "description": l.description,
                    "quantity": money_str(l.quantity),
                    "unitPrice": if created { money_num(l.unit_price) } else { money_str(l.unit_price) },
                    "amount": if created { money_num(l.amount) } else { money_str(l.amount) },
                    "accountId": l.account_id,
                    "taxCodeId": opt(&l.tax_code_id),
                    "taxAmount": money_str(l.tax_amount),
                    "taxes": l.taxes.iter().map(|t| json!({
                        "taxCodeId": t.tax_code_id,
                        "taxAmount": money_str(t.amount),
                    })).collect::<Vec<_>>(),
                })
            })
            .collect();
        let mut m = Map::new();
        m.insert("id".into(), json!(self.id));
        m.insert("billNumber".into(), json!(self.bill_number));
        m.insert("vendorId".into(), json!(self.vendor_id));
        if !created {
            m.insert("vendor".into(), serde_json::to_value(&self.vendor).unwrap_or(Value::Null));
        }
        m.insert("date".into(), json!(self.date));
        m.insert("dueDate".into(), opt(&self.due_date));
        m.insert("status".into(), json!(self.status));
        m.insert("notes".into(), opt(&self.notes));
        m.insert("totalAmount".into(), money_str(self.total_amount));
        m.insert("taxAmount".into(), money_str(self.tax_amount));
        m.insert("pricesIncludeTax".into(), json!(self.prices_include_tax));
        m.insert("currency".into(), opt(&self.currency));
        m.insert("exchangeRate".into(), crate::fx::rate_json(self.exchange_rate));
        m.insert("baseTotal".into(), money_str(self.base_total));
        m.insert("journalEntryId".into(), opt(&self.journal_entry_id));
        m.insert("payableAccountId".into(), opt(&self.payable_account_id));
        m.insert("lines".into(), Value::Array(lines));
        m.insert("createdAt".into(), json!(self.created_at));
        m.insert("updatedAt".into(), json!(self.updated_at));
        Value::Object(m)
    }
}

const COLUMNS: &str = r#""id","billNumber","vendorId","date","dueDate","status","notes","totalAmount","amountPaid","amountCredited","taxAmount","pricesIncludeTax","currency","exchangeRate","baseTotal","baseAmountPaid","baseAmountCredited","journalEntryId","payableAccountId","createdAt","updatedAt""#;

fn from_row(r: &SqliteRow) -> AppResult<Bill> {
    Ok(Bill {
        id: r.try_get("id")?,
        bill_number: r.try_get("billNumber")?,
        vendor_id: r.try_get("vendorId")?,
        date: r.try_get("date")?,
        due_date: r.try_get("dueDate")?,
        status: r.try_get("status")?,
        notes: r.try_get("notes")?,
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
        payable_account_id: r.try_get("payableAccountId")?,
        created_at: r.try_get("createdAt")?,
        updated_at: r.try_get("updatedAt")?,
        lines: Vec::new(),
        vendor: None,
    })
}

async fn attach(conn: &mut SqliteConnection, bill: &mut Bill) -> AppResult<()> {
    let rows = sqlx::query(
        r#"SELECT "id","billId","description","quantity","unitPrice","amount","accountId","taxCodeId","taxAmount"
           FROM "bill_lines" WHERE "billId" = ?1 ORDER BY rowid"#,
    )
    .bind(&bill.id)
    .fetch_all(&mut *conn)
    .await?;
    bill.lines = rows
        .iter()
        .map(|r| {
            Ok(BillLine {
                id: r.try_get("id")?,
                bill_id: r.try_get("billId")?,
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
    let ids: Vec<String> = bill.lines.iter().map(|l| l.id.clone()).collect();
    let mut parts = crate::tax::load_parts(&mut *conn, "BILL_LINE", &ids).await?;
    for l in &mut bill.lines {
        l.taxes = parts.remove(&l.id).unwrap_or_default();
    }
    bill.vendor = parties::get(conn, PartyKind::Vendor, &bill.vendor_id).await?;
    Ok(())
}

pub async fn find_one(conn: &mut SqliteConnection, id: &str) -> AppResult<Bill> {
    let row = sqlx::query(&format!(r#"SELECT {COLUMNS} FROM "bills" WHERE "id" = ?1"#))
        .bind(id)
        .fetch_optional(&mut *conn)
        .await?;
    let Some(row) = row else { return Err(AppError::NotFound(format!("Bill {id} not found"))) };
    let mut bill = from_row(&row)?;
    attach(conn, &mut bill).await?;
    Ok(bill)
}

pub async fn find_all(db: &Db, page: i64, limit: i64) -> AppResult<(Vec<Bill>, i64)> {
    let mut conn = db.read().acquire().await?;
    let total: i64 = sqlx::query_scalar(r#"SELECT COUNT(*) FROM "bills""#).fetch_one(&mut *conn).await?;
    let rows =
        sqlx::query(&format!(r#"SELECT {COLUMNS} FROM "bills" ORDER BY "date" DESC, rowid ASC LIMIT ?1 OFFSET ?2"#))
            .bind(limit)
            .bind((page - 1).saturating_mul(limit))
            .fetch_all(&mut *conn)
            .await?;
    let mut bills = Vec::with_capacity(rows.len());
    for r in &rows {
        let mut b = from_row(r)?;
        attach(&mut conn, &mut b).await?;
        bills.push(b);
    }
    Ok((bills, total))
}

pub struct BillLineInput {
    pub description: String,
    pub quantity: Money,
    pub unit_price: Money,
    pub account_id: String,
    /// D26: optional tax code for the line.
    pub tax_code_id: Option<String>,
}

pub struct CreateBill {
    pub vendor_id: String,
    pub date: String,
    pub due_date: Option<String>,
    pub notes: Option<String>,
    pub lines: Vec<BillLineInput>,
    /// D26: the line amounts already include tax.
    pub prices_include_tax: bool,
    /// D30: a foreign currency (None or the home currency = home) and its exchange rate.
    pub currency: Option<String>,
    pub exchange_rate: Option<i64>,
    /// D33: the liability the bill is owed on (None = 2000 Accounts Payable).
    pub payable_account_id: Option<String>,
}

/// D33: a bill's payable account must be a liability (2000 by default).
pub async fn payable_account(conn: &mut SqliteConnection, id: Option<&str>) -> AppResult<ledger::Account> {
    let account = match id {
        Some(id) => ledger::accounts::find_one(conn, id).await?,
        None => ledger::accounts::find_by_code(conn, "2000").await?,
    };
    if account.account_type != ledger::AccountType::Liability {
        return Err(AppError::bad_request(format!(
            "{} {} is not a liability — choose a payable account",
            account.code, account.name
        )));
    }
    Ok(account)
}

pub async fn create(db: &Db, dto: CreateBill) -> AppResult<Bill> {
    let mut tx = db.begin().await?;
    parties::find_one(&mut tx, PartyKind::Vendor, &dto.vendor_id).await?;
    for l in &dto.lines {
        require_account(&mut tx, &l.account_id).await?;
    }
    if let Some(p) = &dto.payable_account_id {
        payable_account(&mut tx, Some(p)).await?;
    }
    let number = counters::next_formatted(&mut tx, "BILL", 6).await?;
    let id = new_id();
    let now = now_iso();
    let (currency, rate) = crate::fx::resolve(&mut tx, dto.currency.as_deref(), dto.exchange_rate).await?;
    crate::fx::remember_rate(&mut tx, &currency, rate).await?;
    let (mut total, mut tax_total) = (Money::ZERO, Money::ZERO);
    let mut amounts = Vec::with_capacity(dto.lines.len());
    for l in &dto.lines {
        let amount = Money::line_amount(l.quantity, l.unit_price).ok_or_else(overflow)?;
        let (parts, tax) = match &l.tax_code_id {
            Some(code) => crate::tax::line_tax(&mut tx, code, amount, dto.prices_include_tax).await?,
            None => (Vec::new(), Money::ZERO),
        };
        let gross = if dto.prices_include_tax { amount } else { amount.checked_add(tax).ok_or_else(overflow)? };
        total = total.checked_add(gross).ok_or_else(overflow)?;
        tax_total = tax_total.checked_add(tax).ok_or_else(overflow)?;
        amounts.push((amount, tax, parts));
    }
    sqlx::query(
        r#"INSERT INTO "bills" ("id","billNumber","vendorId","date","dueDate","status","notes","totalAmount","createdAt","updatedAt",
             "taxAmount","pricesIncludeTax","currency","exchangeRate","payableAccountId")
           VALUES (?1,?2,?3,?4,?5,'DRAFT',?6,?7,?8,?8,?9,?10,?11,?12,?13)"#,
    )
    .bind(&id)
    .bind(&number)
    .bind(&dto.vendor_id)
    .bind(&dto.date)
    .bind(&dto.due_date)
    .bind(&dto.notes)
    .bind(total.cents())
    .bind(&now)
    .bind(tax_total.cents())
    .bind(dto.prices_include_tax)
    .bind(&currency)
    .bind(rate)
    .bind(&dto.payable_account_id)
    .execute(&mut *tx)
    .await?;
    for (l, (amount, tax, parts)) in dto.lines.iter().zip(amounts) {
        let line_id = new_id();
        sqlx::query(
            r#"INSERT INTO "bill_lines" ("id","billId","description","quantity","unitPrice","amount","accountId","taxCodeId","taxAmount")
               VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)"#,
        )
        .bind(&line_id)
        .bind(&id)
        .bind(&l.description)
        .bind(l.quantity.cents())
        .bind(l.unit_price.cents())
        .bind(amount.cents())
        .bind(&l.account_id)
        .bind(&l.tax_code_id)
        .bind(tax.cents())
        .execute(&mut *tx)
        .await?;
        crate::tax::save_parts(&mut tx, "BILL_LINE", &line_id, &parts).await?;
    }
    let bill = find_one(&mut tx, &id).await?;
    // D30: what the payable is booked at in the home currency.
    let base_total = booked(&mut tx, &bill).await?.total;
    sqlx::query(r#"UPDATE "bills" SET "baseTotal" = ?1 WHERE "id" = ?2"#)
        .bind(base_total.cents())
        .bind(&id)
        .execute(&mut *tx)
        .await?;
    let bill = find_one(&mut tx, &id).await?;
    tx.commit().await?;
    Ok(bill)
}

impl Bill {
    /// What is still owed to the vendor: total - payments - debit notes.
    pub fn outstanding(&self) -> AppResult<Money> {
        self.total_amount
            .checked_sub(self.amount_paid)
            .and_then(|m| m.checked_sub(self.amount_credited))
            .ok_or_else(overflow)
    }
}

pub(crate) async fn set_credited(
    tx: &mut Tx,
    id: &str,
    amount_credited: Money,
    base_credited: Money,
    status: &str,
) -> AppResult<()> {
    sqlx::query(
        r#"UPDATE "bills" SET "amountCredited" = ?1, "status" = ?2, "updatedAt" = ?3, "baseAmountCredited" = ?5 WHERE "id" = ?4"#,
    )
    .bind(amount_credited.cents())
    .bind(status)
    .bind(now_iso())
    .bind(id)
    .bind(base_credited.cents())
    .execute(&mut **tx)
        .await?;
    Ok(())
}

pub(crate) async fn set_paid(
    tx: &mut Tx,
    id: &str,
    amount_paid: Money,
    base_paid: Money,
    status: &str,
) -> AppResult<()> {
    sqlx::query(r#"UPDATE "bills" SET "amountPaid" = ?1, "status" = ?2, "updatedAt" = ?3, "baseAmountPaid" = ?5 WHERE "id" = ?4"#)
        .bind(amount_paid.cents())
        .bind(status)
        .bind(now_iso())
        .bind(id)
        .bind(base_paid.cents())
        .execute(&mut **tx)
        .await?;
    Ok(())
}

pub(crate) async fn set_status(tx: &mut Tx, id: &str, status: &str, journal_entry_id: Option<&str>) -> AppResult<()> {
    sqlx::query(r#"UPDATE "bills" SET "status" = ?1, "journalEntryId" = COALESCE(?2, "journalEntryId"), "updatedAt" = ?3 WHERE "id" = ?4"#)
        .bind(status)
        .bind(journal_entry_id)
        .bind(now_iso())
        .bind(id)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

pub async fn post(db: &Db, id: &str) -> AppResult<Bill> {
    let mut tx = db.begin().await?;
    let bill = find_one(&mut tx, id).await?;
    if bill.status != "DRAFT" {
        return Err(AppError::bad_request(format!("Bill is already {}", bill.status)));
    }
    let ap = payable_account(&mut tx, bill.payable_account_id.as_deref()).await?;
    // In the home currency (D30). Each expense line without its tax; the tax goes to the code's
    // purchase account (D26), part by part (D27).
    let figures = booked(&mut tx, &bill).await?;
    let mut lines: Vec<NewLine> = Vec::new();
    for (l, net) in bill.lines.iter().zip(&figures.nets) {
        lines.push(NewLine {
            account_id: l.account_id.clone(),
            debit: *net,
            credit: Money::ZERO,
            description: Some(l.description.clone()),
        });
    }
    lines.extend(figures.taxes.into_iter().map(|(account_id, tax)| NewLine {
        account_id,
        debit: tax,
        credit: Money::ZERO,
        description: Some("Tax".into()),
    }));
    lines.push(NewLine { account_id: ap.id, debit: Money::ZERO, credit: figures.total, description: None });
    let entry = entries::post(
        &mut tx,
        NewEntry {
            date: bill.date.clone(),
            description: format!("Bill {}", bill.bill_number),
            reference: Some(bill.bill_number.clone()),
            source_type: Some("BILL".into()),
            source_id: Some(bill.id.clone()),
            lines,
        },
    )
    .await?;
    set_status(&mut tx, id, "POSTED", Some(&entry.id)).await?;
    let out = find_one(&mut tx, id).await?;
    tx.commit().await?;
    Ok(out)
}

pub async fn void(db: &Db, id: &str, today: &str) -> AppResult<Bill> {
    let mut tx = db.begin().await?;
    let bill = find_one(&mut tx, id).await?;
    if bill.status == "VOID" {
        return Err(AppError::bad_request("Bill is already voided"));
    }
    if bill.status == "PAID" {
        // The reference's source file garbled the dash ("â€”"); the intended text is sent.
        return Err(AppError::bad_request("Cannot void a paid bill — reverse the payment first"));
    }
    if bill.amount_credited > Money::ZERO {
        return Err(AppError::bad_request("Cannot void a bill with debit notes — void the debit notes first"));
    }
    if bill.amount_paid > Money::ZERO {
        // D15: voiding would leave the payments' debit to AP with no bill behind it.
        return Err(AppError::bad_request("Cannot void a part-paid bill — reverse the payments first"));
    }
    if let Some(je) = &bill.journal_entry_id {
        entries::reverse(&mut tx, je, today).await?;
    }
    set_status(&mut tx, id, "VOID", None).await?;
    let out = find_one(&mut tx, id).await?;
    tx.commit().await?;
    Ok(out)
}
