//! Tax codes and tax arithmetic (D26, requested 2026-10-20; the reference had no tax).
//!
//! The company defines its own codes ("VAT 18%", "GST 10%", "Sales tax 8.875%", "Zero-rated
//! 0%"); nothing about any country's law is built in. A rate is held in ten-thousandths of a
//! percent (18% = 180000) and all arithmetic is integer, rounded half-up (away from zero) per
//! document line, like every other amount in the books (D4).
//!
//! * Prices excluding tax: tax = amount × rate.
//! * Prices including tax: tax = amount × rate / (1 + rate); the net is the amount less the tax.
//!
//! Output tax on sales posts to the code's sales account (a liability); input tax on purchases
//! to its purchase account. A code in use cannot be deleted, only deactivated; changing a rate
//! affects new documents only, because every line stores its own tax.

use crate::{opt, overflow};
use app_core::ids::new_id;
use app_core::money::Money;
use app_core::time::now_iso;
use app_core::{AppError, AppResult};
use database::{Db, Tx};
use ledger::accounts::{self, AccountType};
use serde_json::{json, Value};
use sqlx::sqlite::SqliteRow;
use sqlx::{Row, SqliteConnection};

/// 100% in the units rates are stored in.
pub const RATE_SCALE: i64 = 10_000;
const HUNDRED_PERCENT: i128 = 100 * RATE_SCALE as i128;

#[derive(Debug, Clone)]
pub struct TaxCode {
    pub id: String,
    pub name: String,
    /// Percent × 10 000 (18% = 180000).
    pub rate: i64,
    pub description: Option<String>,
    pub sales_account_id: String,
    pub purchase_account_id: String,
    pub is_active: bool,
    pub created_at: String,
    pub updated_at: String,
}

/// The percentage as a JSON number: 18, 8.875, 0.
pub fn rate_json(rate: i64) -> Value {
    let text = format!("{}.{:04}", rate / RATE_SCALE, rate % RATE_SCALE);
    let trimmed = text.trim_end_matches('0').trim_end_matches('.');
    serde_json::from_str(trimmed).unwrap_or(json!(0))
}

impl TaxCode {
    pub fn to_json(&self) -> Value {
        json!({
            "id": self.id,
            "name": self.name,
            "rate": rate_json(self.rate),
            "description": opt(&self.description),
            "salesAccountId": self.sales_account_id,
            "purchaseAccountId": self.purchase_account_id,
            "isActive": self.is_active,
            "createdAt": self.created_at,
            "updatedAt": self.updated_at,
        })
    }
}

fn div_round_half_up(n: i128, d: i128) -> i128 {
    let (q, r) = (n / d, n % d);
    if 2 * r.abs() >= d.abs() {
        q + n.signum() * d.signum()
    } else {
        q
    }
}

/// The tax in `amount` (cents) at `rate`, rounded half-up.
pub fn tax_of(amount: Money, rate: i64, inclusive: bool) -> AppResult<Money> {
    let cents = i128::from(amount.cents());
    let rate = i128::from(rate);
    let tax = if inclusive {
        div_round_half_up(cents * rate, HUNDRED_PERCENT + rate)
    } else {
        div_round_half_up(cents * rate, HUNDRED_PERCENT)
    };
    i64::try_from(tax).map(Money::from_cents).map_err(|_| overflow())
}

/// `(net, tax, gross)` of a line amount.
pub fn split(amount: Money, rate: i64, inclusive: bool) -> AppResult<(Money, Money, Money)> {
    let tax = tax_of(amount, rate, inclusive)?;
    if inclusive {
        Ok((amount.checked_sub(tax).ok_or_else(overflow)?, tax, amount))
    } else {
        Ok((amount, tax, amount.checked_add(tax).ok_or_else(overflow)?))
    }
}

const COLUMNS: &str =
    r#""id","name","rate","description","salesAccountId","purchaseAccountId","isActive","createdAt","updatedAt""#;

