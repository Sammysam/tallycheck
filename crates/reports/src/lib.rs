//! Financial reports (reference `reports.service.ts`).
//!
//! * Profit & Loss and Balance Sheet are computed **only** from ledger lines (every entry
//!   counts, D1), grouped by the account's type and subtype exactly as the reference groups
//!   them. Amounts are exact cents; the reference summed JavaScript floats (D4).
//! * AR / AP ageing list the open documents, as the reference does: POSTED invoices with an
//!   outstanding balance and POSTED bills, aged from their due date (or their date).

pub mod cash_flow;
pub mod tax_return;

use app_core::money::{as_json_number, Money};
use app_core::{AppError, AppResult};
use chrono::NaiveDate;
use ledger::entries::{active_accounts, normal_balance, sums_by_account};
use ledger::{Account, AccountType};
use serde::Serialize;
use serde_json::{json, Value};
use sqlx::{Row, SqliteConnection};

#[derive(Debug, Clone, Serialize)]
pub struct AccountLine {
    pub account: Account,
    #[serde(serialize_with = "as_json_number")]
    pub balance: Money,
}

fn sum(lines: &[AccountLine]) -> AppResult<Money> {
    lines
        .iter()
        .try_fold(Money::ZERO, |acc, l| acc.checked_add(l.balance))
        .ok_or_else(|| AppError::internal("report total overflow"))
}

fn sub(a: Money, b: Money) -> AppResult<Money> {
    a.checked_sub(b).ok_or_else(|| AppError::internal("report total overflow"))
}

async fn lines_for(
    conn: &mut SqliteConnection,
    date_from: Option<&str>,
    date_to: Option<&str>,
) -> AppResult<Vec<AccountLine>> {
    let sums = sums_by_account(conn, date_from, date_to).await?;
    let mut out = Vec::new();
    for account in active_accounts(conn).await? {
        let (d, c) = sums.get(&account.id).copied().unwrap_or((Money::ZERO, Money::ZERO));
        let balance = normal_balance(account.account_type, d, c)?;
        out.push(AccountLine { account, balance });
    }
    Ok(out)
}

#[derive(Debug, Clone)]
pub struct ProfitAndLoss {
    pub date_from: String,
    pub date_to: String,
    pub revenue: Vec<AccountLine>,
    pub cogs: Vec<AccountLine>,
    pub operating_expenses: Vec<AccountLine>,
    pub total_revenue: Money,
    pub total_cogs: Money,
    pub gross_profit: Money,
    pub total_operating_expenses: Money,
    pub net_income: Money,
}

impl ProfitAndLoss {
    pub fn to_json(&self) -> Value {
        json!({
            "dateFrom": self.date_from,
            "dateTo": self.date_to,
            "revenue": self.revenue,
            "totalRevenue": self.total_revenue.to_json_number(),
            "cogs": self.cogs,
            "totalCogs": self.total_cogs.to_json_number(),
            "grossProfit": self.gross_profit.to_json_number(),
            "operatingExpenses": self.operating_expenses,
            "totalOperatingExpenses": self.total_operating_expenses.to_json_number(),
            "netIncome": self.net_income.to_json_number(),
        })
    }
}

/// Revenue accounts; expenses with subtype `cogs`; every other expense is operating.
pub async fn profit_and_loss(conn: &mut SqliteConnection, date_from: &str, date_to: &str) -> AppResult<ProfitAndLoss> {
    let lines = lines_for(conn, Some(date_from), Some(date_to)).await?;
    let pick = |f: &dyn Fn(&Account) -> bool| lines.iter().filter(|l| f(&l.account)).cloned().collect::<Vec<_>>();
    let revenue = pick(&|a| a.account_type == AccountType::Revenue);
    let cogs = pick(&|a| a.account_type == AccountType::Expense && a.subtype.as_deref() == Some("cogs"));
    let operating_expenses = pick(&|a| a.account_type == AccountType::Expense && a.subtype.as_deref() != Some("cogs"));
    let total_revenue = sum(&revenue)?;
    let total_cogs = sum(&cogs)?;
    let gross_profit = sub(total_revenue, total_cogs)?;
    let total_operating_expenses = sum(&operating_expenses)?;
    let net_income = sub(gross_profit, total_operating_expenses)?;
    Ok(ProfitAndLoss {
        date_from: date_from.to_string(),
        date_to: date_to.to_string(),
        revenue,
        cogs,
        operating_expenses,
        total_revenue,
        total_cogs,
        gross_profit,
        total_operating_expenses,
        net_income,
    })
}

/// Reference `getBalanceSheet`: assets, liabilities and equity up to `as_of`, plus retained
/// earnings = net income from 1900-01-01 to `as_of`.
pub async fn balance_sheet(conn: &mut SqliteConnection, as_of: &str) -> AppResult<Value> {
    let lines = lines_for(conn, None, Some(as_of)).await?;
    let of = |t: AccountType| lines.iter().filter(|l| l.account.account_type == t).cloned().collect::<Vec<_>>();
    let (assets, liabilities, equity) = (of(AccountType::Asset), of(AccountType::Liability), of(AccountType::Equity));
    let (total_assets, total_liabilities, total_equity) = (sum(&assets)?, sum(&liabilities)?, sum(&equity)?);
    let retained = profit_and_loss(conn, "1900-01-01", as_of).await?.net_income;
    let total_le = total_liabilities
        .checked_add(total_equity)
        .and_then(|x| x.checked_add(retained))
        .ok_or_else(|| AppError::internal("report total overflow"))?;
    Ok(json!({
        "asOf": as_of,
        "assets": assets,
        "totalAssets": total_assets.to_json_number(),
        "liabilities": liabilities,
        "totalLiabilities": total_liabilities.to_json_number(),
        "equity": equity,
        "totalEquity": total_equity.to_json_number(),
        "retainedEarnings": retained.to_json_number(),
        "totalLiabilitiesAndEquity": total_le.to_json_number(),
        // The reference allowed a one-cent float tolerance; exact cents need none.
        "balanced": total_assets == total_le,
    }))
}

