//! Payments (reference `payments.service.ts`). One transaction: the payment, its journal entry
//! and the invoice/bill status change commit together.
//!
//! * AR: debit the payment account, credit 1100 AR; the invoice accumulates `amountPaid` and
//!   becomes PAID when nothing is outstanding.
//! * AP: debit 2000 AP, credit the payment account; the bill accumulates `amountPaid` and
//!   becomes PAID when nothing is outstanding (D13). A DRAFT bill cannot be paid (D14).
//! * Void (D18): the payment's entry is reversed and the amount is outstanding again.
//! * Withholding (D27): the payer may hold back tax for the tax office. The amount withheld
//!   settles the document together with the money paid: on a customer's payment it is a tax
//!   credit (the withholding code's sales account, an asset); on our payment to a vendor it is
//!   owed to the tax office (the code's purchase account, a liability).
//! * Foreign currency (D30): the amounts are in the document's currency; the money moves in the
//!   home currency at the payment's rate. The receivable or payable is cleared at the amount it
//!   was booked at (all of what is left when the document is settled in full), and the
//!   difference is a realised exchange gain or loss on the exchange account.

use crate::invoices;
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
    /// D27: tax held back by the payer, and its withholding code.
    pub withholding_tax_code_id: Option<String>,
    pub withholding_amount: Money,
    /// D30: the rate on the payment date, the money in the home currency, the booked amount it
    /// cleared, and the exchange difference (booked − received − withheld, home currency).
    pub exchange_rate: i64,
    pub base_amount: Money,
    pub base_settled: Money,
    pub exchange_difference: Money,
    /// The paid document's currency (None = home), for display.
    pub currency: Option<String>,
    /// POSTED, or VOID once reversed (D18).
    pub status: String,
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
            "withholdingTaxCodeId": opt(&self.withholding_tax_code_id),
            "withholdingAmount": money_str(self.withholding_amount),
            "exchangeRate": crate::fx::rate_json(self.exchange_rate),
            "baseAmount": money_str(self.base_amount),
            "baseSettled": money_str(self.base_settled),
            "exchangeDifference": money_str(self.exchange_difference),
            "currency": opt(&self.currency),
            "journalEntryId": opt(&self.journal_entry_id),
            "status": self.status,
            "createdAt": self.created_at,
            "updatedAt": self.updated_at,
        })
    }
}

