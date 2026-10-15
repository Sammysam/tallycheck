//! Employee records and profile photos (reference `employees.service.ts`, first half).

use crate::docs::{self, Document};
use crate::{delete_quietly, extension, with_new_file, Upload, MAX_PHOTO_BYTES, PHOTO_MIMES};
use app_core::ids::new_id;
use app_core::money::Money;
use app_core::time::now_iso;
use app_core::{AppError, AppResult};
use database::Db;
use documents::{FileStore, StoredFile};
use serde::Serialize;
use sqlx::sqlite::SqliteRow;
use sqlx::{Row, SqliteConnection};

pub const STATUSES: &[&str] = &["ACTIVE", "ON_LEAVE", "LEFT"];
pub const EMPLOYMENT_TYPES: &[&str] = &["PERMANENT", "CONTRACT"];

/// Columns only someone with the `payroll` permission may see (reference `PAYROLL_ONLY_FIELDS`).
pub const PAYROLL_ONLY_FIELDS: &[&str] = &["bankName", "bankBranch", "bankAccountNo", "staffLoanBalance"];

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Employee {
    pub id: String,
    pub employee_id: String,
    pub name: String,
    pub job_title: Option<String>,
    pub department: Option<String>,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub join_date: Option<String>,
    pub status: String,
    pub notes: Option<String>,
    pub employment_type: String,
    pub epf_number: Option<String>,
    pub national_id: Option<String>,
    pub resigned_date: Option<String>,
    pub bank_name: Option<String>,
    pub bank_branch: Option<String>,
    pub bank_account_no: Option<String>,
    #[serde(serialize_with = "app_core::money::as_db_string")]
    pub staff_loan_balance: Money,
    pub photo_key: Option<String>,
    pub photo_mime: Option<String>,
    pub user_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// `getProfile`: the employee plus every document, newest first.
#[derive(Debug, Serialize)]
pub struct Profile {
    #[serde(flatten)]
    pub employee: Employee,
    pub documents: Vec<Document>,
}

const COLUMNS: &str = r#""id","employeeId","name","jobTitle","department","email","phone","joinDate","status","notes",
"employmentType","epfNumber","nationalId","resignedDate","bankName","bankBranch","bankAccountNo","staffLoanBalance",
"photoKey","photoMime","userId","createdAt","updatedAt""#;

fn from_row(r: &SqliteRow) -> AppResult<Employee> {
    Ok(Employee {
        id: r.try_get("id")?,
        employee_id: r.try_get("employeeId")?,
        name: r.try_get("name")?,
        job_title: r.try_get("jobTitle")?,
        department: r.try_get("department")?,
        email: r.try_get("email")?,
        phone: r.try_get("phone")?,
        join_date: r.try_get("joinDate")?,
        status: r.try_get("status")?,
        notes: r.try_get("notes")?,
        employment_type: r.try_get("employmentType")?,
        epf_number: r.try_get("epfNumber")?,
        national_id: r.try_get("nationalId")?,
        resigned_date: r.try_get("resignedDate")?,
        bank_name: r.try_get("bankName")?,
        bank_branch: r.try_get("bankBranch")?,
        bank_account_no: r.try_get("bankAccountNo")?,
        staff_loan_balance: Money::from_cents(r.try_get("staffLoanBalance")?),
        photo_key: r.try_get("photoKey")?,
        photo_mime: r.try_get("photoMime")?,
        user_id: r.try_get("userId")?,
        created_at: r.try_get("createdAt")?,
        updated_at: r.try_get("updatedAt")?,
    })
}

pub async fn get(conn: &mut SqliteConnection, id: &str) -> AppResult<Option<Employee>> {
    let row = sqlx::query(&format!(r#"SELECT {COLUMNS} FROM "employees" WHERE "id" = ?1"#))
        .bind(id)
        .fetch_optional(conn)
        .await?;
    row.as_ref().map(from_row).transpose()
}

/// Reference `findOne`: 404 `Employee <id> not found`.
pub async fn find_one(conn: &mut SqliteConnection, id: &str) -> AppResult<Employee> {
    get(conn, id).await?.ok_or_else(|| AppError::NotFound(format!("Employee {id} not found")))
}

pub async fn profile(db: &Db, id: &str) -> AppResult<Profile> {
    let mut c = db.read().acquire().await?;
    let employee = find_one(&mut c, id).await?;
    let documents = docs::for_employee(&mut c, id).await?;
    Ok(Profile { employee, documents })
}

/// Reference `findAll`: ordered by name; optional status filter; `q` (trimmed) searched
/// case-insensitively in staff number, name, department, job title and email.
pub async fn find_all(db: &Db, q: Option<&str>, status: Option<&str>) -> AppResult<Vec<Employee>> {
    let q = q.map(str::trim).filter(|s| !s.is_empty());
    let mut sql = format!(r#"SELECT {COLUMNS} FROM "employees" WHERE 1=1"#);
    if status.is_some() {
        sql.push_str(r#" AND "status" = ?1"#);
    }
    if q.is_some() {
        // As with the reference's ILIKE, the search text is not escaped: % and _ are wildcards.
        sql.push_str(
            r#" AND ("employeeId" LIKE ?2 OR "name" LIKE ?2 OR "department" LIKE ?2
                    OR "jobTitle" LIKE ?2 OR "email" LIKE ?2)"#,
        );
    }
    sql.push_str(r#" ORDER BY "name" ASC, rowid ASC"#);
    let rows = sqlx::query(&sql).bind(status).bind(q.map(|q| format!("%{q}%"))).fetch_all(db.read()).await?;
    rows.iter().map(from_row).collect()
}

/// Create / PATCH fields after validation. For nullable columns `Some(None)` means "sent as
/// null" (clears it); `None` means "not sent".
#[derive(Debug, Default, Clone)]
pub struct EmployeeFields {
    pub employee_id: Option<String>,
    pub name: Option<String>,
    pub job_title: Option<Option<String>>,
    pub department: Option<Option<String>>,
    pub email: Option<Option<String>>,
    pub phone: Option<Option<String>>,
    pub join_date: Option<Option<String>>,
    pub status: Option<String>,
    pub notes: Option<Option<String>>,
    pub employment_type: Option<String>,
    pub epf_number: Option<Option<String>>,
    pub national_id: Option<Option<String>>,
    pub resigned_date: Option<Option<String>>,
    pub user_id: Option<Option<String>>,
}

/// Case-insensitive: "cen-001" and "CEN-001" are the same person.
async fn assert_employee_id_free(
    conn: &mut SqliteConnection,
    employee_id: &str,
    except: Option<&str>,
) -> AppResult<()> {
    let clash: Option<String> = sqlx::query_scalar(
        r#"SELECT "name" FROM "employees" WHERE lower("employeeId") = lower(?1) AND (?2 IS NULL OR "id" <> ?2) LIMIT 1"#,
    )
    .bind(employee_id)
    .bind(except)
    .fetch_optional(&mut *conn)
    .await?;
    match clash {
        Some(name) => Err(AppError::bad_request(format!("Employee ID \"{employee_id}\" is already used by {name}"))),
        None => Ok(()),
    }
}

/// `userId` must name an existing login account. The reference had no foreign key here; this
/// port adds one (deviation D6), so a dangling id is refused up front instead of failing the write.
async fn assert_user_exists(conn: &mut SqliteConnection, user_id: Option<&str>) -> AppResult<()> {
    let Some(id) = user_id else { return Ok(()) };
    let found: Option<String> =
        sqlx::query_scalar(r#"SELECT "id" FROM "users" WHERE "id" = ?1"#).bind(id).fetch_optional(&mut *conn).await?;
    found.map(|_| ()).ok_or_else(|| AppError::NotFound(format!("User {id} not found")))
}

fn apply(e: &mut Employee, f: EmployeeFields) {
    // Null for a NOT NULL column never reaches here (validation drops it); the reference failed
    // with a database error.
    if let Some(v) = f.employee_id {
        e.employee_id = v;
    }
    if let Some(v) = f.name {
        e.name = v;
    }
    if let Some(v) = f.status {
        e.status = v;
    }
    if let Some(v) = f.employment_type {
        e.employment_type = v;
    }
    for (slot, v) in [
        (&mut e.job_title, f.job_title),
        (&mut e.department, f.department),
        (&mut e.email, f.email),
        (&mut e.phone, f.phone),
        (&mut e.join_date, f.join_date),
        (&mut e.notes, f.notes),
        (&mut e.epf_number, f.epf_number),
        (&mut e.national_id, f.national_id),
        (&mut e.resigned_date, f.resigned_date),
        (&mut e.user_id, f.user_id),
    ] {
        if let Some(v) = v {
            *slot = v;
        }
    }
}

async fn write(conn: &mut SqliteConnection, e: &Employee, updated_at: &str) -> AppResult<()> {
    sqlx::query(
        r#"UPDATE "employees" SET "employeeId"=?1,"name"=?2,"jobTitle"=?3,"department"=?4,"email"=?5,"phone"=?6,
           "joinDate"=?7,"status"=?8,"notes"=?9,"employmentType"=?10,"epfNumber"=?11,"nationalId"=?12,
           "resignedDate"=?13,"bankName"=?14,"bankBranch"=?15,"bankAccountNo"=?16,"staffLoanBalance"=?17,
           "photoKey"=?18,"photoMime"=?19,"userId"=?20,"updatedAt"=?21 WHERE "id"=?22"#,
    )
    .bind(&e.employee_id)
    .bind(&e.name)
    .bind(&e.job_title)
    .bind(&e.department)
    .bind(&e.email)
    .bind(&e.phone)
    .bind(&e.join_date)
    .bind(&e.status)
    .bind(&e.notes)
    .bind(&e.employment_type)
    .bind(&e.epf_number)
    .bind(&e.national_id)
    .bind(&e.resigned_date)
    .bind(&e.bank_name)
    .bind(&e.bank_branch)
    .bind(&e.bank_account_no)
    .bind(e.staff_loan_balance.cents())
    .bind(&e.photo_key)
    .bind(&e.photo_mime)
    .bind(&e.user_id)
    .bind(updated_at)
    .bind(&e.id)
    .execute(conn)
    .await?;
    Ok(())
}

/// Saves `next` if it differs from `current` (TypeORM's save skips unchanged rows, so
/// `updatedAt` only moves on a real change) and returns the stored row.
pub(crate) async fn save_if_changed(
    conn: &mut SqliteConnection,
    current: &Employee,
    next: &Employee,
) -> AppResult<Employee> {
    if next != current {
        write(&mut *conn, next, &now_iso()).await?;
    }
    find_one(conn, &next.id).await
}

pub async fn create(db: &Db, f: EmployeeFields) -> AppResult<Employee> {
    let id = new_id();
    let now = now_iso();
    let mut tx = db.begin().await?;
    let employee_id = f.employee_id.clone().unwrap_or_default();
    assert_employee_id_free(&mut tx, &employee_id, None).await?;
    assert_user_exists(&mut tx, f.user_id.clone().flatten().as_deref()).await?;
    let mut e = Employee {
        id: id.clone(),
        employee_id: String::new(),
        name: String::new(),
        job_title: None,
        department: None,
        email: None,
        phone: None,
        join_date: None,
        status: "ACTIVE".into(),
        notes: None,
        employment_type: "PERMANENT".into(),
        epf_number: None,
        national_id: None,
        resigned_date: None,
        bank_name: None,
        bank_branch: None,
        bank_account_no: None,
        staff_loan_balance: Money::ZERO,
        photo_key: None,
        photo_mime: None,
        user_id: None,
        created_at: now.clone(),
        updated_at: now.clone(),
    };
    apply(&mut e, f);
    sqlx::query(
        r#"INSERT INTO "employees" ("id","employeeId","name","createdAt","updatedAt") VALUES (?1,?2,?3,?4,?4)"#,
    )
    .bind(&id)
    .bind(&e.employee_id)
    .bind(&e.name)
    .bind(&now)
    .execute(&mut *tx)
    .await?;
    write(&mut tx, &e, &now).await?;
    let saved = find_one(&mut tx, &id).await?;
    tx.commit().await?;
    Ok(saved)
}

pub async fn update(db: &Db, id: &str, f: EmployeeFields) -> AppResult<Employee> {
    let mut tx = db.begin().await?;
    let current = find_one(&mut tx, id).await?;
    if let Some(new_staff_no) = &f.employee_id {
        if *new_staff_no != current.employee_id {
            assert_employee_id_free(&mut tx, new_staff_no, Some(id)).await?;
        }
    }
    if let Some(Some(user)) = &f.user_id {
        assert_user_exists(&mut tx, Some(user)).await?;
    }
    let mut next = current.clone();
    apply(&mut next, f);
    let saved = save_if_changed(&mut tx, &current, &next).await?;
    tx.commit().await?;
    Ok(saved)
}

/// Removes the record (documents and salaries cascade), then every file that belonged to it.
/// Rows first: if the database refuses, no files are lost.
pub async fn remove(db: &Db, store: &FileStore, id: &str) -> AppResult<()> {
    let mut tx = db.begin().await?;
    let e = find_one(&mut tx, id).await?;
    let mut keys: Vec<String> =
        sqlx::query_scalar(r#"SELECT "fileKey" FROM "employee_documents" WHERE "employeeId" = ?1"#)
            .bind(id)
            .fetch_all(&mut *tx)
            .await?;
    if let Some(k) = e.photo_key {
        keys.push(k);
    }
    sqlx::query(r#"DELETE FROM "employees" WHERE "id" = ?1"#).bind(id).execute(&mut *tx).await?;
    tx.commit().await?;
    delete_quietly(store, &keys);
    Ok(())
}

// ─── Photo ──────────────────────────────────────────────────────────────────

pub async fn set_photo(db: &Db, store: &FileStore, id: &str, file: Option<Upload>) -> AppResult<Employee> {
    {
        let mut c = db.read().acquire().await?;
        find_one(&mut c, id).await?;
    }
    let file = file.ok_or_else(|| AppError::bad_request("Choose an image to upload"))?;
    if !PHOTO_MIMES.contains(&file.mime_type.as_str()) {
        return Err(AppError::bad_request("Profile photo must be a JPG, PNG or WebP image"));
    }
    if file.bytes.len() > MAX_PHOTO_BYTES {
        return Err(AppError::bad_request("Profile photo must be 5 MB or smaller"));
    }
    let key = format!("employees/{id}/photo/{}{}", new_id(), extension(&file.original_name));
    let (saved, previous) = with_new_file(store, &key, &file, || async {
        let mut tx = db.begin().await?;
        let current = find_one(&mut tx, id).await?;
        let mut next = current.clone();
        next.photo_key = Some(key.clone());
        next.photo_mime = Some(file.mime_type.clone());
        let saved = save_if_changed(&mut tx, &current, &next).await?;
        tx.commit().await?;
        Ok((saved, current.photo_key))
    })
    .await?;
    if let Some(p) = previous {
        delete_quietly(store, &[p]);
    }
    Ok(saved)
}

pub async fn get_photo(db: &Db, store: &FileStore, id: &str) -> AppResult<StoredFile> {
    let mut c = db.read().acquire().await?;
    let e = find_one(&mut c, id).await?;
    let key = e.photo_key.ok_or_else(|| AppError::NotFound("This employee has no photo".into()))?;
    store.get(&key)
}

pub async fn remove_photo(db: &Db, store: &FileStore, id: &str) -> AppResult<Employee> {
    let mut tx = db.begin().await?;
    let current = find_one(&mut tx, id).await?;
    let mut next = current.clone();
    next.photo_key = None;
    next.photo_mime = None;
    let saved = save_if_changed(&mut tx, &current, &next).await?;
    tx.commit().await?;
    if let Some(p) = current.photo_key {
        delete_quietly(store, &[p]);
    }
    Ok(saved)
}
