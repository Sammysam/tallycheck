//! Customer statements (D23, requested 2026-10-20; the reference had none).
//!
//! What a customer was charged and what they paid, in date order with a running balance:
//! posted invoices (POSTED or PAID; drafts and voided invoices never count) less posted
//! customer payments (with any tax the customer withheld, D27) and posted credit notes (voided ones never count). The balance brought
//! forward covers everything before `date_from`; with no end date the closing balance equals
//! what the customer still owes on their open invoices. A user without `invoices:all` sees
//! only invoices they created, as on the Invoices page.

use crate::invoices::Scope;
use crate::parties::{self, PartyKind};
use app_core::money::Money;
use app_core::{AppError, AppResult};
use serde_json::{json, Value};
use sqlx::{Row, SqliteConnection};

#[derive(Debug, Clone)]
pub struct StatementLine {
    pub date: String,
    /// INVOICE, PAYMENT or CREDIT_NOTE.
    pub kind: String,
    pub number: String,
    pub reference: Option<String>,
    pub due_date: Option<String>,
    pub charge: Money,
    pub credit: Money,
    pub balance: Money,
}

fn overflow() -> AppError {
    AppError::bad_request("Amount is too large")
}

/// Every statement movement of the customer, oldest first (invoices before payments on a day).
const MOVEMENTS: &str = r#"
SELECT * FROM (
  SELECT i."date" AS "date", 'INVOICE' AS "kind", 0 AS "ord", i."invoiceNumber" AS "number", i."poNumber" AS "reference",
         i."dueDate" AS "dueDate", i."totalAmount" AS "charge", 0 AS "credit"
    FROM "invoices" i
   WHERE i."customerId" = ?1 AND i."status" IN ('POSTED', 'PAID') AND (?2 = 1 OR i."createdById" IS ?3)
  UNION ALL
  SELECT p."date", 'PAYMENT', 1, p."paymentNumber", i."invoiceNumber", NULL, 0, p."amount" + p."withholdingAmount"
    FROM "payments" p JOIN "invoices" i ON i."id" = p."invoiceId"
   WHERE p."type" = 'AR' AND p."status" = 'POSTED' AND i."customerId" = ?1 AND (?2 = 1 OR i."createdById" IS ?3)
  UNION ALL
  SELECT n."date", 'CREDIT_NOTE', 2, n."noteNumber", i."invoiceNumber", NULL, 0, n."amount"
    FROM "credit_notes" n JOIN "invoices" i ON i."id" = n."invoiceId"
   WHERE n."type" = 'AR' AND n."status" = 'POSTED' AND i."customerId" = ?1 AND (?2 = 1 OR i."createdById" IS ?3)
) ORDER BY "date", "ord", "number""#;

pub async fn customer_statement(
    conn: &mut SqliteConnection,
    customer_id: &str,
    date_from: Option<&str>,
    date_to: Option<&str>,
    scope: &Scope,
) -> AppResult<Value> {
    let customer = parties::find_one(conn, PartyKind::Customer, customer_id).await?;
    let rows = sqlx::query(MOVEMENTS)
        .bind(customer_id)
        .bind(scope.sees_all)
        .bind(&scope.user_id)
        .fetch_all(&mut *conn)
        .await?;

    let mut opening = Money::ZERO;
    let mut running = Money::ZERO;
    let (mut charges, mut credits) = (Money::ZERO, Money::ZERO);
    let mut lines = Vec::new();
    for r in &rows {
        let date: String = r.try_get("date")?;
        let charge = Money::from_cents(r.try_get("charge")?);
        let credit = Money::from_cents(r.try_get("credit")?);
        if date_to.is_some_and(|to| date.as_str() > to) {
            continue;
        }
        let delta = charge.checked_sub(credit).ok_or_else(overflow)?;
        if date_from.is_some_and(|from| date.as_str() < from) {
            opening = opening.checked_add(delta).ok_or_else(overflow)?;
            running = opening;
            continue;
        }
        running = running.checked_add(delta).ok_or_else(overflow)?;
        charges = charges.checked_add(charge).ok_or_else(overflow)?;
        credits = credits.checked_add(credit).ok_or_else(overflow)?;
        lines.push(StatementLine {
            date,
            kind: r.try_get("kind")?,
            number: r.try_get("number")?,
            reference: r.try_get("reference")?,
            due_date: r.try_get("dueDate")?,
            charge,
            credit,
            balance: running,
        });
    }

    let lines: Vec<Value> = lines
        .iter()
        .map(|l| {
            json!({
                "date": l.date,
                "type": l.kind,
                "number": l.number,
                "reference": l.reference,
                "dueDate": l.due_date,
                "charge": l.charge.to_json_number(),
                "credit": l.credit.to_json_number(),
                "balance": l.balance.to_json_number(),
            })
        })
        .collect();
    Ok(json!({
        "customer": customer,
        "dateFrom": date_from,
        "dateTo": date_to,
        "openingBalance": opening.to_json_number(),
        "lines": lines,
        "totalCharges": charges.to_json_number(),
        "totalCredits": credits.to_json_number(),
        "closingBalance": running.to_json_number(),
    }))
}
