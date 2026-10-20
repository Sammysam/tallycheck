//! Tax codes and tax arithmetic (D26, D27; the reference had no tax).
//!
//! The company defines its own codes; nothing about any country's law is built in. A rate is
//! held in ten-thousandths of a percent (18% = 180000) and all arithmetic is integer, rounded
//! half-up (away from zero) per document line, like every other amount in the books (D4).
//!
//! Kinds (D27):
//! * STANDARD — one rate. Prices excluding tax: tax = amount × rate. Prices including tax:
//!   tax = amount × rate / (1 + rate). Output tax posts to the sales account (a liability),
//!   input tax to the purchase account.
//! * GROUP — several standard codes charged together (state + county + city). Each component's
//!   tax is rounded on its own and posts to its own accounts; a compound component is charged on
//!   the net plus the components before it. With prices including tax the net is found first
//!   from the combined rate, then each component is worked out on it, and the last component
//!   takes any one-cent rounding difference so the parts add up to the tax in the price.
//! * WITHHOLDING — held back from a payment (see `payments`), never charged on a line.
//!
//! Every taxed line stores its tax split by standard code (`document_taxes`), so changing a
//! rate or a group affects new documents only. A code in use cannot be deleted, only switched
//! off.

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
use std::collections::HashMap;

/// 100% in the units rates are stored in.
pub const RATE_SCALE: i64 = 10_000;
const HUNDRED_PERCENT: i128 = 100 * RATE_SCALE as i128;

pub const STANDARD: &str = "STANDARD";
pub const GROUP: &str = "GROUP";
pub const WITHHOLDING: &str = "WITHHOLDING";

#[derive(Debug, Clone)]
pub struct Component {
    pub tax_code_id: String,
    pub is_compound: bool,
}

#[derive(Debug, Clone)]
pub struct TaxCode {
    pub id: String,
    pub name: String,
    pub kind: String,
    /// Percent × 10 000 (18% = 180000). For a group: the combined rate, for display.
    pub rate: i64,
    pub description: Option<String>,
    pub sales_account_id: String,
    pub purchase_account_id: String,
    pub is_active: bool,
    pub components: Vec<Component>,
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
            "kind": self.kind,
            "rate": rate_json(self.rate),
            "description": opt(&self.description),
            "salesAccountId": self.sales_account_id,
            "purchaseAccountId": self.purchase_account_id,
            "isActive": self.is_active,
            "components": self.components.iter().map(|c| json!({
                "taxCodeId": c.tax_code_id,
                "isCompound": c.is_compound,
            })).collect::<Vec<_>>(),
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

fn to_money(cents: i128) -> AppResult<Money> {
    i64::try_from(cents).map(Money::from_cents).map_err(|_| overflow())
}

/// The tax in `amount` (cents) at one `rate`, rounded half-up.
pub fn tax_of(amount: Money, rate: i64, inclusive: bool) -> AppResult<Money> {
    let cents = i128::from(amount.cents());
    let rate = i128::from(rate);
    let tax = if inclusive {
        div_round_half_up(cents * rate, HUNDRED_PERCENT + rate)
    } else {
        div_round_half_up(cents * rate, HUNDRED_PERCENT)
    };
    to_money(tax)
}

/// One standard code's share of a line's tax.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaxPart {
    pub tax_code_id: String,
    pub amount: Money,
}

/// `(id, rate, compound)` of each part, in charging order.
type Rates = Vec<(String, i64, bool)>;

/// Splits the tax on a line `amount` into its parts (pure arithmetic, see the module notes).
pub fn split_parts(amount: Money, rates: &Rates, inclusive: bool) -> AppResult<Vec<TaxPart>> {
    let cents = i128::from(amount.cents());
    let exclusive_on = |net: i128| -> Vec<(String, i128)> {
        let mut before = 0i128;
        rates
            .iter()
            .map(|(id, rate, compound)| {
                let base = net + if *compound { before } else { 0 };
                let t = div_round_half_up(base * i128::from(*rate), HUNDRED_PERCENT);
                before += t;
                (id.clone(), t)
            })
            .collect()
    };
    let parts = if !inclusive {
        exclusive_on(cents)
    } else if rates.len() == 1 {
        vec![(rates[0].0.clone(), i128::from(tax_of(amount, rates[0].1, true)?.cents()))]
    } else {
        // The combined rate, in millionths of a percent-unit: exact for plain parts, and to a
        // twelfth decimal for compound ones (far below a cent).
        const S: i128 = 1_000_000_000_000;
        let mut combined = 0i128;
        for (_, rate, compound) in rates {
            let r = i128::from(*rate) * 1_000_000; // rate as a fraction × S
            let base = S + if *compound { combined } else { 0 };
            combined += base * r / S;
        }
        let net = div_round_half_up(cents * S, S + combined);
        let total = cents - net;
        let mut parts = exclusive_on(net);
        let sum: i128 = parts.iter().map(|(_, t)| *t).sum();
        if let Some(last) = parts.last_mut() {
            last.1 += total - sum;
        }
        parts
    };
    parts.into_iter().map(|(id, t)| Ok(TaxPart { tax_code_id: id, amount: to_money(t)? })).collect()
}

