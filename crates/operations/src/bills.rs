//! Bills (reference `bills.service.ts`). Posting debits each line's expense account and credits
//! 2000 Accounts Payable for the total; voiding reverses that entry.
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
    pub journal_entry_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub lines: Vec<BillLine>,
    pub vendor: Option<Party>,
}

impl Bill {
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
        m.insert("journalEntryId".into(), opt(&self.journal_entry_id));
        m.insert("lines".into(), Value::Array(lines));
        m.insert("createdAt".into(), json!(self.created_at));
        m.insert("updatedAt".into(), json!(self.updated_at));
        Value::Object(m)
    }
}

const COLUMNS: &str = r#""id","billNumber","vendorId","date","dueDate","status","notes","totalAmount","amountPaid","amountCredited","journalEntryId","createdAt","updatedAt""#;

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
        journal_entry_id: r.try_get("journalEntryId")?,
        created_at: r.try_get("createdAt")?,
        updated_at: r.try_get("updatedAt")?,
        lines: Vec::new(),
        vendor: None,
    })
}

async fn attach(conn: &mut SqliteConnection, bill: &mut Bill) -> AppResult<()> {
    let rows = sqlx::query(
        r#"SELECT "id","billId","description","quantity","unitPrice","amount","accountId"
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
            })
        })
        .collect::<AppResult<Vec<_>>>()?;
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
}

pub struct CreateBill {
    pub vendor_id: String,
    pub date: String,
    pub due_date: Option<String>,
    pub notes: Option<String>,
    pub lines: Vec<BillLineInput>,
}

pub async fn create(db: &Db, dto: CreateBill) -> AppResult<Bill> {
    let mut tx = db.begin().await?;
    parties::find_one(&mut tx, PartyKind::Vendor, &dto.vendor_id).await?;
    for l in &dto.lines {
        require_account(&mut tx, &l.account_id).await?;
    }
    let number = counters::next_formatted(&mut tx, "BILL", 6).await?;
    let id = new_id();
    let now = now_iso();
    let mut total = Money::ZERO;
    let mut amounts = Vec::with_capacity(dto.lines.len());
    for l in &dto.lines {
        let amount = Money::line_amount(l.quantity, l.unit_price).ok_or_else(overflow)?;
        total = total.checked_add(amount).ok_or_else(overflow)?;
        amounts.push(amount);
    }
    sqlx::query(
        r#"INSERT INTO "bills" ("id","billNumber","vendorId","date","dueDate","status","notes","totalAmount","createdAt","updatedAt")
           VALUES (?1,?2,?3,?4,?5,'DRAFT',?6,?7,?8,?8)"#,
    )
    .bind(&id)
    .bind(&number)
    .bind(&dto.vendor_id)
    .bind(&dto.date)
    .bind(&dto.due_date)
    .bind(&dto.notes)
    .bind(total.cents())
    .bind(&now)
    .execute(&mut *tx)
    .await?;
    for (l, amount) in dto.lines.iter().zip(amounts) {
        sqlx::query(
            r#"INSERT INTO "bill_lines" ("id","billId","description","quantity","unitPrice","amount","accountId")
               VALUES (?1,?2,?3,?4,?5,?6,?7)"#,
        )
        .bind(new_id())
        .bind(&id)
        .bind(&l.description)
        .bind(l.quantity.cents())
        .bind(l.unit_price.cents())
        .bind(amount.cents())
        .bind(&l.account_id)
        .execute(&mut *tx)
        .await?;
    }
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

pub(crate) async fn set_credited(tx: &mut Tx, id: &str, amount_credited: Money, status: &str) -> AppResult<()> {
    sqlx::query(r#"UPDATE "bills" SET "amountCredited" = ?1, "status" = ?2, "updatedAt" = ?3 WHERE "id" = ?4"#)
        .bind(amount_credited.cents())
        .bind(status)
        .bind(now_iso())
        .bind(id)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

pub(crate) async fn set_paid(tx: &mut Tx, id: &str, amount_paid: Money, status: &str) -> AppResult<()> {
    sqlx::query(r#"UPDATE "bills" SET "amountPaid" = ?1, "status" = ?2, "updatedAt" = ?3 WHERE "id" = ?4"#)
        .bind(amount_paid.cents())
        .bind(status)
        .bind(now_iso())
        .bind(id)
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
    let ap = ledger::accounts::find_by_code(&mut tx, "2000").await?;
    let mut lines: Vec<NewLine> = bill
        .lines
        .iter()
        .map(|l| NewLine {
            account_id: l.account_id.clone(),
            debit: l.amount,
            credit: Money::ZERO,
            description: Some(l.description.clone()),
        })
        .collect();
    lines.push(NewLine { account_id: ap.id, debit: Money::ZERO, credit: bill.total_amount, description: None });
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
