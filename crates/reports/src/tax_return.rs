//! Tax return (D26, D27): what to report to the tax office for a period.
//!
//! Per standard tax code, by document date:
//! * Sales: posted or paid invoices' lines, less posted customer credit notes — the net
//!   amount the tax was charged on and the output tax.
//! * Purchases: posted or paid bills' lines and posted expenses, less posted vendor debit
//!   notes — the net amount and the input tax.
//!
//! A tax group's parts each have their own row (they may go to different authorities), so a
//! line taxed by a group appears under each part; the totals count every line once. Drafts and
//! voided documents never count. Lines without tax are on their own row ("No tax code").
//! Net tax = output tax − input tax: payable when positive, reclaimable when negative.
//!
//! Foreign-currency documents (D30) are converted at their own rate, as their entries were.
//!
//! Withholding (D27): tax customers held back from their payments (a credit to claim) and tax
//! held back from vendors (owed to the tax office), per withholding code, by payment date.

use app_core::money::Money;
use app_core::{AppError, AppResult};
use serde_json::{json, Value};
use sqlx::{Row, SqliteConnection};
use std::collections::{BTreeMap, HashSet};

/// The rate (percent × 10 000) as a plain JSON percentage: 18, 8.875, 0.
fn rate_json(rate: i64) -> Value {
    let text = format!("{}.{:04}", rate / 10_000, rate % 10_000);
    serde_json::from_str(text.trim_end_matches('0').trim_end_matches('.')).unwrap_or(json!(0))
}

/// D30: a document-currency amount in the home currency at `rate` (hundred-millionths),
/// rounded as the document's entry was.
fn to_base(amount: Money, rate: i64) -> AppResult<Money> {
    const ONE: i128 = 100_000_000;
    if i128::from(rate) == ONE {
        return Ok(amount);
    }
    let step = i128::from(app_core::money::rounding_step());
    let (n, d) = (i128::from(amount.cents()) * i128::from(rate), ONE * step);
    let (q, r) = (n / d, n % d);
    let units = if 2 * r.abs() >= d { q + n.signum() } else { q };
    i64::try_from(units * step).map(Money::from_cents).map_err(|_| overflow())
}

fn overflow() -> AppError {
    AppError::internal("tax return overflow")
}

/// One part of one line: side ('S' / 'P'), the line, the standard code (or NULL), net, tax.
const MOVEMENTS: &str = r#"
SELECT 'S' AS "side", 'I:' || l."id" AS "line", dt."taxCodeId" AS "code",
       CASE WHEN i."pricesIncludeTax" = 1 THEN l."amount" - l."taxAmount" ELSE l."amount" END AS "net",
       COALESCE(dt."taxAmount", 0) AS "tax", i."exchangeRate" AS "rate"
  FROM "invoice_lines" l JOIN "invoices" i ON i."id" = l."invoiceId"
  LEFT JOIN "document_taxes" dt ON dt."sourceType" = 'INVOICE_LINE' AND dt."sourceId" = l."id"
 WHERE i."status" IN ('POSTED', 'PAID') AND (?1 IS NULL OR i."date" >= ?1) AND (?2 IS NULL OR i."date" <= ?2)
UNION ALL
SELECT 'S', 'N:' || n."id", dt."taxCodeId", -(n."amount" - n."taxAmount"), -COALESCE(dt."taxAmount", 0), ni."exchangeRate"
  FROM "credit_notes" n JOIN "invoices" ni ON ni."id" = n."invoiceId"
  LEFT JOIN "document_taxes" dt ON dt."sourceType" = 'CREDIT_NOTE' AND dt."sourceId" = n."id"
 WHERE n."type" = 'AR' AND n."status" = 'POSTED' AND (?1 IS NULL OR n."date" >= ?1) AND (?2 IS NULL OR n."date" <= ?2)
UNION ALL
SELECT 'P', 'B:' || l."id", dt."taxCodeId",
       CASE WHEN b."pricesIncludeTax" = 1 THEN l."amount" - l."taxAmount" ELSE l."amount" END,
       COALESCE(dt."taxAmount", 0), b."exchangeRate"
  FROM "bill_lines" l JOIN "bills" b ON b."id" = l."billId"
  LEFT JOIN "document_taxes" dt ON dt."sourceType" = 'BILL_LINE' AND dt."sourceId" = l."id"
 WHERE b."status" IN ('POSTED', 'PAID') AND (?1 IS NULL OR b."date" >= ?1) AND (?2 IS NULL OR b."date" <= ?2)
UNION ALL
SELECT 'P', 'E:' || e."id", dt."taxCodeId", e."amount" - e."taxAmount", COALESCE(dt."taxAmount", 0), 100000000
  FROM "expenses" e
  LEFT JOIN "document_taxes" dt ON dt."sourceType" = 'EXPENSE' AND dt."sourceId" = e."id"
 WHERE e."status" = 'POSTED' AND (?1 IS NULL OR e."date" >= ?1) AND (?2 IS NULL OR e."date" <= ?2)
