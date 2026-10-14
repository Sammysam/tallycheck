//! Payments (reference `payments.service.ts`). One transaction: the payment, its journal entry
//! and the invoice/bill status change commit together.
//!
//! * AR: debit the payment account, credit 1100 AR; the invoice accumulates `amountPaid` and
//!   becomes PAID when nothing is outstanding.
//! * AP: debit 2000 AP, credit the payment account; the bill becomes PAID only when this one
//!   payment covers its whole total (reference behaviour, see bills.rs).

use crate::invoices::{self, PAID};
use crate::parties::{self, PartyKind};
use crate::{bills, money_num, money_str, opt, overflow};
use app_core::ids::new_id;
use app_core::money::Money;
use app_core::time::now_iso;
use app_core::{AppError, AppResult};
use database::{counters, Db};
use ledger::{entries, NewEntry, NewLine};
use serde_json::{json, Value};
use sqlx::sqlite::SqliteRow;
use sqlx::{Row, SqliteConnection};

#[derive(Debug, Clone)]
pub struct Payment {
    pub id: String,
    pub payment_number: String,
    pub kind: String,
    pub invoice_id: Option<String>,
    pub bill_id: Option<String>,
    pub customer_id: Option<String>,
    pub vendor_id: Option<String>,
    pub date: String,
    pub amount: Money,
    pub payment_method: String,
    pub payment_account_id: String,
    pub notes: Option<String>,
    pub journal_entry_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl Payment {
    pub fn to_json(&self, created: bool) -> Value {
        json!({
            "id": self.id,
            "paymentNumber": self.payment_number,
            "type": self.kind,
            "invoiceId": opt(&self.invoice_id),
            "billId": opt(&self.bill_id),
            "customerId": opt(&self.customer_id),
            "vendorId": opt(&self.vendor_id),
            "date": self.date,
            "amount": if created { money_num(self.amount) } else { money_str(self.amount) },
            "paymentMethod": self.payment_method,
            "paymentAccountId": self.payment_account_id,
            "notes": opt(&self.notes),
            "journalEntryId": opt(&self.journal_entry_id),
            "createdAt": self.created_at,
            "updatedAt": self.updated_at,
        })
    }
}

const COLUMNS: &str = r#""id","paymentNumber","type","invoiceId","billId","customerId","vendorId","date","amount","paymentMethod","paymentAccountId","notes","journalEntryId","createdAt","updatedAt""#;

fn from_row(r: &SqliteRow) -> AppResult<Payment> {
    Ok(Payment {
        id: r.try_get("id")?,
        payment_number: r.try_get("paymentNumber")?,
        kind: r.try_get("type")?,
        invoice_id: r.try_get("invoiceId")?,
        bill_id: r.try_get("billId")?,
        customer_id: r.try_get("customerId")?,
        vendor_id: r.try_get("vendorId")?,
        date: r.try_get("date")?,
        amount: Money::from_cents(r.try_get("amount")?),
        payment_method: r.try_get("paymentMethod")?,
        payment_account_id: r.try_get("paymentAccountId")?,
        notes: r.try_get("notes")?,
        journal_entry_id: r.try_get("journalEntryId")?,
        created_at: r.try_get("createdAt")?,
        updated_at: r.try_get("updatedAt")?,
    })
}

pub async fn find_one(conn: &mut SqliteConnection, id: &str) -> AppResult<Payment> {
    let row = sqlx::query(&format!(r#"SELECT {COLUMNS} FROM "payments" WHERE "id" = ?1"#))
        .bind(id)
        .fetch_optional(conn)
        .await?;
    match row {
        Some(r) => from_row(&r),
        None => Err(AppError::NotFound(format!("Payment {id} not found"))),
    }
}

pub async fn find_all(db: &Db, page: i64, limit: i64) -> AppResult<(Vec<Payment>, i64)> {
    let total: i64 = sqlx::query_scalar(r#"SELECT COUNT(*) FROM "payments""#).fetch_one(db.read()).await?;
    let rows =
        sqlx::query(&format!(r#"SELECT {COLUMNS} FROM "payments" ORDER BY "date" DESC, rowid ASC LIMIT ?1 OFFSET ?2"#))
            .bind(limit)
            .bind((page - 1).saturating_mul(limit))
            .fetch_all(db.read())
            .await?;
    Ok((rows.iter().map(from_row).collect::<AppResult<Vec<_>>>()?, total))
}

pub struct CreatePayment {
    pub kind: String,
    pub invoice_id: Option<String>,
    pub bill_id: Option<String>,
    pub customer_id: Option<String>,
    pub vendor_id: Option<String>,
    pub date: String,
    pub amount: Money,
    pub payment_method: String,
    pub payment_account_id: String,
    pub notes: Option<String>,
}

pub async fn create(db: &Db, dto: CreatePayment) -> AppResult<Payment> {
    let is_ar = dto.kind == "AR";
    if is_ar && dto.invoice_id.is_none() {
        return Err(AppError::bad_request("invoiceId is required for AR payments"));
    }
    if !is_ar && dto.bill_id.is_none() {
        return Err(AppError::bad_request("billId is required for AP payments"));
    }
    let mut tx = db.begin().await?;

    // Validate the referenced documents before touching the ledger.
    let mut invoice = None;
    if let Some(iid) = &dto.invoice_id {
        let inv = invoices::load_any(&mut tx, iid).await?;
        if inv.status == invoices::VOID {
            return Err(AppError::bad_request("Cannot pay a voided invoice"));
        }
        if inv.status == invoices::DRAFT {
            return Err(AppError::bad_request("Post the invoice before recording a payment against it"));
        }
        let balance = inv.total_amount.checked_sub(inv.amount_paid).ok_or_else(overflow)?;
        if balance <= Money::ZERO {
            return Err(AppError::bad_request("Invoice is already fully paid"));
        }
        if dto.amount > balance {
            return Err(AppError::bad_request(format!("Payment exceeds the outstanding balance of {balance}")));
        }
        invoice = Some(inv);
    }
    let mut bill = None;
    if let Some(bid) = &dto.bill_id {
        let b = bills::find_one(&mut tx, bid).await?;
        if b.status == "VOID" {
            return Err(AppError::bad_request("Cannot pay a voided bill"));
        }
        bill = Some(b);
    }
    // The reference stored any customer/vendor id it was given; here they must exist (D6).
    if let Some(c) = &dto.customer_id {
        parties::find_one(&mut tx, PartyKind::Customer, c).await?;
    }
    if let Some(v) = &dto.vendor_id {
        parties::find_one(&mut tx, PartyKind::Vendor, v).await?;
    }

    let number = counters::next_formatted(&mut tx, "PAY", 6).await?;
    let (control_code, source_type, source_id, label) = if is_ar {
        ("1100", "PAYMENT_AR", dto.invoice_id.clone(), "AR")
    } else {
        ("2000", "PAYMENT_AP", dto.bill_id.clone(), "AP")
    };
    let control = ledger::accounts::find_by_code(&mut tx, control_code).await?;
    let lines = if is_ar {
        vec![
            NewLine {
                account_id: dto.payment_account_id.clone(),
                debit: dto.amount,
                credit: Money::ZERO,
                description: None,
            },
            NewLine { account_id: control.id, debit: Money::ZERO, credit: dto.amount, description: None },
        ]
    } else {
        vec![
            NewLine { account_id: control.id, debit: dto.amount, credit: Money::ZERO, description: None },
            NewLine {
                account_id: dto.payment_account_id.clone(),
                debit: Money::ZERO,
                credit: dto.amount,
                description: None,
            },
        ]
    };
    let entry = entries::post(
        &mut tx,
        NewEntry {
            date: dto.date.clone(),
            description: format!("{label} Payment {number}"),
            reference: Some(number.clone()),
            source_type: Some(source_type.into()),
            source_id,
            lines,
        },
    )
    .await?;

    if is_ar {
        if let Some(inv) = &invoice {
            // Partial payments accumulate — PAID only once the balance is cleared.
            let paid = inv.amount_paid.checked_add(dto.amount).ok_or_else(overflow)?;
            let status = if paid >= inv.total_amount { PAID } else { inv.status.as_str() };
            invoices::set_paid(&mut tx, inv, paid, status).await?;
        }
    } else if let Some(b) = &bill {
        if b.total_amount <= dto.amount {
            bills::set_status(&mut tx, &b.id, "PAID", None).await?;
        }
    }

    let id = new_id();
    let now = now_iso();
    sqlx::query(
        r#"INSERT INTO "payments" ("id","paymentNumber","type","invoiceId","billId","customerId","vendorId","date","amount",
             "paymentMethod","paymentAccountId","notes","journalEntryId","createdAt","updatedAt")
           VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?14)"#,
    )
    .bind(&id)
    .bind(&number)
    .bind(&dto.kind)
    .bind(&dto.invoice_id)
    .bind(&dto.bill_id)
    .bind(&dto.customer_id)
    .bind(&dto.vendor_id)
    .bind(&dto.date)
    .bind(dto.amount.cents())
    .bind(&dto.payment_method)
    .bind(&dto.payment_account_id)
    .bind(&dto.notes)
    .bind(&entry.id)
    .bind(&now)
    .execute(&mut *tx)
    .await?;
    let p = find_one(&mut tx, &id).await?;
    tx.commit().await?;
    Ok(p)
}
