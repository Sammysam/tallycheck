//! Customers and vendors (reference `customers.service.ts`, `vendors.service.ts`): the same
//! shape and rules, different tables and messages.

use app_core::ids::new_id;
use app_core::time::now_iso;
use app_core::{AppError, AppResult};
use database::Db;
use serde::Serialize;
use sqlx::sqlite::SqliteRow;
use sqlx::{Row, SqliteConnection};

#[derive(Debug, Clone, Copy)]
pub enum PartyKind {
    Customer,
    Vendor,
}

impl PartyKind {
    fn table(self) -> &'static str {
        match self {
            PartyKind::Customer => "customers",
            PartyKind::Vendor => "vendors",
        }
    }

    fn label(self) -> &'static str {
        match self {
            PartyKind::Customer => "Customer",
            PartyKind::Vendor => "Vendor",
        }
    }

    fn in_use_message(self) -> &'static str {
        match self {
            PartyKind::Customer => {
                "Cannot delete this customer because it is linked to existing invoices. Void or remove those invoices first."
            }
            PartyKind::Vendor => {
                "Cannot delete this vendor because it is linked to existing bills. Void or remove those bills first."
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Party {
    pub id: String,
    pub name: String,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub address: Option<String>,
    pub tax_id: Option<String>,
    pub is_active: bool,
    pub created_at: String,
    pub updated_at: String,
}

const COLUMNS: &str = r#""id","name","email","phone","address","taxId","isActive","createdAt","updatedAt""#;

fn from_row(r: &SqliteRow) -> AppResult<Party> {
    Ok(Party {
        id: r.try_get("id")?,
        name: r.try_get("name")?,
        email: r.try_get("email")?,
        phone: r.try_get("phone")?,
        address: r.try_get("address")?,
        tax_id: r.try_get("taxId")?,
        is_active: r.try_get::<i64, _>("isActive")? != 0,
        created_at: r.try_get("createdAt")?,
        updated_at: r.try_get("updatedAt")?,
    })
}

pub async fn get(conn: &mut SqliteConnection, kind: PartyKind, id: &str) -> AppResult<Option<Party>> {
    let row = sqlx::query(&format!(r#"SELECT {COLUMNS} FROM "{}" WHERE "id" = ?1"#, kind.table()))
        .bind(id)
        .fetch_optional(conn)
        .await?;
    row.as_ref().map(from_row).transpose()
}

/// Reference `findOne`: 404 `Customer <id> not found` / `Vendor <id> not found`.
pub async fn find_one(conn: &mut SqliteConnection, kind: PartyKind, id: &str) -> AppResult<Party> {
    get(conn, kind, id).await?.ok_or_else(|| AppError::NotFound(format!("{} {id} not found", kind.label())))
}

/// Active records ordered by name.
pub async fn find_all(db: &Db, kind: PartyKind) -> AppResult<Vec<Party>> {
    let rows = sqlx::query(&format!(
        r#"SELECT {COLUMNS} FROM "{}" WHERE "isActive" = 1 ORDER BY "name" ASC, rowid ASC"#,
        kind.table()
    ))
    .fetch_all(db.read())
    .await?;
    rows.iter().map(from_row).collect()
}

/// Create/PATCH fields. For the nullable ones `Some(None)` means "sent as null".
#[derive(Default)]
pub struct PartyFields {
    pub name: Option<String>,
    pub email: Option<Option<String>>,
    pub phone: Option<Option<String>>,
    pub address: Option<Option<String>>,
    pub tax_id: Option<Option<String>>,
}

pub async fn create(db: &Db, kind: PartyKind, f: PartyFields) -> AppResult<Party> {
    let id = new_id();
    let now = now_iso();
    let mut tx = db.begin().await?;
    sqlx::query(&format!(
        r#"INSERT INTO "{}" ("id","name","email","phone","address","taxId","isActive","createdAt","updatedAt")
           VALUES (?1,?2,?3,?4,?5,?6,1,?7,?7)"#,
        kind.table()
    ))
    .bind(&id)
    .bind(f.name.unwrap_or_default())
    .bind(f.email.flatten())
    .bind(f.phone.flatten())
    .bind(f.address.flatten())
    .bind(f.tax_id.flatten())
    .bind(&now)
    .execute(&mut *tx)
    .await?;
    let party = find_one(&mut tx, kind, &id).await?;
    tx.commit().await?;
    Ok(party)
}

/// `Object.assign(entity, dto)`: only sent fields change. A null name is ignored (the
/// reference crashed on the NOT NULL column).
pub async fn update(db: &Db, kind: PartyKind, id: &str, f: PartyFields) -> AppResult<Party> {
    let mut tx = db.begin().await?;
    let current = find_one(&mut tx, kind, id).await?;
    let mut next = current.clone();
    if let Some(n) = f.name {
        next.name = n;
    }
    if let Some(v) = f.email {
        next.email = v;
    }
    if let Some(v) = f.phone {
        next.phone = v;
    }
    if let Some(v) = f.address {
        next.address = v;
    }
    if let Some(v) = f.tax_id {
        next.tax_id = v;
    }
    if next != current {
        sqlx::query(&format!(
            r#"UPDATE "{}" SET "name"=?1,"email"=?2,"phone"=?3,"address"=?4,"taxId"=?5,"updatedAt"=?6 WHERE "id"=?7"#,
            kind.table()
        ))
        .bind(&next.name)
        .bind(&next.email)
        .bind(&next.phone)
        .bind(&next.address)
        .bind(&next.tax_id)
        .bind(now_iso())
        .bind(id)
        .execute(&mut *tx)
        .await?;
    }
    let party = find_one(&mut tx, kind, id).await?;
    tx.commit().await?;
    Ok(party)
}

pub(crate) fn is_foreign_key_violation(e: &sqlx::Error) -> bool {
    matches!(e, sqlx::Error::Database(db) if db.code().as_deref() == Some("787"))
}

pub async fn remove(db: &Db, kind: PartyKind, id: &str) -> AppResult<()> {
    let mut tx = db.begin().await?;
    find_one(&mut tx, kind, id).await?;
    match sqlx::query(&format!(r#"DELETE FROM "{}" WHERE "id" = ?1"#, kind.table())).bind(id).execute(&mut *tx).await {
        Ok(_) => {}
        Err(e) if is_foreign_key_violation(&e) => return Err(AppError::bad_request(kind.in_use_message())),
        Err(e) => return Err(e.into()),
    }
    tx.commit().await?;
    Ok(())
}