/// `(net, tax, gross)` of a line amount at one rate.
pub fn split(amount: Money, rate: i64, inclusive: bool) -> AppResult<(Money, Money, Money)> {
    let tax = tax_of(amount, rate, inclusive)?;
    if inclusive {
        Ok((amount.checked_sub(tax).ok_or_else(overflow)?, tax, amount))
    } else {
        Ok((amount, tax, amount.checked_add(tax).ok_or_else(overflow)?))
    }
}

/// The parts of the tax a line using `code_id` carries, and their total. The code must be
/// active and charged on lines (standard or group).
pub async fn line_tax(
    conn: &mut SqliteConnection,
    code_id: &str,
    amount: Money,
    inclusive: bool,
) -> AppResult<(Vec<TaxPart>, Money)> {
    let code = usable(conn, code_id).await?;
    let rates: Rates = match code.kind.as_str() {
        STANDARD => vec![(code.id.clone(), code.rate, false)],
        GROUP => {
            let mut rates = Vec::new();
            for c in &code.components {
                let part = usable(conn, &c.tax_code_id).await?;
                rates.push((part.id, part.rate, c.is_compound));
            }
            rates
        }
        _ => {
            return Err(AppError::bad_request(format!(
                "\"{}\" is a withholding tax — it is held back from payments, not charged on lines",
                code.name
            )))
        }
    };
    let parts = split_parts(amount, &rates, inclusive)?;
    let total = parts.iter().try_fold(Money::ZERO, |s, p| s.checked_add(p.amount)).ok_or_else(overflow)?;
    Ok((parts, total))
}

// ─── Stored splits ───────────────────────────────────────────────────────────

pub async fn save_parts(tx: &mut Tx, source_type: &str, source_id: &str, parts: &[TaxPart]) -> AppResult<()> {
    for p in parts {
        sqlx::query(
            r#"INSERT INTO "document_taxes" ("id","sourceType","sourceId","taxCodeId","taxAmount") VALUES (?1,?2,?3,?4,?5)"#,
        )
        .bind(new_id())
        .bind(source_type)
        .bind(source_id)
        .bind(&p.tax_code_id)
        .bind(p.amount.cents())
        .execute(&mut **tx)
        .await?;
    }
    Ok(())
}

/// The stored split of each of `source_ids`.
pub async fn load_parts(
    conn: &mut SqliteConnection,
    source_type: &str,
    source_ids: &[String],
) -> AppResult<HashMap<String, Vec<TaxPart>>> {
    let mut out: HashMap<String, Vec<TaxPart>> = HashMap::new();
    if source_ids.is_empty() {
        return Ok(out);
    }
    let placeholders = (2..2 + source_ids.len()).map(|i| format!("?{i}")).collect::<Vec<_>>().join(",");
    let sql = format!(
        r#"SELECT "sourceId","taxCodeId","taxAmount" FROM "document_taxes"
           WHERE "sourceType" = ?1 AND "sourceId" IN ({placeholders}) ORDER BY rowid"#
    );
    let mut q = sqlx::query(&sql).bind(source_type);
    for id in source_ids {
        q = q.bind(id);
    }
    for r in q.fetch_all(&mut *conn).await? {
        out.entry(r.try_get("sourceId")?)
            .or_default()
            .push(TaxPart { tax_code_id: r.try_get("taxCodeId")?, amount: Money::from_cents(r.try_get("taxAmount")?) });
    }
    Ok(out)
}

