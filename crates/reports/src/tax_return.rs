//! Tax return (D26, requested 2026-10-20): what to report to the tax office for a period.
//!
//! Per tax code, by document date:
//! * Sales: posted or paid invoices' lines, less posted customer credit notes — the net
//!   amount and the output tax charged.
//! * Purchases: posted or paid bills' lines and posted expenses, less posted vendor debit
//!   notes — the net amount and the input tax paid.
//!
//! Drafts and voided documents never count. Lines without a tax code are shown on their own
//! row ("No tax code"), so total sales and purchases are complete.
//! Net tax = output tax − input tax: payable when positive, reclaimable when negative.

use app_core::money::Money;
use app_core::{AppError, AppResult};
use serde_json::{json, Value};
use sqlx::{Row, SqliteConnection};
use std::collections::BTreeMap;

/// The rate (percent × 10 000) as a plain JSON percentage: 18, 8.875, 0.
fn rate_json(rate: i64) -> Value {
    let text = format!("{}.{:04}", rate / 10_000, rate % 10_000);
    serde_json::from_str(text.trim_end_matches('0').trim_end_matches('.')).unwrap_or(json!(0))
}

fn overflow() -> AppError {
    AppError::internal("tax return overflow")
}

/// One movement: side ('S' sales / 'P' purchases), tax code (or NULL), net, tax.
const MOVEMENTS: &str = r#"
SELECT 'S' AS "side", l."taxCodeId" AS "code",
       CASE WHEN i."pricesIncludeTax" = 1 THEN l."amount" - l."taxAmount" ELSE l."amount" END AS "net",
       l."taxAmount" AS "tax"
  FROM "invoice_lines" l JOIN "invoices" i ON i."id" = l."invoiceId"
 WHERE i."status" IN ('POSTED', 'PAID') AND (?1 IS NULL OR i."date" >= ?1) AND (?2 IS NULL OR i."date" <= ?2)
UNION ALL
SELECT 'S', n."taxCodeId", -(n."amount" - n."taxAmount"), -n."taxAmount"
  FROM "credit_notes" n
 WHERE n."type" = 'AR' AND n."status" = 'POSTED' AND (?1 IS NULL OR n."date" >= ?1) AND (?2 IS NULL OR n."date" <= ?2)
UNION ALL
SELECT 'P', l."taxCodeId",
       CASE WHEN b."pricesIncludeTax" = 1 THEN l."amount" - l."taxAmount" ELSE l."amount" END,
       l."taxAmount"
  FROM "bill_lines" l JOIN "bills" b ON b."id" = l."billId"
 WHERE b."status" IN ('POSTED', 'PAID') AND (?1 IS NULL OR b."date" >= ?1) AND (?2 IS NULL OR b."date" <= ?2)
UNION ALL
SELECT 'P', e."taxCodeId", e."amount" - e."taxAmount", e."taxAmount"
  FROM "expenses" e
 WHERE e."status" = 'POSTED' AND (?1 IS NULL OR e."date" >= ?1) AND (?2 IS NULL OR e."date" <= ?2)
UNION ALL
SELECT 'P', n."taxCodeId", -(n."amount" - n."taxAmount"), -n."taxAmount"
  FROM "credit_notes" n
 WHERE n."type" = 'AP' AND n."status" = 'POSTED' AND (?1 IS NULL OR n."date" >= ?1) AND (?2 IS NULL OR n."date" <= ?2)"#;

#[derive(Default, Clone, Copy)]
struct Sums {
    sales_net: Money,
    output_tax: Money,
    purchases_net: Money,
    input_tax: Money,
}

impl Sums {
    fn add(&mut self, side: &str, net: Money, tax: Money) -> AppResult<()> {
        let add = |a: Money, b: Money| a.checked_add(b).ok_or_else(overflow);
        if side == "S" {
            self.sales_net = add(self.sales_net, net)?;
            self.output_tax = add(self.output_tax, tax)?;
        } else {
            self.purchases_net = add(self.purchases_net, net)?;
            self.input_tax = add(self.input_tax, tax)?;
        }
        Ok(())
    }

    fn json(&self) -> Value {
        json!({
            "salesNet": self.sales_net.to_json_number(),
            "outputTax": self.output_tax.to_json_number(),
            "purchasesNet": self.purchases_net.to_json_number(),
            "inputTax": self.input_tax.to_json_number(),
        })
    }
}

/// `date_from` / `date_to` are `YYYY-MM-DD`, or empty for "no bound".
pub async fn tax_return(conn: &mut SqliteConnection, date_from: &str, date_to: &str) -> AppResult<Value> {
    let from = (!date_from.is_empty()).then_some(date_from);
    let to = (!date_to.is_empty()).then_some(date_to);
    let rows = sqlx::query(MOVEMENTS).bind(from).bind(to).fetch_all(&mut *conn).await?;

    let mut by_code: BTreeMap<Option<String>, Sums> = BTreeMap::new();
    let mut total = Sums::default();
    for r in &rows {
        let side: String = r.try_get("side")?;
        let code: Option<String> = r.try_get("code")?;
        let net = Money::from_cents(r.try_get("net")?);
        let tax = Money::from_cents(r.try_get("tax")?);
        by_code.entry(code).or_default().add(&side, net, tax)?;
        total.add(&side, net, tax)?;
    }

    let codes = crate::tax_codes_by_id(conn).await?;
    let mut lines: Vec<(String, Value)> = by_code
        .into_iter()
        .map(|(code, sums)| {
            let (name, rate) = match code.as_ref().and_then(|c| codes.get(c)) {
                Some((name, rate)) => (name.clone(), rate_json(*rate)),
                None => ("No tax code".to_string(), Value::Null),
            };
            let mut v = sums.json();
            if let Value::Object(m) = &mut v {
                m.insert("taxCodeId".into(), json!(code));
                m.insert("name".into(), json!(name));
                m.insert("rate".into(), rate);
            }
            // Coded rows first, by name; "No tax code" last.
            (format!("{}{}", if code.is_some() { "0" } else { "1" }, name), v)
        })
        .collect();
    lines.sort_by(|a, b| a.0.cmp(&b.0));

    let net = total.output_tax.checked_sub(total.input_tax).ok_or_else(overflow)?;
    Ok(json!({
        "dateFrom": date_from,
        "dateTo": date_to,
        "rows": lines.into_iter().map(|(_, v)| v).collect::<Vec<_>>(),
        "totals": total.json(),
        "netTax": net.to_json_number(),
    }))
}