const COLUMNS: &str = r#""id","paymentNumber","type","invoiceId","billId","customerId","vendorId","date","amount","paymentMethod","paymentAccountId","notes","journalEntryId","status","withholdingTaxCodeId","withholdingAmount","exchangeRate","baseAmount","baseSettled","exchangeDifference","createdAt","updatedAt",
    COALESCE((SELECT "currency" FROM "invoices" WHERE "id" = "payments"."invoiceId"),
             (SELECT "currency" FROM "bills" WHERE "id" = "payments"."billId")) AS "currency""#;

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
        withholding_tax_code_id: r.try_get("withholdingTaxCodeId")?,
        withholding_amount: Money::from_cents(r.try_get("withholdingAmount")?),
        exchange_rate: r.try_get("exchangeRate")?,
        base_amount: Money::from_cents(r.try_get("baseAmount")?),
        base_settled: Money::from_cents(r.try_get("baseSettled")?),
        exchange_difference: Money::from_cents(r.try_get("exchangeDifference")?),
        currency: r.try_get("currency")?,
        journal_entry_id: r.try_get("journalEntryId")?,
        status: r.try_get("status")?,
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
    /// D27: tax held back by the payer (0 for none) and its withholding code.
    pub withholding_tax_code_id: Option<String>,
    pub withholding_amount: Money,
    /// D30: for a foreign-currency document, the rate on the payment date (None = its own rate).
    pub exchange_rate: Option<i64>,
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

    // D27: what settles the document is the money paid plus any tax withheld.
    let withholding = match (&dto.withholding_tax_code_id, dto.withholding_amount > Money::ZERO) {
        (Some(code), true) => {
            let c = crate::tax::usable(&mut tx, code).await?;
            if c.kind != crate::tax::WITHHOLDING {
                return Err(AppError::bad_request(format!("\"{}\" is not a withholding tax code", c.name)));
            }
            Some(c)
        }
        (None, true) => return Err(AppError::bad_request("Choose the withholding tax code for the amount withheld")),
        (Some(_), false) => {
            return Err(AppError::bad_request("Enter the amount withheld, or clear the withholding tax"))
        }
        (None, false) => None,
    };
    let settled = dto.amount.checked_add(dto.withholding_amount).ok_or_else(overflow)?;

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
        let balance = inv.outstanding()?;
        if balance <= Money::ZERO {
            return Err(AppError::bad_request("Invoice is already fully paid"));
        }
        if settled > balance {
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
        // D14–D13: the same rules invoices follow.
        if b.status == "DRAFT" {
            return Err(AppError::bad_request("Post the bill before recording a payment against it"));
        }
        let balance = b.outstanding()?;
        if balance <= Money::ZERO {
            return Err(AppError::bad_request("Bill is already fully paid"));
        }
        if !is_ar && settled > balance {
            return Err(AppError::bad_request(format!("Payment exceeds the outstanding balance of {balance}")));
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

    // D30: the document's currency and booked rate, and what is still owed on it.
    let (doc_rate, foreign, outstanding, base_outstanding) = match (&invoice, &bill) {
        (Some(i), _) => (i.exchange_rate, i.currency.is_some(), i.outstanding()?, i.base_outstanding()?),
        (_, Some(b)) => (b.exchange_rate, b.currency.is_some(), b.outstanding()?, b.base_outstanding()?),
        _ => (crate::fx::RATE_ONE, false, Money::ZERO, Money::ZERO),
    };
    let rate = if foreign { dto.exchange_rate.filter(|r| *r > 0).unwrap_or(doc_rate) } else { crate::fx::RATE_ONE };
    let base_amount = crate::fx::to_base(dto.amount, rate)?;
    let base_withheld = crate::fx::to_base(dto.withholding_amount, rate)?;
    // What this clears off the books: the booked share, or everything left when settled in full.
    let base_settled = if settled == outstanding { base_outstanding } else { crate::fx::to_base(settled, doc_rate)? };
    let difference =
        base_settled.checked_sub(base_amount).and_then(|d| d.checked_sub(base_withheld)).ok_or_else(overflow)?;

    let number = counters::next_formatted(&mut tx, "PAY", 6).await?;
    let (control_code, source_type, source_id, label) = if is_ar {
        ("1100", "PAYMENT_AR", dto.invoice_id.clone(), "AR")
    } else {
        ("2000", "PAYMENT_AP", dto.bill_id.clone(), "AP")
    };
    let control = ledger::accounts::find_by_code(&mut tx, control_code).await?;
    let mut lines = if is_ar {
        vec![
            NewLine {
                account_id: dto.payment_account_id.clone(),
                debit: base_amount,
                credit: Money::ZERO,
                description: None,
            },
            NewLine { account_id: control.id, debit: Money::ZERO, credit: base_settled, description: None },
        ]
    } else {
        vec![
            NewLine { account_id: control.id, debit: base_settled, credit: Money::ZERO, description: None },
            NewLine {
                account_id: dto.payment_account_id.clone(),
                debit: Money::ZERO,
                credit: base_amount,
                description: None,
            },
        ]
    };
    // D30: booked − (received + withheld). On a customer payment a positive difference is a loss
    // (debit); on a vendor payment it is a gain (credit) — and the other way round.
    if difference != Money::ZERO {
        let account_id = crate::fx::exchange_account(&mut tx).await?;
        let abs = if difference > Money::ZERO {
            difference
        } else {
            Money::ZERO.checked_sub(difference).ok_or_else(overflow)?
        };
        let debit_side = (difference > Money::ZERO) == is_ar;
        lines.push(NewLine {
            account_id,
            debit: if debit_side { abs } else { Money::ZERO },
            credit: if debit_side { Money::ZERO } else { abs },
            description: Some("Exchange difference".into()),
        });
    }
    if let Some(code) = &withholding {
        let w = base_withheld;
        lines.push(if is_ar {
            NewLine {
                account_id: code.sales_account_id.clone(),
                debit: w,
                credit: Money::ZERO,
                description: Some("Tax withheld by the customer".into()),
            }
        } else {
            NewLine {
                account_id: code.purchase_account_id.clone(),
                debit: Money::ZERO,
                credit: w,
                description: Some("Tax withheld from the vendor".into()),
            }
        });
    }
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
            let paid = inv.amount_paid.checked_add(settled).ok_or_else(overflow)?;
            let base_paid = inv.base_amount_paid.checked_add(base_settled).ok_or_else(overflow)?;
            let status = invoices::settled_status(&inv.status, inv.total_amount, paid, inv.amount_credited)?;
            invoices::set_paid(&mut tx, inv, paid, base_paid, status).await?;
        }
    } else if let Some(b) = &bill {
        let paid = b.amount_paid.checked_add(settled).ok_or_else(overflow)?;
        let base_paid = b.base_amount_paid.checked_add(base_settled).ok_or_else(overflow)?;
        let status = invoices::settled_status(&b.status, b.total_amount, paid, b.amount_credited)?;
        bills::set_paid(&mut tx, &b.id, paid, base_paid, status).await?;
    }
    if foreign {
        let currency =
            invoice.as_ref().and_then(|i| i.currency.clone()).or(bill.as_ref().and_then(|b| b.currency.clone()));
        crate::fx::remember_rate(&mut tx, &currency, rate).await?;
    }

    let id = new_id();
    let now = now_iso();
    sqlx::query(
        r#"INSERT INTO "payments" ("id","paymentNumber","type","invoiceId","billId","customerId","vendorId","date","amount",
             "paymentMethod","paymentAccountId","notes","journalEntryId","createdAt","updatedAt","withholdingTaxCodeId","withholdingAmount",
             "exchangeRate","baseAmount","baseSettled","exchangeDifference")
           VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?14,?15,?16,?17,?18,?19,?20)"#,
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
    .bind(withholding.as_ref().map(|c| c.id.clone()))
    .bind(dto.withholding_amount.cents())
    .bind(rate)
    .bind(base_amount.cents())
    .bind(base_settled.cents())
    .bind(difference.cents())
    .execute(&mut *tx)
    .await?;
    let p = find_one(&mut tx, &id).await?;
    tx.commit().await?;
    Ok(p)
}

