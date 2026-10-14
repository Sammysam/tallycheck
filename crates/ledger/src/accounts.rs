//! Chart of accounts (reference `accounts.service.ts`).

use app_core::ids::new_id;
use app_core::time::now_iso;
use app_core::{AppError, AppResult};
use database::{Db, Tx};
use serde::Serialize;
use sqlx::sqlite::SqliteRow;
use sqlx::{Row, SqliteConnection};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum AccountType {
    Asset,
    Liability,
    Equity,
    Revenue,
    Expense,
}

impl AccountType {
    pub const ALL: [&'static str; 5] = ["ASSET", "LIABILITY", "EQUITY", "REVENUE", "EXPENSE"];

    pub fn parse(s: &str) -> Option<AccountType> {
        match s {
            "ASSET" => Some(AccountType::Asset),
            "LIABILITY" => Some(AccountType::Liability),
            "EQUITY" => Some(AccountType::Equity),
            "REVENUE" => Some(AccountType::Revenue),
            "EXPENSE" => Some(AccountType::Expense),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            AccountType::Asset => "ASSET",
            AccountType::Liability => "LIABILITY",
            AccountType::Equity => "EQUITY",
            AccountType::Revenue => "REVENUE",
            AccountType::Expense => "EXPENSE",
        }
    }

    /// Assets and expenses carry debit balances; the rest carry credit balances.
    pub fn is_debit_normal(self) -> bool {
        matches!(self, AccountType::Asset | AccountType::Expense)
    }
}

/// An account as the reference serialises its `Account` entity.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub id: String,
    pub code: String,
    pub name: String,
    #[serde(rename = "type")]
    pub account_type: AccountType,
    pub subtype: Option<String>,
    pub description: Option<String>,
    pub is_active: bool,
    pub parent_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

pub(crate) const COLUMNS: &str =
    r#""id","code","name","type","subtype","description","isActive","parentId","createdAt","updatedAt""#;

pub(crate) fn from_row(row: &SqliteRow) -> AppResult<Account> {
    let t: String = row.try_get("type")?;
    Ok(Account {
        id: row.try_get("id")?,
        code: row.try_get("code")?,
        name: row.try_get("name")?,
        account_type: AccountType::parse(&t).ok_or_else(|| AppError::internal(format!("bad account type {t}")))?,
        subtype: row.try_get("subtype")?,
        description: row.try_get("description")?,
        is_active: row.try_get::<i64, _>("isActive")? != 0,
        parent_id: row.try_get("parentId")?,
        created_at: row.try_get("createdAt")?,
        updated_at: row.try_get("updatedAt")?,
    })
}

/// Active accounts ordered by code (reference `findAll`).
pub async fn find_all(db: &Db) -> AppResult<Vec<Account>> {
    let rows = sqlx::query(&format!(r#"SELECT {COLUMNS} FROM "accounts" WHERE "isActive" = 1 ORDER BY "code" ASC"#))
        .fetch_all(db.read())
        .await?;
    rows.iter().map(from_row).collect()
}

pub async fn get(conn: &mut SqliteConnection, id: &str) -> AppResult<Option<Account>> {
    let row = sqlx::query(&format!(r#"SELECT {COLUMNS} FROM "accounts" WHERE "id" = ?1"#))
        .bind(id)
        .fetch_optional(conn)
        .await?;
    row.as_ref().map(from_row).transpose()
}

/// Reference `findOne`: 404 `Account <id> not found`.
pub async fn find_one(conn: &mut SqliteConnection, id: &str) -> AppResult<Account> {
    get(conn, id).await?.ok_or_else(|| AppError::NotFound(format!("Account {id} not found")))
}

/// Reference `findByCode`: 404 `Account code <code> not found`. Used by postings that need a
/// fixed account (1100 AR, 2000 AP, payroll payables, …).
pub async fn find_by_code(conn: &mut SqliteConnection, code: &str) -> AppResult<Account> {
    let row = sqlx::query(&format!(r#"SELECT {COLUMNS} FROM "accounts" WHERE "code" = ?1"#))
        .bind(code)
        .fetch_optional(conn)
        .await?;
    match row {
        Some(r) => from_row(&r),
        None => Err(AppError::NotFound(format!("Account code {code} not found"))),
    }
}

async fn code_taken(conn: &mut SqliteConnection, code: &str) -> AppResult<bool> {
    let n: i64 =
        sqlx::query_scalar(r#"SELECT COUNT(*) FROM "accounts" WHERE "code" = ?1"#).bind(code).fetch_one(conn).await?;
    Ok(n > 0)
}

pub struct CreateAccount {
    pub code: String,
    pub name: String,
    pub account_type: AccountType,
    pub subtype: Option<String>,
    pub description: Option<String>,
    pub parent_id: Option<String>,
}

pub async fn create(db: &Db, dto: CreateAccount) -> AppResult<Account> {
    let mut tx = db.begin().await?;
    if code_taken(&mut tx, &dto.code).await? {
        return Err(AppError::Conflict(format!("Account code {} already exists", dto.code)));
    }
    if let Some(parent) = &dto.parent_id {
        find_one(&mut tx, parent).await?;
    }
    let id = new_id();
    let now = now_iso();
    sqlx::query(
        r#"INSERT INTO "accounts" ("id","code","name","type","subtype","description","isActive","parentId","createdAt","updatedAt")
           VALUES (?1,?2,?3,?4,?5,?6,1,?7,?8,?8)"#,
    )
    .bind(&id)
    .bind(&dto.code)
    .bind(&dto.name)
    .bind(dto.account_type.as_str())
    .bind(&dto.subtype)
    .bind(&dto.description)
    .bind(&dto.parent_id)
    .bind(&now)
    .execute(&mut *tx)
    .await?;
    let account = find_one(&mut tx, &id).await?;
    tx.commit().await?;
    Ok(account)
}

/// A PATCH: `None` leaves a field alone; `Some(None)` clears a nullable field.
#[derive(Default)]
pub struct UpdateAccount {
    pub code: Option<String>,
    pub name: Option<String>,
    pub account_type: Option<AccountType>,
    pub subtype: Option<Option<String>>,
    pub description: Option<Option<String>>,
    pub parent_id: Option<Option<String>>,
}

pub async fn update(db: &Db, id: &str, dto: UpdateAccount) -> AppResult<Account> {
    let mut tx = db.begin().await?;
    let current = find_one(&mut tx, id).await?;
    let mut next = current.clone();
    if let Some(code) = dto.code {
        // The reference had no check here and failed with a database error (500).
        if code != current.code && code_taken(&mut tx, &code).await? {
            return Err(AppError::Conflict(format!("Account code {code} already exists")));
        }
        next.code = code;
    }
    if let Some(name) = dto.name {
        next.name = name;
    }
    if let Some(t) = dto.account_type {
        next.account_type = t;
    }
    if let Some(s) = dto.subtype {
        next.subtype = s;
    }
    if let Some(d) = dto.description {
        next.description = d;
    }
    if let Some(p) = dto.parent_id {
        if let Some(parent) = &p {
            find_one(&mut tx, parent).await?;
        }
        next.parent_id = p;
    }
    if next != current {
        sqlx::query(
            r#"UPDATE "accounts" SET "code"=?1, "name"=?2, "type"=?3, "subtype"=?4, "description"=?5, "parentId"=?6, "updatedAt"=?7
               WHERE "id"=?8"#,
        )
        .bind(&next.code)
        .bind(&next.name)
        .bind(next.account_type.as_str())
        .bind(&next.subtype)
        .bind(&next.description)
        .bind(&next.parent_id)
        .bind(now_iso())
        .bind(id)
        .execute(&mut *tx)
        .await?;
    }
    let account = find_one(&mut tx, id).await?;
    tx.commit().await?;
    Ok(account)
}

fn is_foreign_key_violation(e: &sqlx::Error) -> bool {
    match e {
        // SQLITE_CONSTRAINT_FOREIGNKEY
        sqlx::Error::Database(db) => db.code().as_deref() == Some("787"),
        _ => false,
    }
}

/// Deletes an unused account. An account referenced by ledger lines or documents stays.
pub async fn remove(db: &Db, id: &str) -> AppResult<()> {
    let mut tx = db.begin().await?;
    find_one(&mut tx, id).await?;
    match sqlx::query(r#"DELETE FROM "accounts" WHERE "id" = ?1"#).bind(id).execute(&mut *tx).await {
        Ok(_) => {}
        Err(e) if is_foreign_key_violation(&e) => {
            return Err(AppError::bad_request(
                "Cannot delete this account because it is used by ledger entries, invoices or bills. It stays for your records.",
            ));
        }
        Err(e) => return Err(e.into()),
    }
    tx.commit().await?;
    Ok(())
}

/// Default chart (reference `AccountsService.seed`), inserted only when no account exists.
pub const DEFAULT_CHART: &[(&str, &str, AccountType, &str)] = &[
    ("1000", "Cash", AccountType::Asset, "current_asset"),
    ("1100", "Accounts Receivable", AccountType::Asset, "current_asset"),
    ("1200", "Inventory", AccountType::Asset, "current_asset"),
    ("2000", "Accounts Payable", AccountType::Liability, "current_liability"),
    ("2100", "Accrued Liabilities", AccountType::Liability, "current_liability"),
    ("3000", "Owner's Equity", AccountType::Equity, "equity"),
    ("4000", "Revenue", AccountType::Revenue, "operating_revenue"),
    ("5000", "Cost of Goods Sold", AccountType::Expense, "cogs"),
    ("6000", "Operating Expenses", AccountType::Expense, "operating_expense"),
    ("6100", "Salaries Expense", AccountType::Expense, "operating_expense"),
];

/// Adds every default account whose code does not exist yet; existing accounts are untouched.
/// Approved deviation D10: the reference seeded only an empty chart, so after the payroll
/// migration (11 accounts) it never added 1000 Cash, 1100 AR, 2000 AP … on a fresh install.
/// Returns how many accounts were added.
pub async fn seed(db: &Db) -> AppResult<usize> {
    let mut tx = db.begin().await?;
    let mut missing = Vec::new();
    for row in DEFAULT_CHART {
        if !code_taken(&mut tx, row.0).await? {
            missing.push(*row);
        }
    }
    insert_chart(&mut tx, &missing).await?;
    tx.commit().await?;
    Ok(missing.len())
}

pub async fn insert_chart(tx: &mut Tx, chart: &[(&str, &str, AccountType, &str)]) -> AppResult<()> {
    let now = now_iso();
    for (code, name, t, subtype) in chart {
        sqlx::query(
            r#"INSERT INTO "accounts" ("id","code","name","type","subtype","isActive","createdAt","updatedAt")
               VALUES (?1,?2,?3,?4,?5,1,?6,?6)"#,
        )
        .bind(new_id())
        .bind(code)
        .bind(name)
        .bind(t.as_str())
        .bind(subtype)
        .bind(&now)
        .execute(&mut **tx)
        .await?;
    }
    Ok(())
}
