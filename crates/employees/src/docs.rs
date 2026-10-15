//! Employee documents (reference `employees.service.ts`, second half). Replacing a document
//! never overwrites it: a new row is created, the old one is kept and marked not current.

use crate::{delete_quietly, extension, records, with_new_file, Actor, Upload, DOCUMENT_MIMES, MAX_DOCUMENT_BYTES};
use app_core::ids::new_id;
use app_core::time::now_iso;
use app_core::{AppError, AppResult};
use database::Db;
use documents::{FileStore, StoredFile};
use serde::Serialize;
use serde_json::Value;
use sqlx::sqlite::SqliteRow;
use sqlx::{Row, SqliteConnection};

pub const CATEGORIES: &[&str] = &["CERTIFICATE", "ID", "CONTRACT", "VISA", "PAYSLIP", "OTHER"];

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Document {
    pub id: String,
    pub employee_id: String,
    pub category: String,
    pub title: String,
    pub file_key: String,
    pub original_name: String,
    pub mime_type: String,
    /// A `bigint` column: pg returns it as a string on read.
    #[serde(serialize_with = "size_as_string")]
    pub size: i64,
    pub expiry_date: Option<String>,
    pub notes: Option<String>,
    pub is_current: bool,
    pub replaces_id: Option<String>,
    pub uploaded_by_id: Option<String>,
    pub uploaded_by_name: Option<String>,
    pub created_at: String,
}

fn size_as_string<S: serde::Serializer>(n: &i64, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(&n.to_string())
}

impl Document {
    /// A freshly saved row as TypeORM echoes it: `size` is still the number multer reported.
    pub fn created_json(&self) -> Value {
        let mut v = serde_json::to_value(self).unwrap_or(Value::Null);
        if let Some(obj) = v.as_object_mut() {
            obj.insert("size".into(), Value::from(self.size));
        }
        v
    }
}

const COLUMNS: &str = r#""id","employeeId","category","title","fileKey","originalName","mimeType","size","expiryDate",
"notes","isCurrent","replacesId","uploadedById","uploadedByName","createdAt""#;

fn from_row(r: &SqliteRow) -> AppResult<Document> {
    Ok(Document {
        id: r.try_get("id")?,
        employee_id: r.try_get("employeeId")?,
        category: r.try_get("category")?,
        title: r.try_get("title")?,
        file_key: r.try_get("fileKey")?,
        original_name: r.try_get("originalName")?,
        mime_type: r.try_get("mimeType")?,
        size: r.try_get("size")?,
        expiry_date: r.try_get("expiryDate")?,
        notes: r.try_get("notes")?,
        is_current: r.try_get::<i64, _>("isCurrent")? != 0,
        replaces_id: r.try_get("replacesId")?,
        uploaded_by_id: r.try_get("uploadedById")?,
        uploaded_by_name: r.try_get("uploadedByName")?,
        created_at: r.try_get("createdAt")?,
    })
}

/// Every document of an employee, newest first.
pub(crate) async fn for_employee(conn: &mut SqliteConnection, employee_id: &str) -> AppResult<Vec<Document>> {
    let rows = sqlx::query(&format!(
        r#"SELECT {COLUMNS} FROM "employee_documents" WHERE "employeeId" = ?1 ORDER BY "createdAt" DESC, rowid DESC"#
    ))
    .bind(employee_id)
    .fetch_all(conn)
    .await?;
    rows.iter().map(from_row).collect()
}

pub async fn list(db: &Db, employee_id: &str) -> AppResult<Vec<Document>> {
    let mut c = db.read().acquire().await?;
    records::find_one(&mut c, employee_id).await?;
    for_employee(&mut c, employee_id).await
}