#[derive(Debug, Clone)]
pub struct AgingItem {
    pub id: String,
    pub number: String,
    pub name: String,
    pub date: String,
    pub due_date: Option<String>,
    pub amount: Money,
}

const BUCKETS: [&str; 5] = ["current", "days1_30", "days31_60", "days61_90", "days90plus"];

fn bucket_of(days_past: i64) -> usize {
    match days_past {
        d if d <= 0 => 0,
        d if d <= 30 => 1,
        d if d <= 60 => 2,
        d if d <= 90 => 3,
        _ => 4,
    }
}

fn parse_date(s: &str) -> AppResult<NaiveDate> {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").map_err(|e| AppError::internal(format!("stored date {s:?}: {e}")))
}

/// Buckets by days past the due date (or the document date when there is none).
pub fn build_aging(items: Vec<AgingItem>, today: NaiveDate) -> AppResult<Value> {
    let mut buckets: Vec<(Vec<Value>, Money)> = vec![(Vec::new(), Money::ZERO); BUCKETS.len()];
    let mut grand = Money::ZERO;
    for item in items {
        let due = parse_date(item.due_date.as_deref().unwrap_or(&item.date))?;
        let b = &mut buckets[bucket_of((today - due).num_days())];
        b.1 = b.1.checked_add(item.amount).ok_or_else(|| AppError::internal("aging overflow"))?;
        grand = grand.checked_add(item.amount).ok_or_else(|| AppError::internal("aging overflow"))?;
        b.0.push(json!({
            "id": item.id,
            "number": item.number,
            "name": item.name,
            "date": item.date,
            "dueDate": item.due_date,
            "amount": item.amount.to_json_number(),
        }));
    }
    let mut out = serde_json::Map::new();
    for (name, (items, total)) in BUCKETS.iter().zip(buckets) {
        out.insert(name.to_string(), json!({ "items": items, "total": total.to_json_number() }));
    }
    out.insert("grandTotal".into(), grand.to_json_number());
    Ok(Value::Object(out))
}

/// POSTED invoices with something still owed (part-paid invoices age only their balance).
pub async fn ar_aging(conn: &mut SqliteConnection, today: NaiveDate) -> AppResult<Value> {
    let rows = sqlx::query(
        r#"SELECT i."id", i."invoiceNumber", COALESCE(NULLIF(c."name", ''), i."billToName", '') AS "name",
                  i."date", i."dueDate", i."baseTotal" - i."baseAmountPaid" - i."baseAmountCredited" AS "outstanding"
           FROM "invoices" i LEFT JOIN "customers" c ON c."id" = i."customerId"
           WHERE i."status" = 'POSTED' AND i."baseTotal" - i."baseAmountPaid" - i."baseAmountCredited" > 0
           ORDER BY i.rowid"#,
    )
    .fetch_all(&mut *conn)
    .await?;
    let items = rows
        .iter()
        .map(|r| {
            Ok(AgingItem {
                id: r.try_get("id")?,
                number: r.try_get("invoiceNumber")?,
                name: r.try_get("name")?,
                date: r.try_get("date")?,
                due_date: r.try_get("dueDate")?,
                amount: Money::from_cents(r.try_get("outstanding")?),
            })
        })
        .collect::<AppResult<Vec<_>>>()?;
    build_aging(items, today)
}

/// POSTED bills, aged on what is still owed (D13: part-payments now reduce it).
pub async fn ap_aging(conn: &mut SqliteConnection, today: NaiveDate) -> AppResult<Value> {
    let rows = sqlx::query(
        r#"SELECT b."id", b."billNumber", COALESCE(v."name", '') AS "name", b."date", b."dueDate",
                  b."baseTotal" - b."baseAmountPaid" - b."baseAmountCredited" AS "outstanding"
           FROM "bills" b LEFT JOIN "vendors" v ON v."id" = b."vendorId"
           WHERE b."status" = 'POSTED'
           ORDER BY b.rowid"#,
    )
    .fetch_all(&mut *conn)
    .await?;
    let items = rows
        .iter()
        .map(|r| {
            Ok(AgingItem {
                id: r.try_get("id")?,
                number: r.try_get("billNumber")?,
                name: r.try_get("name")?,
                date: r.try_get("date")?,
                due_date: r.try_get("dueDate")?,
                amount: Money::from_cents(r.try_get("outstanding")?),
            })
        })
        .collect::<AppResult<Vec<_>>>()?;
    build_aging(items, today)
}

#[cfg(test)]
mod tests;

/// Tax code id → (name, rate), for reports.
pub(crate) async fn tax_codes_by_id(
    conn: &mut SqliteConnection,
) -> AppResult<std::collections::HashMap<String, (String, i64)>> {
    let rows = sqlx::query(r#"SELECT "id", "name", "rate" FROM "tax_codes""#).fetch_all(&mut *conn).await?;
    rows.iter().map(|r| Ok((r.try_get("id")?, (r.try_get("name")?, r.try_get("rate")?)))).collect()
}
