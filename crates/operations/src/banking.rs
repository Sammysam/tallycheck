//! Bank accounts (reference `banking.service.ts`). `currentBalance` is a stored column the
//! reference never updates from postings; it is kept as-is (balances come from the ledger).

use crate::require_account;
use app_core::ids::new_id;
use app_core::money::{as_db_string, Money};
use app_core::time::now_iso;
use app_core::{AppError, AppResult};
use database::Db;
use serde::Serialize;
use sqlx::sqlite::SqliteRow;
use sqlx::{Row, SqliteConnection};

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BankAccount {
    pub id: String,
    pub name: String,
    pub bank_name: Option<String>,
    pub account_number: Option<String>,
    pub account_id: String,
    #[serde(serialize_with = "as_db_string")]
    pub current_balance: Money,
    pub is_active: bool,
    pub created_at: String,
    pub updated_at: String,
}

const COLUMNS: &str =
    r#""id","name","bankName","accountNumber","accountId","currentBalance","isActive","createdAt","updatedAt""#;

fn from_row(r: &SqliteRow) -> AppResult<BankAccount> {
    Ok(BankAccount {
        id: r.try_get("id")?,
        name: r.try_get("name")?,
        bank_name: r.try_get("bankName")?,
        account_number: r.try_get("accountNumber")?,
        account_id: r.try_get("accountId")?,
        current_balance: Money::from_cents(r.try_get("currentBalance")?),
        is_active: r.try_get::<i64, _>("isActive")? != 0,
        created_at: r.try_get("createdAt")?,
        updated_at: r.try_get("updatedAt")?,
    })
}

pub async fn find_one(conn: &mut SqliteConnection, id: &str) -> AppResult<BankAccount> {
    let row = sqlx::query(&format!(r#"SELECT {COLUMNS} FROM "bank_accounts" WHERE "id" = ?1"#))
        .bind(id)
        .fetch_optional(conn)
        .await?;
    match row {
        Some(r) => from_row(&r),
        None => Err(AppError::NotFound(format!("Bank account {id} not found"))),
    }
}

pub async fn find_all(db: &Db) -> AppResult<Vec<BankAccount>> {
    let rows = sqlx::query(&format!(
        r#"SELECT {COLUMNS} FROM "bank_accounts" WHERE "isActive" = 1 ORDER BY "name" ASC, rowid ASC"#
    ))
    .fetch_all(db.read())
    .await?;
    rows.iter().map(from_row).collect()
}

#[derive(Default)]
pub struct BankFields {
    pub name: Option<String>,
    pub bank_name: Option<Option<String>>,
    pub account_number: Option<Option<String>>,
    pub account_id: Option<String>,
}

pub async fn create(db: &Db, f: BankFields) -> AppResult<BankAccount> {
    let mut tx = db.begin().await?;
    let account_id = f.account_id.unwrap_or_default();
    require_account(&mut tx, &account_id).await?;
    let id = new_id();
    let now = now_iso();
    sqlx::query(
        r#"INSERT INTO "bank_accounts" ("id","name","bankName","accountNumber","accountId","currentBalance","isActive","createdAt","updatedAt")
           VALUES (?1,?2,?3,?4,?5,0,1,?6,?6)"#,
    )
    .bind(&id)
    .bind(f.name.unwrap_or_default())
    .bind(f.bank_name.flatten())
    .bind(f.account_number.flatten())
    .bind(&account_id)
    .bind(&now)
    .execute(&mut *tx)
    .await?;
    let b = find_one(&mut tx, &id).await?;
    tx.commit().await?;
    Ok(b)
}

pub async fn update(db: &Db, id: &str, f: BankFields) -> AppResult<BankAccount> {
    let mut tx = db.begin().await?;
    let current = find_one(&mut tx, id).await?;
    let mut next = current.clone();
    if let Some(v) = f.name {
        next.name = v;
    }
    if let Some(v) = f.bank_name {
        next.bank_name = v;
    }
    if let Some(v) = f.account_number {
        next.account_number = v;
    }
    if let Some(v) = f.account_id {
        require_account(&mut tx, &v).await?;
        next.account_id = v;
    }
    if next != current {
        sqlx::query(
            r#"UPDATE "bank_accounts" SET "name"=?1,"bankName"=?2,"accountNumber"=?3,"accountId"=?4,"updatedAt"=?5 WHERE "id"=?6"#,
        )
        .bind(&next.name)
        .bind(&next.bank_name)
        .bind(&next.account_number)
        .bind(&next.account_id)
        .bind(now_iso())
        .bind(id)
        .execute(&mut *tx)
        .await?;
    }
    let b = find_one(&mut tx, id).await?;
    tx.commit().await?;
    Ok(b)
}

/// `DELETE /banking/:id` deactivates (the row is kept).
pub async fn deactivate(db: &Db, id: &str) -> AppResult<()> {
    let mut tx = db.begin().await?;
    let b = find_one(&mut tx, id).await?;
    if b.is_active {
        sqlx::query(r#"UPDATE "bank_accounts" SET "isActive" = 0, "updatedAt" = ?1 WHERE "id" = ?2"#)
            .bind(now_iso())
            .bind(id)
            .execute(&mut *tx)
            .await?;
    }
    tx.commit().await?;
    Ok(())
}