/// Reference `getDocument`: the document must belong to that employee.
pub async fn get_document(conn: &mut SqliteConnection, employee_id: &str, id: &str) -> AppResult<Document> {
    let row =
        sqlx::query(&format!(r#"SELECT {COLUMNS} FROM "employee_documents" WHERE "id" = ?1 AND "employeeId" = ?2"#))
            .bind(id)
            .bind(employee_id)
            .fetch_optional(conn)
            .await?;
    row.as_ref().map(from_row).transpose()?.ok_or_else(|| AppError::NotFound(format!("Document {id} not found")))
}

/// Text fields riding along with an upload, or a PATCH. For nullable columns `Some(None)`
/// means "sent as null".
#[derive(Debug, Default, Clone)]
pub struct DocFields {
    pub title: Option<String>,
    pub category: Option<String>,
    pub expiry_date: Option<Option<String>>,
    pub notes: Option<Option<String>>,
}

fn assert_document_file(file: Option<Upload>) -> AppResult<Upload> {
    let file = file.ok_or_else(|| AppError::bad_request("Choose a file to upload"))?;
    if !DOCUMENT_MIMES.contains(&file.mime_type.as_str()) {
        return Err(AppError::bad_request("Documents must be PDF, JPG, PNG, WebP or Word files"));
    }
    if file.bytes.len() > MAX_DOCUMENT_BYTES {
        return Err(AppError::bad_request("Documents must be 10 MB or smaller"));
    }
    Ok(file)
}

async fn insert(conn: &mut SqliteConnection, d: &Document) -> AppResult<()> {
    sqlx::query(
        r#"INSERT INTO "employee_documents" ("id","employeeId","category","title","fileKey","originalName","mimeType",
           "size","expiryDate","notes","isCurrent","replacesId","uploadedById","uploadedByName","createdAt")
           VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)"#,
    )
    .bind(&d.id)
    .bind(&d.employee_id)
    .bind(&d.category)
    .bind(&d.title)
    .bind(&d.file_key)
    .bind(&d.original_name)
    .bind(&d.mime_type)
    .bind(d.size)
    .bind(&d.expiry_date)
    .bind(&d.notes)
    .bind(i64::from(d.is_current))
    .bind(&d.replaces_id)
    .bind(&d.uploaded_by_id)
    .bind(&d.uploaded_by_name)
    .bind(&d.created_at)
    .execute(conn)
    .await?;
    Ok(())
}

fn size_of(file: &Upload) -> AppResult<i64> {
    i64::try_from(file.bytes.len()).map_err(AppError::internal)
}

pub async fn add(
    db: &Db,
    store: &FileStore,
    employee_id: &str,
    f: DocFields,
    file: Option<Upload>,
    by: &Actor,
) -> AppResult<Document> {
    {
        let mut c = db.read().acquire().await?;
        records::find_one(&mut c, employee_id).await?;
    }
    let file = assert_document_file(file)?;
    let key = format!("employees/{employee_id}/documents/{}{}", new_id(), extension(&file.original_name));
    let doc = Document {
        id: new_id(),
        employee_id: employee_id.to_string(),
        category: f.category.unwrap_or_else(|| "OTHER".into()),
        title: f.title.unwrap_or_default(),
        file_key: key.clone(),
        original_name: file.original_name.clone(),
        mime_type: file.mime_type.clone(),
        size: size_of(&file)?,
        expiry_date: f.expiry_date.flatten(),
        notes: f.notes.flatten(),
        is_current: true,
        replaces_id: None,
        uploaded_by_id: by.id.clone(),
        uploaded_by_name: by.username.clone(),
        created_at: now_iso(),
    };
    with_new_file(store, &key, &file, || async {
        let mut tx = db.begin().await?;
        records::find_one(&mut tx, employee_id).await?;
        insert(&mut tx, &doc).await?;
        let saved = get_document(&mut tx, employee_id, &doc.id).await?;
        tx.commit().await?;
        Ok(saved)
    })
    .await
}