fn from_row(r: &SqliteRow) -> AppResult<TaxCode> {
    Ok(TaxCode {
        id: r.try_get("id")?,
        name: r.try_get("name")?,
        rate: r.try_get("rate")?,
        description: r.try_get("description")?,
        sales_account_id: r.try_get("salesAccountId")?,
        purchase_account_id: r.try_get("purchaseAccountId")?,
        is_active: r.try_get::<i64, _>("isActive")? == 1,
        created_at: r.try_get("createdAt")?,
        updated_at: r.try_get("updatedAt")?,
    })
}

pub async fn find_all(conn: &mut SqliteConnection) -> AppResult<Vec<TaxCode>> {
    let rows = sqlx::query(&format!(r#"SELECT {COLUMNS} FROM "tax_codes" ORDER BY "isActive" DESC, "name""#))
        .fetch_all(&mut *conn)
        .await?;
    rows.iter().map(from_row).collect()
}

pub async fn find_one(conn: &mut SqliteConnection, id: &str) -> AppResult<TaxCode> {
    let row = sqlx::query(&format!(r#"SELECT {COLUMNS} FROM "tax_codes" WHERE "id" = ?1"#))
        .bind(id)
        .fetch_optional(&mut *conn)
        .await?;
    match row {
        Some(r) => from_row(&r),
        None => Err(AppError::NotFound(format!("Tax code {id} not found"))),
    }
}

/// The code a new document line may use: it must exist and be active.
pub async fn usable(conn: &mut SqliteConnection, id: &str) -> AppResult<TaxCode> {
    let code = find_one(conn, id).await?;
    if !code.is_active {
        return Err(AppError::bad_request(format!("The tax code \"{}\" is no longer in use", code.name)));
    }
    Ok(code)
}

pub struct TaxCodeInput {
    pub name: Option<String>,
    pub rate: Option<i64>,
    pub description: Option<Option<String>>,
    pub sales_account_id: Option<String>,
    pub purchase_account_id: Option<String>,
    pub is_active: Option<bool>,
}

async fn check_accounts(tx: &mut Tx, sales: &str, purchase: &str) -> AppResult<()> {
    let s = accounts::find_one(tx, sales).await?;
    if s.account_type != AccountType::Liability {
        return Err(AppError::bad_request("The sales tax account must be a liability account (e.g. Tax Payable)"));
    }
    let p = accounts::find_one(tx, purchase).await?;
    if !matches!(p.account_type, AccountType::Asset | AccountType::Liability) {
        return Err(AppError::bad_request(
            "The purchase tax account must be an asset (e.g. Tax Receivable) or the same liability account",
        ));
    }
    Ok(())
}

async fn name_taken(tx: &mut Tx, name: &str, except: Option<&str>) -> AppResult<bool> {
    let n: i64 =
        sqlx::query_scalar(r#"SELECT COUNT(*) FROM "tax_codes" WHERE lower("name") = lower(?1) AND "id" IS NOT ?2"#)
            .bind(name)
            .bind(except)
            .fetch_one(&mut **tx)
            .await?;
    Ok(n > 0)
}

pub async fn create(db: &Db, input: TaxCodeInput) -> AppResult<TaxCode> {
    let name = input.name.unwrap_or_default();
    let (Some(rate), Some(sales), Some(purchase)) = (input.rate, input.sales_account_id, input.purchase_account_id)
    else {
        return Err(AppError::bad_request("name, rate, salesAccountId and purchaseAccountId are required"));
    };
    let mut tx = db.begin().await?;
    if name_taken(&mut tx, &name, None).await? {
        return Err(AppError::Conflict(format!("A tax code named \"{name}\" already exists")));
    }
    check_accounts(&mut tx, &sales, &purchase).await?;
    let id = new_id();
    let now = now_iso();
    sqlx::query(
        r#"INSERT INTO "tax_codes" ("id","name","rate","description","salesAccountId","purchaseAccountId","isActive","createdAt","updatedAt")
           VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?8)"#,
    )
    .bind(&id)
    .bind(&name)
    .bind(rate)
    .bind(input.description.flatten())
    .bind(&sales)
    .bind(&purchase)
    .bind(input.is_active.unwrap_or(true))
    .bind(&now)
    .execute(&mut *tx)
    .await?;
    let out = find_one(&mut tx, &id).await?;
    tx.commit().await?;
    Ok(out)
}

pub async fn update(db: &Db, id: &str, input: TaxCodeInput) -> AppResult<TaxCode> {
    let mut tx = db.begin().await?;
    let mut code = find_one(&mut tx, id).await?;
    if let Some(name) = input.name {
        if name_taken(&mut tx, &name, Some(id)).await? {
            return Err(AppError::Conflict(format!("A tax code named \"{name}\" already exists")));
        }
        code.name = name;
    }
    if let Some(rate) = input.rate {
        code.rate = rate;
    }
    if let Some(d) = input.description {
        code.description = d;
    }
    if let Some(s) = input.sales_account_id {
        code.sales_account_id = s;
    }
    if let Some(p) = input.purchase_account_id {
        code.purchase_account_id = p;
    }
    if let Some(a) = input.is_active {
        code.is_active = a;
    }
    check_accounts(&mut tx, &code.sales_account_id, &code.purchase_account_id).await?;
    sqlx::query(
        r#"UPDATE "tax_codes" SET "name"=?1,"rate"=?2,"description"=?3,"salesAccountId"=?4,"purchaseAccountId"=?5,
             "isActive"=?6,"updatedAt"=?7 WHERE "id"=?8"#,
    )
    .bind(&code.name)
    .bind(code.rate)
    .bind(&code.description)
    .bind(&code.sales_account_id)
    .bind(&code.purchase_account_id)
    .bind(code.is_active)
    .bind(now_iso())
    .bind(id)
    .execute(&mut *tx)
    .await?;
    let out = find_one(&mut tx, id).await?;
    tx.commit().await?;
    Ok(out)
}

/// Deletes a code no document has used; one in use can only be deactivated.
pub async fn remove(db: &Db, id: &str) -> AppResult<()> {
    let mut tx = db.begin().await?;
    find_one(&mut tx, id).await?;
    let used: i64 = sqlx::query_scalar(
        r#"SELECT (SELECT COUNT(*) FROM "invoice_lines" WHERE "taxCodeId" = ?1)
                + (SELECT COUNT(*) FROM "bill_lines" WHERE "taxCodeId" = ?1)
                + (SELECT COUNT(*) FROM "expenses" WHERE "taxCodeId" = ?1)
                + (SELECT COUNT(*) FROM "credit_notes" WHERE "taxCodeId" = ?1)"#,
    )
    .bind(id)
    .fetch_one(&mut *tx)
    .await?;
    if used > 0 {
        return Err(AppError::bad_request("This tax code is used on documents — deactivate it instead of deleting it"));
    }
    sqlx::query(r#"DELETE FROM "tax_codes" WHERE "id" = ?1"#).bind(id).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(s: &str) -> Money {
        Money::parse_decimal(s).unwrap_or_default()
    }

    #[test]
    fn exclusive_and_inclusive_tax_round_half_up_per_line() -> AppResult<()> {
        assert_eq!(split(m("100"), 180_000, false)?, (m("100"), m("18"), m("118")));
        assert_eq!(split(m("118"), 180_000, true)?, (m("100"), m("18"), m("118")));
        // 8.875% of 10.00 = 0.8875 → 0.89; 10% of 0.05 = 0.005 → 0.01 (half-up).
        assert_eq!(tax_of(m("10"), 88_750, false)?, m("0.89"));
        assert_eq!(tax_of(m("0.05"), 100_000, false)?, m("0.01"));
        assert_eq!(tax_of(m("0.04"), 100_000, false)?, Money::ZERO);
        // Inclusive 15% in 100.00: 100 × 15/115 = 13.043… → 13.04, net 86.96.
        assert_eq!(split(m("100"), 150_000, true)?, (m("86.96"), m("13.04"), m("100")));
        assert_eq!(split(m("100"), 0, false)?, (m("100"), Money::ZERO, m("100")));
        Ok(())
    }

    #[test]
    fn rates_print_as_plain_percentages() {
        assert_eq!(rate_json(180_000), json!(18));
        assert_eq!(rate_json(88_750), json!(8.875));
        assert_eq!(rate_json(0), json!(0));
        assert_eq!(rate_json(5), json!(0.0005));
    }
}