/// Removes the stored splits of the given lines (before they are replaced).
pub async fn delete_parts(tx: &mut Tx, source_type: &str, source_ids: &[String]) -> AppResult<()> {
    for id in source_ids {
        sqlx::query(r#"DELETE FROM "document_taxes" WHERE "sourceType" = ?1 AND "sourceId" = ?2"#)
            .bind(source_type)
            .bind(id)
            .execute(&mut **tx)
            .await?;
    }
    Ok(())
}

/// Adds each part to the tax account of its code — the sales account on sales, the purchase
/// account on purchases — for a journal entry.
pub async fn by_account(
    conn: &mut SqliteConnection,
    parts: impl IntoIterator<Item = TaxPart>,
    sales: bool,
) -> AppResult<Vec<(String, Money)>> {
    let mut out: Vec<(String, Money)> = Vec::new();
    for p in parts {
        if p.amount == Money::ZERO {
            continue;
        }
        let code = find_one(conn, &p.tax_code_id).await?;
        let account = if sales { code.sales_account_id } else { code.purchase_account_id };
        match out.iter_mut().find(|(a, _)| *a == account) {
            Some((_, t)) => *t = t.checked_add(p.amount).ok_or_else(overflow)?,
            None => out.push((account, p.amount)),
        }
    }
    Ok(out)
}

// ─── Tax codes ───────────────────────────────────────────────────────────────

const COLUMNS: &str = r#""id","name","kind","rate","description","salesAccountId","purchaseAccountId","isActive","createdAt","updatedAt""#;

fn from_row(r: &SqliteRow) -> AppResult<TaxCode> {
    Ok(TaxCode {
        id: r.try_get("id")?,
        name: r.try_get("name")?,
        kind: r.try_get("kind")?,
        rate: r.try_get("rate")?,
        description: r.try_get("description")?,
        sales_account_id: r.try_get("salesAccountId")?,
        purchase_account_id: r.try_get("purchaseAccountId")?,
        is_active: r.try_get::<i64, _>("isActive")? == 1,
        components: Vec::new(),
        created_at: r.try_get("createdAt")?,
        updated_at: r.try_get("updatedAt")?,
    })
}

async fn attach_components(conn: &mut SqliteConnection, code: &mut TaxCode) -> AppResult<()> {
    if code.kind != GROUP {
        return Ok(());
    }
    let rows = sqlx::query(
        r#"SELECT "componentId","isCompound" FROM "tax_code_components" WHERE "groupId" = ?1 ORDER BY "position""#,
    )
    .bind(&code.id)
    .fetch_all(&mut *conn)
    .await?;
    code.components = rows
        .iter()
        .map(|r| {
            Ok(Component {
                tax_code_id: r.try_get("componentId")?,
                is_compound: r.try_get::<i64, _>("isCompound")? == 1,
            })
        })
        .collect::<AppResult<_>>()?;
    Ok(())
}

pub async fn find_all(conn: &mut SqliteConnection) -> AppResult<Vec<TaxCode>> {
    let rows = sqlx::query(&format!(r#"SELECT {COLUMNS} FROM "tax_codes" ORDER BY "isActive" DESC, "name""#))
        .fetch_all(&mut *conn)
        .await?;
    let mut codes = rows.iter().map(from_row).collect::<AppResult<Vec<_>>>()?;
    for c in &mut codes {
        attach_components(conn, c).await?;
    }
    Ok(codes)
}

pub async fn find_one(conn: &mut SqliteConnection, id: &str) -> AppResult<TaxCode> {
    let row = sqlx::query(&format!(r#"SELECT {COLUMNS} FROM "tax_codes" WHERE "id" = ?1"#))
        .bind(id)
        .fetch_optional(&mut *conn)
        .await?;
    let mut code = match row {
        Some(r) => from_row(&r)?,
        None => return Err(AppError::NotFound(format!("Tax code {id} not found"))),
    };
    attach_components(conn, &mut code).await?;
    Ok(code)
}

/// The code a new document may use: it must exist and be active.
pub async fn usable(conn: &mut SqliteConnection, id: &str) -> AppResult<TaxCode> {
    let code = find_one(conn, id).await?;
    if !code.is_active {
        return Err(AppError::bad_request(format!("The tax code \"{}\" is no longer in use", code.name)));
    }
    Ok(code)
}

pub struct TaxCodeInput {
    pub name: Option<String>,
    pub kind: Option<String>,
    pub rate: Option<i64>,
    pub description: Option<Option<String>>,
    pub sales_account_id: Option<String>,
    pub purchase_account_id: Option<String>,
    pub is_active: Option<bool>,
    /// A group's parts in charging order: `(standard code id, compound)`.
    pub components: Option<Vec<(String, bool)>>,
}

async fn check_accounts(tx: &mut Tx, kind: &str, sales: &str, purchase: &str) -> AppResult<()> {
    let s = accounts::find_one(tx, sales).await?;
    let p = accounts::find_one(tx, purchase).await?;
    if kind == WITHHOLDING {
        if s.account_type != AccountType::Asset {
            return Err(AppError::bad_request(
                "Tax withheld by customers must go to an asset account (e.g. Withholding Tax Receivable)",
            ));
        }
        if p.account_type != AccountType::Liability {
            return Err(AppError::bad_request(
                "Tax withheld from vendors must go to a liability account (e.g. Withholding Tax Payable)",
            ));
        }
        return Ok(());
    }
    if s.account_type != AccountType::Liability {
        return Err(AppError::bad_request("The sales tax account must be a liability account (e.g. Tax Payable)"));
    }
    if !matches!(p.account_type, AccountType::Asset | AccountType::Liability) {
        return Err(AppError::bad_request(
            "The purchase tax account must be an asset (e.g. Tax Receivable) or the same liability account",
        ));
    }
    Ok(())
}

/// Checks a group's parts and returns its combined rate (for display) and the first part's
/// accounts (the group's own account columns are required but not used).
async fn check_components(tx: &mut Tx, parts: &[(String, bool)]) -> AppResult<(i64, String, String)> {
    if parts.len() < 2 {
        return Err(AppError::bad_request("A tax group needs at least two standard tax codes"));
    }
    let mut seen = std::collections::HashSet::new();
    let mut rates: Rates = Vec::new();
    let mut accounts = None;
    for (id, compound) in parts {
        if !seen.insert(id.clone()) {
            return Err(AppError::bad_request("A tax code can appear only once in a group"));
        }
        let code = find_one(tx, id).await?;
        if code.kind != STANDARD {
            return Err(AppError::bad_request(format!("\"{}\" is not a standard tax code", code.name)));
        }
        accounts.get_or_insert((code.sales_account_id.clone(), code.purchase_account_id.clone()));
        rates.push((code.id, code.rate, *compound));
    }
    // The combined rate: the tax on 100.00, as a percentage with four decimals.
    let on_hundred = split_parts(Money::from_cents(1_000_000), &rates, false)?;
    let combined = on_hundred.iter().map(|p| p.amount.cents()).sum::<i64>();
    let (sales, purchase) = accounts.unwrap_or_default();
    Ok((combined, sales, purchase))
}

async fn save_components(tx: &mut Tx, group_id: &str, parts: &[(String, bool)]) -> AppResult<()> {
    sqlx::query(r#"DELETE FROM "tax_code_components" WHERE "groupId" = ?1"#).bind(group_id).execute(&mut **tx).await?;
    for (i, (id, compound)) in parts.iter().enumerate() {
        sqlx::query(
            r#"INSERT INTO "tax_code_components" ("groupId","componentId","position","isCompound") VALUES (?1,?2,?3,?4)"#,
        )
        .bind(group_id)
        .bind(id)
        .bind(i64::try_from(i).unwrap_or(i64::MAX))
        .bind(*compound)
        .execute(&mut **tx)
        .await?;
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
    let kind = input.kind.unwrap_or_else(|| STANDARD.into());
    let mut tx = db.begin().await?;
    if name_taken(&mut tx, &name, None).await? {
        return Err(AppError::Conflict(format!("A tax code named \"{name}\" already exists")));
    }
    let (rate, sales, purchase, parts) = if kind == GROUP {
        let parts = input.components.unwrap_or_default();
        let (rate, sales, purchase) = check_components(&mut tx, &parts).await?;
        (rate, sales, purchase, parts)
    } else {
        let (Some(rate), Some(sales), Some(purchase)) = (input.rate, input.sales_account_id, input.purchase_account_id)
        else {
            return Err(AppError::bad_request("name, rate, salesAccountId and purchaseAccountId are required"));
        };
        check_accounts(&mut tx, &kind, &sales, &purchase).await?;
        (rate, sales, purchase, Vec::new())
    };
    let id = new_id();
    let now = now_iso();
    sqlx::query(
        r#"INSERT INTO "tax_codes" ("id","name","kind","rate","description","salesAccountId","purchaseAccountId","isActive","createdAt","updatedAt")
           VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?9)"#,
    )
    .bind(&id)
    .bind(&name)
    .bind(&kind)
    .bind(rate)
    .bind(input.description.flatten())
    .bind(&sales)
    .bind(&purchase)
    .bind(input.is_active.unwrap_or(true))
    .bind(&now)
    .execute(&mut *tx)
    .await?;
    save_components(&mut tx, &id, &parts).await?;
    let out = find_one(&mut tx, &id).await?;
    tx.commit().await?;
    Ok(out)
}

pub async fn update(db: &Db, id: &str, input: TaxCodeInput) -> AppResult<TaxCode> {
    let mut tx = db.begin().await?;
    let mut code = find_one(&mut tx, id).await?;
    if input.kind.as_deref().is_some_and(|k| k != code.kind) {
        return Err(AppError::bad_request("The kind of a tax code cannot change — create a new code instead"));
    }
    if let Some(name) = input.name {
        if name_taken(&mut tx, &name, Some(id)).await? {
            return Err(AppError::Conflict(format!("A tax code named \"{name}\" already exists")));
        }
        code.name = name;
    }
    if let Some(d) = input.description {
        code.description = d;
    }
    if let Some(a) = input.is_active {
        code.is_active = a;
    }
    if code.kind == GROUP {
        if let Some(parts) = input.components {
            if parts.iter().any(|(p, _)| p == id) {
                return Err(AppError::bad_request("A tax group cannot contain itself"));
            }
            let (rate, sales, purchase) = check_components(&mut tx, &parts).await?;
            (code.rate, code.sales_account_id, code.purchase_account_id) = (rate, sales, purchase);
            save_components(&mut tx, id, &parts).await?;
        }
    } else {
        if let Some(rate) = input.rate {
            code.rate = rate;
        }
        if let Some(s) = input.sales_account_id {
            code.sales_account_id = s;
        }
        if let Some(p) = input.purchase_account_id {
            code.purchase_account_id = p;
        }
        check_accounts(&mut tx, &code.kind, &code.sales_account_id, &code.purchase_account_id).await?;
    }
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

/// Deletes a code nothing has used; one in use can only be switched off.
pub async fn remove(db: &Db, id: &str) -> AppResult<()> {
    let mut tx = db.begin().await?;
    find_one(&mut tx, id).await?;
    let used: i64 = sqlx::query_scalar(
        r#"SELECT (SELECT COUNT(*) FROM "invoice_lines" WHERE "taxCodeId" = ?1)
                + (SELECT COUNT(*) FROM "bill_lines" WHERE "taxCodeId" = ?1)
                + (SELECT COUNT(*) FROM "expenses" WHERE "taxCodeId" = ?1)
                + (SELECT COUNT(*) FROM "credit_notes" WHERE "taxCodeId" = ?1)
                + (SELECT COUNT(*) FROM "document_taxes" WHERE "taxCodeId" = ?1)
                + (SELECT COUNT(*) FROM "payments" WHERE "withholdingTaxCodeId" = ?1)"#,
    )
    .bind(id)
    .fetch_one(&mut *tx)
    .await?;
    if used > 0 {
        return Err(AppError::bad_request("This tax code is used on documents — deactivate it instead of deleting it"));
    }
    let in_group: i64 = sqlx::query_scalar(r#"SELECT COUNT(*) FROM "tax_code_components" WHERE "componentId" = ?1"#)
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
    if in_group > 0 {
        return Err(AppError::bad_request("This tax code is part of a tax group — remove it from the group first"));
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

    fn part(id: &str, amount: &str) -> TaxPart {
        TaxPart { tax_code_id: id.into(), amount: m(amount) }
    }

    #[test]
    fn group_parts_are_rounded_on_their_own_and_compound_parts_include_earlier_tax() -> AppResult<()> {
        // State 6% + county 1% + city 1.25% on 10.00 (excluding tax).
        let plain: Rates =
            vec![("state".into(), 60_000, false), ("county".into(), 10_000, false), ("city".into(), 12_500, false)];
        assert_eq!(
            split_parts(m("10"), &plain, false)?,
            vec![part("state", "0.6"), part("county", "0.1"), part("city", "0.13")]
        );
        // 5% then a compound 10% on 100: 5.00, then 10% of 105 = 10.50.
        let compound: Rates = vec![("a".into(), 50_000, false), ("b".into(), 100_000, true)];
        assert_eq!(split_parts(m("100"), &compound, false)?, vec![part("a", "5"), part("b", "10.5")]);
        // Including tax: 115.50 holds exactly 5.00 + 10.50 on a net of 100.00.
        assert_eq!(split_parts(m("115.5"), &compound, true)?, vec![part("a", "5"), part("b", "10.5")]);
        // Including 8.25% in 10.00: net 9.24, parts 0.55 + 0.09 + 0.12 = 0.76 = 10.00 − 9.24.
        let inc = split_parts(m("10"), &plain, true)?;
        assert_eq!(inc.iter().map(|p| p.amount.cents()).sum::<i64>(), 76);
        Ok(())
    }
}