/// Uploads a newer version. Title, category, expiry and notes carry over unless sent.
pub async fn replace(
    db: &Db,
    store: &FileStore,
    employee_id: &str,
    id: &str,
    f: DocFields,
    file: Option<Upload>,
    by: &Actor,
) -> AppResult<Document> {
    let previous = {
        let mut c = db.read().acquire().await?;
        get_document(&mut c, employee_id, id).await?
    };
    if !previous.is_current {
        return Err(AppError::bad_request("This is an older version — replace the current one instead"));
    }
    let file = assert_document_file(file)?;
    let key = format!("employees/{employee_id}/documents/{}{}", new_id(), extension(&file.original_name));
    let next = Document {
        id: new_id(),
        employee_id: employee_id.to_string(),
        category: f.category.unwrap_or_else(|| previous.category.clone()),
        title: f.title.filter(|t| !t.trim().is_empty()).unwrap_or_else(|| previous.title.clone()),
        file_key: key.clone(),
        original_name: file.original_name.clone(),
        mime_type: file.mime_type.clone(),
        size: size_of(&file)?,
        expiry_date: f.expiry_date.unwrap_or_else(|| previous.expiry_date.clone()),
        notes: f.notes.unwrap_or_else(|| previous.notes.clone()),
        is_current: true,
        replaces_id: Some(previous.id.clone()),
        uploaded_by_id: by.id.clone(),
        uploaded_by_name: by.username.clone(),
        created_at: now_iso(),
    };
    with_new_file(store, &key, &file, || async {
        let mut tx = db.begin().await?;
        // Re-checked inside the write transaction: two replaces of the same version cannot
        // both succeed.
        let prev = get_document(&mut tx, employee_id, id).await?;
        if !prev.is_current {
            return Err(AppError::bad_request("This is an older version — replace the current one instead"));
        }
        insert(&mut tx, &next).await?;
        sqlx::query(r#"UPDATE "employee_documents" SET "isCurrent" = 0 WHERE "id" = ?1"#)
            .bind(id)
            .execute(&mut *tx)
            .await?;
        let saved = get_document(&mut tx, employee_id, &next.id).await?;
        tx.commit().await?;
        Ok(saved)
    })
    .await
}

/// Edits the label only; the file itself never changes.
pub async fn update(db: &Db, employee_id: &str, id: &str, f: DocFields) -> AppResult<Document> {
    let mut tx = db.begin().await?;
    let mut doc = get_document(&mut tx, employee_id, id).await?;
    if let Some(t) = f.title {
        doc.title = t;
    }
    if let Some(c) = f.category {
        doc.category = c;
    }
    if let Some(e) = f.expiry_date {
        doc.expiry_date = e;
    }
    if let Some(n) = f.notes {
        doc.notes = n;
    }
    sqlx::query(r#"UPDATE "employee_documents" SET "title"=?1,"category"=?2,"expiryDate"=?3,"notes"=?4 WHERE "id"=?5"#)
        .bind(&doc.title)
        .bind(&doc.category)
        .bind(&doc.expiry_date)
        .bind(&doc.notes)
        .bind(id)
        .execute(&mut *tx)
        .await?;
    let saved = get_document(&mut tx, employee_id, id).await?;
    tx.commit().await?;
    Ok(saved)
}

pub async fn open(db: &Db, store: &FileStore, employee_id: &str, id: &str) -> AppResult<(Document, StoredFile)> {
    let mut c = db.read().acquire().await?;
    let doc = get_document(&mut c, employee_id, id).await?;
    let file = store.get(&doc.file_key)?;
    Ok((doc, file))
}

/// Permanently removes one version. If it was current, the version it replaced becomes current
/// again; anything that pointed at it now points at what it replaced.
pub async fn remove(db: &Db, store: &FileStore, employee_id: &str, id: &str) -> AppResult<()> {
    let mut tx = db.begin().await?;
    let doc = get_document(&mut tx, employee_id, id).await?;
    // Re-point first: the foreign key on replacesId would otherwise null these pointers.
    sqlx::query(r#"UPDATE "employee_documents" SET "replacesId" = ?1 WHERE "replacesId" = ?2"#)
        .bind(&doc.replaces_id)
        .bind(id)
        .execute(&mut *tx)
        .await?;
    sqlx::query(r#"DELETE FROM "employee_documents" WHERE "id" = ?1"#).bind(id).execute(&mut *tx).await?;
    if doc.is_current {
        if let Some(prev) = &doc.replaces_id {
            sqlx::query(r#"UPDATE "employee_documents" SET "isCurrent" = 1 WHERE "id" = ?1"#)
                .bind(prev)
                .execute(&mut *tx)
                .await?;
        }
    }
    tx.commit().await?;
    delete_quietly(store, &[doc.file_key]);
    Ok(())
}