/// D18: voids a payment. Its journal entry is reversed (dated `today`), the invoice or bill
/// takes the amount back as outstanding — PAID returns to POSTED — and the payment row is
/// kept, marked VOID, so the history stays complete. All in one transaction.
pub async fn void(db: &Db, id: &str, today: &str) -> AppResult<Payment> {
    let mut tx = db.begin().await?;
    let p = find_one(&mut tx, id).await?;
    if p.status == "VOID" {
        return Err(AppError::bad_request("Payment is already voided"));
    }
    if let Some(je) = &p.journal_entry_id {
        entries::reverse(&mut tx, je, today).await?;
    }
    let settled = p.amount.checked_add(p.withholding_amount).ok_or_else(overflow)?;
    if let Some(iid) = &p.invoice_id {
        let inv = invoices::load_any(&mut tx, iid).await?;
        let paid = inv.amount_paid.checked_sub(settled).ok_or_else(overflow)?.max(Money::ZERO);
        let base_paid = inv.base_amount_paid.checked_sub(p.base_settled).ok_or_else(overflow)?.max(Money::ZERO);
        let status = invoices::settled_status(&inv.status, inv.total_amount, paid, inv.amount_credited)?;
        invoices::set_paid(&mut tx, &inv, paid, base_paid, status).await?;
    }
    if let Some(bid) = &p.bill_id {
        let b = bills::find_one(&mut tx, bid).await?;
        let paid = b.amount_paid.checked_sub(settled).ok_or_else(overflow)?.max(Money::ZERO);
        let base_paid = b.base_amount_paid.checked_sub(p.base_settled).ok_or_else(overflow)?.max(Money::ZERO);
        let status = invoices::settled_status(&b.status, b.total_amount, paid, b.amount_credited)?;
        bills::set_paid(&mut tx, &b.id, paid, base_paid, status).await?;
    }
    sqlx::query(r#"UPDATE "payments" SET "status" = 'VOID', "updatedAt" = ?1 WHERE "id" = ?2"#)
        .bind(now_iso())
        .bind(id)
        .execute(&mut *tx)
        .await?;
    let out = find_one(&mut tx, id).await?;
    tx.commit().await?;
    Ok(out)
}
