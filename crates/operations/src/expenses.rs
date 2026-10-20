//! Expenses (reference `expenses.service.ts`): a one-line spend — debit the expense account,
//! credit the account it was paid from.

use crate::{money_num, money_str, opt, overflow, require_account};
use app_core::ids::new_id;
use app_core::money::Money;
use app_core::time::now_iso;
use app_core::{AppError, AppResult};
use database::{counters, Db, Tx};
use ledger::{entries, NewEntry, NewLine};
use serde_json::{json, Value};
use sqlx::sqlite::SqliteRow;
use sqlx::{Row, SqliteConnection};

#[derive(Debug, Clone)]
pub struct Expense {
    pub id: String,
    pub expense_number: String,
    pub date: String,
    pub description: String,
    pub amount: Money,
    pub expense_account_id: String,
    pub payment_account_id: String,
    pub receipt_url: Option<String>,
    pub status: String,
    pub journal_entry_id: Option<String>,
    /// D26: the amount is what was paid, tax included; this is the tax inside it.
    pub tax_code_id: Option<String>,
    pub tax_amount: Money,
    pub created_at: String,
    pub updated_at: String,
}

impl Expense {
    /// `created`: the insert echo, where `amount` is the number the client sent.
    pub fn to_json(&self, created: bool) -> Value {
        json!({
            "id": self.id,
            "expenseNumber": self.expense_number,
            "date": self.date,
            "description": self.description,
            "amount": if created { money_num(self.amount) } else { money_str(self.amount) },
            "expenseAccountId": self.expense_account_id,
            "paymentAccountId": self.payment_account_id,
            "receiptUrl": opt(&self.receipt_url),
            "status": self.status,
            "journalEntryId": opt(&self.journal_entry_id),
            "taxCodeId": opt(&self.tax_code_id),
            "taxAmount": money_str(self.tax_amount),
            "createdAt": self.created_at,
            "updatedAt": self.updated_at,
        })
    }
}

const COLUMNS: &str = r#""id","expenseNumber","date","description","amount","expenseAccountId","paymentAccountId","receiptUrl","status","journalEntryId","taxCodeId","taxAmount","createdAt","updatedAt""#;

fn from_row(r: &SqliteRow) -> AppResult<Expense> {
    Ok(Expense {
        id: r.try_get("id")?,
        expense_number: r.try_get("expenseNumber")?,
        date: r.try_get("date")?,
        description: r.try_get("description")?,
        amount: Money::from_cents(r.try_get("amount")?),
        expense_account_id: r.try_get("expenseAccountId")?,
        payment_account_id: r.try_get("paymentAccountId")?,
        receipt_url: r.try_get("receiptUrl")?,
        status: r.try_get("status")?,
        journal_entry_id: r.try_get("journalEntryId")?,
        tax_code_id: r.try_get("taxCodeId")?,
        tax_amount: Money::from_cents(r.try_get("taxAmount")?),
        created_at: r.try_get("createdAt")?,
        updated_at: r.try_get("updatedAt")?,
    })
}