UNION ALL
SELECT 'P', 'D:' || n."id", dt."taxCodeId", -(n."amount" - n."taxAmount"), -COALESCE(dt."taxAmount", 0), nb."exchangeRate"
  FROM "credit_notes" n JOIN "bills" nb ON nb."id" = n."billId"
  LEFT JOIN "document_taxes" dt ON dt."sourceType" = 'CREDIT_NOTE' AND dt."sourceId" = n."id"
 WHERE n."type" = 'AP' AND n."status" = 'POSTED' AND (?1 IS NULL OR n."date" >= ?1) AND (?2 IS NULL OR n."date" <= ?2)"#;

const WITHHELD: &str = r#"
SELECT p."type" AS "side", p."withholdingTaxCodeId" AS "code", p."withholdingAmount" AS "amount", p."exchangeRate" AS "rate"
  FROM "payments" p
 WHERE p."status" = 'POSTED' AND p."withholdingAmount" > 0
   AND (?1 IS NULL OR p."date" >= ?1) AND (?2 IS NULL OR p."date" <= ?2)"#;

#[derive(Default, Clone, Copy)]
struct Sums {
    sales_net: Money,
    output_tax: Money,
    purchases_net: Money,
    input_tax: Money,
}

fn add(a: Money, b: Money) -> AppResult<Money> {
    a.checked_add(b).ok_or_else(overflow)
}

impl Sums {
    fn add_net(&mut self, side: &str, net: Money) -> AppResult<()> {
        if side == "S" {
            self.sales_net = add(self.sales_net, net)?;
        } else {
            self.purchases_net = add(self.purchases_net, net)?;
        }
        Ok(())
    }

    fn add_tax(&mut self, side: &str, tax: Money) -> AppResult<()> {
        if side == "S" {
            self.output_tax = add(self.output_tax, tax)?;
        } else {
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
    let mut counted: HashSet<String> = HashSet::new();
    for r in &rows {
        let side: String = r.try_get("side")?;
        let line: String = r.try_get("line")?;
        let code: Option<String> = r.try_get("code")?;
        let rate: i64 = r.try_get("rate")?;
        let net = to_base(Money::from_cents(r.try_get("net")?), rate)?;
        let tax = to_base(Money::from_cents(r.try_get("tax")?), rate)?;
        let row = by_code.entry(code).or_default();
        row.add_net(&side, net)?;
        row.add_tax(&side, tax)?;
        total.add_tax(&side, tax)?;
        // A line under several parts of a group is one sale: count its net once.
        if counted.insert(line) {
            total.add_net(&side, net)?;
        }
    }

    let codes = crate::tax_codes_by_id(conn).await?;
    let named = |code: &Option<String>| match code.as_ref().and_then(|c| codes.get(c)) {
        Some((name, rate)) => (name.clone(), rate_json(*rate)),
        None => ("No tax code".to_string(), Value::Null),
    };
    let mut lines: Vec<(String, Value)> = by_code
        .into_iter()
        .map(|(code, sums)| {
            let (name, rate) = named(&code);
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

    // Withholding, per code: held back by customers (AR) and from vendors (AP).
    let mut withheld: BTreeMap<String, (Money, Money)> = BTreeMap::new();
    let (mut by_customers, mut from_vendors) = (Money::ZERO, Money::ZERO);
    for r in sqlx::query(WITHHELD).bind(from).bind(to).fetch_all(&mut *conn).await? {
        let side: String = r.try_get("side")?;
        let code: Option<String> = r.try_get("code")?;
        let amount = to_base(Money::from_cents(r.try_get("amount")?), r.try_get("rate")?)?;
        let slot = withheld.entry(code.unwrap_or_default()).or_insert((Money::ZERO, Money::ZERO));
        if side == "AR" {
            slot.0 = add(slot.0, amount)?;
            by_customers = add(by_customers, amount)?;
        } else {
            slot.1 = add(slot.1, amount)?;
            from_vendors = add(from_vendors, amount)?;
        }
    }
    let withholding: Vec<Value> = withheld
        .into_iter()
        .map(|(code, (customers, vendors))| {
            let (name, rate) = named(&Some(code.clone()));
            json!({
                "taxCodeId": code,
                "name": name,
                "rate": rate,
                "withheldByCustomers": customers.to_json_number(),
                "withheldFromVendors": vendors.to_json_number(),
            })
        })
        .collect();

    let net = total.output_tax.checked_sub(total.input_tax).ok_or_else(overflow)?;
    Ok(json!({
        "dateFrom": date_from,
        "dateTo": date_to,
        "rows": lines.into_iter().map(|(_, v)| v).collect::<Vec<_>>(),
        "totals": total.json(),
        "netTax": net.to_json_number(),
        "withholding": withholding,
        "withholdingTotals": {
            "withheldByCustomers": by_customers.to_json_number(),
            "withheldFromVendors": from_vendors.to_json_number(),
        },
    }))
}