pub async fn find_one(conn: &mut SqliteConnection, id: &str) -> AppResult<Expense> {
    let row = sqlx::query(&format!(r#"SELECT {COLUMNS} FROM "expenses" WHERE "id" = ?1"#))
        .bind(id)
        .fetch_optional(conn)
        .await?;
    match row {
        Some(r) => from_row(&r),
        None => Err(AppError::NotFound(format!("Expense {id} not found"))),
    }
}

pub async fn find_all(db: &Db, page: i64, limit: i64) -> AppResult<(Vec<Expense>, i64)> {
    let total: i64 = sqlx::query_scalar(r#"SELECT COUNT(*) FROM "expenses""#).fetch_one(db.read()).await?;
    let rows =
        sqlx::query(&format!(r#"SELECT {COLUMNS} FROM "expenses" ORDER BY "date" DESC, rowid ASC LIMIT ?1 OFFSET ?2"#))
            .bind(limit)
            .bind((page - 1).saturating_mul(limit))
            .fetch_all(db.read())
            .await?;
    Ok((rows.iter().map(from_row).collect::<AppResult<Vec<_>>>()?, total))
}

pub struct CreateExpense {
    pub date: String,
    pub description: String,
    pub amount: Money,
    pub expense_account_id: String,
    pub payment_account_id: String,
    pub receipt_url: Option<String>,
    /// D26: optional tax code; the amount includes the tax.
    pub tax_code_id: Option<String>,
}

pub async fn create(db: &Db, dto: CreateExpense) -> AppResult<Expense> {
    let mut tx = db.begin().await?;
    require_account(&mut tx, &dto.expense_account_id).await?;
    require_account(&mut tx, &dto.payment_account_id).await?;
    let (parts, tax) = match &dto.tax_code_id {
        Some(code) => crate::tax::line_tax(&mut tx, code, dto.amount, true).await?,
        None => (Vec::new(), Money::ZERO),
    };
    let number = counters::next_formatted(&mut tx, "EXP", 6).await?;
    let id = new_id();
    let now = now_iso();
    sqlx::query(
        r#"INSERT INTO "expenses" ("id","expenseNumber","date","description","amount","expenseAccountId","paymentAccountId",
             "receiptUrl","status","createdAt","updatedAt","taxCodeId","taxAmount")
           VALUES (?1,?2,?3,?4,?5,?6,?7,?8,'DRAFT',?9,?9,?10,?11)"#,
    )
    .bind(&id)
    .bind(&number)
    .bind(&dto.date)
    .bind(&dto.description)
    .bind(dto.amount.cents())
    .bind(&dto.expense_account_id)
    .bind(&dto.payment_account_id)
    .bind(&dto.receipt_url)
    .bind(&now)
    .bind(&dto.tax_code_id)
    .bind(tax.cents())
    .execute(&mut *tx)
    .await?;
    crate::tax::save_parts(&mut tx, "EXPENSE", &id, &parts).await?;
    let e = find_one(&mut tx, &id).await?;
    tx.commit().await?;
    Ok(e)
}

async fn set(tx: &mut Tx, id: &str, status: &str, journal_entry_id: Option<&str>) -> AppResult<()> {
    sqlx::query(r#"UPDATE "expenses" SET "status"=?1, "journalEntryId"=COALESCE(?2,"journalEntryId"), "updatedAt"=?3 WHERE "id"=?4"#)
        .bind(status)
        .bind(journal_entry_id)
        .bind(now_iso())
        .bind(id)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

pub async fn post(db: &Db, id: &str) -> AppResult<Expense> {
    let mut tx = db.begin().await?;
    let e = find_one(&mut tx, id).await?;
    if e.status != "DRAFT" {
        return Err(AppError::bad_request(format!("Expense is already {}", e.status)));
    }
    // The expense account gets the amount without its tax; the tax goes to the code's
    // purchase account (D26).
    let net = e.amount.checked_sub(e.tax_amount).ok_or_else(overflow)?;
    let mut lines =
        vec![NewLine { account_id: e.expense_account_id.clone(), debit: net, credit: Money::ZERO, description: None }];
    // D27: each part of the tax to its own code's account.
    let parts = crate::tax::load_parts(&mut tx, "EXPENSE", std::slice::from_ref(&e.id)).await?.remove(&e.id);
    for (account_id, tax) in crate::tax::by_account(&mut tx, parts.unwrap_or_default(), false).await? {
        lines.push(NewLine { account_id, debit: tax, credit: Money::ZERO, description: Some("Tax".into()) });
    }
    lines.push(NewLine {
        account_id: e.payment_account_id.clone(),
        debit: Money::ZERO,
        credit: e.amount,
        description: None,
    });
    let entry = entries::post(
        &mut tx,
        NewEntry {
            date: e.date.clone(),
            description: format!("Expense {}: {}", e.expense_number, e.description),
            reference: Some(e.expense_number.clone()),
            source_type: Some("EXPENSE".into()),
            source_id: Some(e.id.clone()),
            lines,
        },
    )
    .await?;
    set(&mut tx, id, "POSTED", Some(&entry.id)).await?;
    let out = find_one(&mut tx, id).await?;
    tx.commit().await?;
    Ok(out)
}

pub async fn void(db: &Db, id: &str, today: &str) -> AppResult<Expense> {
    let mut tx = db.begin().await?;
    let e = find_one(&mut tx, id).await?;
    if e.status == "VOID" {
        return Err(AppError::bad_request("Expense is already voided"));
    }
    if let Some(je) = &e.journal_entry_id {
        entries::reverse(&mut tx, je, today).await?;
    }
    set(&mut tx, id, "VOID", None).await?;
    let out = find_one(&mut tx, id).await?;
    tx.commit().await?;
    Ok(out)
}
