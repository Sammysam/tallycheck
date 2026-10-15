//! W4 routes: employees, photos and documents (`employees` permission) and salaries, increments
//! and bank details under `/payroll/employees` (`payroll` permission). Reference
//! `employees.controller.ts` and `payroll.controller.ts`.

use crate::body::JsonBody;
use crate::extract::{parse_uuid, perm, Authorized};
use crate::state::AppState;
use crate::upload::{DocumentForm, PhotoForm};
use crate::validate::{hr, query_map};
use app_core::AppResult;
use auth::User;
use axum::extract::{Path, Query, State};
use axum::http::header::{CACHE_CONTROL, CONTENT_DISPOSITION, CONTENT_TYPE};
use axum::http::{HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use documents::StoredFile;
use employees::salaries::redact;
use employees::{docs, records, salaries, Actor};
use serde::Serialize;
use serde_json::Value;

type Pairs = Query<Vec<(String, String)>>;

/// Bank details and loan balance only go to callers who may see payroll (Admin or `payroll`).
fn shown_to<T: Serialize>(user: &User, value: &T) -> Value {
    let mut v = serde_json::to_value(value).unwrap_or(Value::Null);
    if !user.has_permission("payroll") {
        if let Some(obj) = v.as_object_mut() {
            redact(obj);
        }
    }
    v
}

fn actor(user: &User) -> Actor {
    Actor { id: Some(user.id.clone()), username: Some(user.username.clone()) }
}

/// `encodeURIComponent`.
fn encode_uri_component(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&b) {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Streams a stored file back: files go through the server, never a public URL, and are never
/// cached. With a name, `Content-Disposition` is `attachment` (or `inline`).
fn send(file: StoredFile, filename: Option<&str>, inline: bool) -> Response {
    let mut res = file.bytes.into_response();
    let headers = res.headers_mut();
    headers.insert(
        CONTENT_TYPE,
        HeaderValue::from_str(&file.content_type).unwrap_or(HeaderValue::from_static("application/octet-stream")),
    );
    headers.insert(CACHE_CONTROL, HeaderValue::from_static("private, no-store"));
    if let Some(name) = filename {
        let safe: String = name.chars().map(|c| if matches!(c, '"' | '\r' | '\n') { '_' } else { c }).collect();
        let disposition = format!(
            "{}; filename=\"{safe}\"; filename*=UTF-8''{}",
            if inline { "inline" } else { "attachment" },
            encode_uri_component(name)
        );
        // Non-ASCII names: the plain filename= part is reduced to ASCII; filename* carries the
        // exact name (Node wrote the raw UTF-8 bytes, which browsers read as Latin-1).
        let ascii: String = disposition.chars().map(|c| if c.is_ascii() { c } else { '_' }).collect();
        if let Ok(v) = HeaderValue::from_str(&ascii) {
            headers.insert(CONTENT_DISPOSITION, v);
        }
    }
    res
}

// ─── Employees ──────────────────────────────────────────────────────────────

pub async fn list(
    State(s): State<AppState>,
    a: Authorized<perm::Employees>,
    Query(q): Pairs,
) -> AppResult<Json<Vec<Value>>> {
    let (q, status) = hr::employee_query(query_map(q))?;
    let rows = records::find_all(&s.db, q.as_deref(), status.as_deref()).await?;
    Ok(Json(rows.iter().map(|e| shown_to(a.user(), e)).collect()))
}

pub async fn create(
    State(s): State<AppState>,
    a: Authorized<perm::Employees>,
    body: JsonBody,
) -> AppResult<(StatusCode, Json<Value>)> {
    let f = hr::employee(body.object(), false)?;
    let e = records::create(&s.db, f).await?;
    Ok((StatusCode::CREATED, Json(shown_to(a.user(), &e))))
}

pub async fn get(
    State(s): State<AppState>,
    a: Authorized<perm::Employees>,
    Path(id): Path<String>,
) -> AppResult<Json<Value>> {
    parse_uuid(&id)?;
    Ok(Json(shown_to(a.user(), &records::profile(&s.db, &id).await?)))
}

pub async fn update(
    State(s): State<AppState>,
    a: Authorized<perm::Employees>,
    Path(id): Path<String>,
    body: JsonBody,
) -> AppResult<Json<Value>> {
    parse_uuid(&id)?;
    let f = hr::employee(body.object(), true)?;
    Ok(Json(shown_to(a.user(), &records::update(&s.db, &id, f).await?)))
}

pub async fn remove(
    State(s): State<AppState>,
    _a: Authorized<perm::Employees>,
    Path(id): Path<String>,
) -> AppResult<StatusCode> {
    parse_uuid(&id)?;
    records::remove(&s.db, &s.files, &id).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ─── Photo ──────────────────────────────────────────────────────────────────

pub async fn photo_set(
    State(s): State<AppState>,
    a: Authorized<perm::Employees>,
    Path(id): Path<String>,
    form: PhotoForm,
) -> AppResult<(StatusCode, Json<Value>)> {
    parse_uuid(&id)?;
    let e = records::set_photo(&s.db, &s.files, &id, form.file).await?;
    Ok((StatusCode::CREATED, Json(shown_to(a.user(), &e))))
}

pub async fn photo_get(
    State(s): State<AppState>,
    _a: Authorized<perm::Employees>,
    Path(id): Path<String>,
) -> AppResult<Response> {
    parse_uuid(&id)?;
    Ok(send(records::get_photo(&s.db, &s.files, &id).await?, None, true))
}

pub async fn photo_remove(
    State(s): State<AppState>,
    a: Authorized<perm::Employees>,
    Path(id): Path<String>,
) -> AppResult<Json<Value>> {
    parse_uuid(&id)?;
    Ok(Json(shown_to(a.user(), &records::remove_photo(&s.db, &s.files, &id).await?)))
}

// ─── Documents ──────────────────────────────────────────────────────────────

pub async fn docs_list(
    State(s): State<AppState>,
    _a: Authorized<perm::Employees>,
    Path(id): Path<String>,
) -> AppResult<Json<Vec<docs::Document>>> {
    parse_uuid(&id)?;
    Ok(Json(docs::list(&s.db, &id).await?))
}

pub async fn docs_add(
    State(s): State<AppState>,
    a: Authorized<perm::Employees>,
    Path(id): Path<String>,
    form: DocumentForm,
) -> AppResult<(StatusCode, Json<Value>)> {
    parse_uuid(&id)?;
    let f = hr::document(form.fields)?;
    let doc = docs::add(&s.db, &s.files, &id, f, form.file, &actor(a.user())).await?;
    Ok((StatusCode::CREATED, Json(doc.created_json())))
}

pub async fn docs_replace(
    State(s): State<AppState>,
    a: Authorized<perm::Employees>,
    Path((id, doc_id)): Path<(String, String)>,
    form: DocumentForm,
) -> AppResult<(StatusCode, Json<Value>)> {
    parse_uuid(&id)?;
    parse_uuid(&doc_id)?;
    let f = hr::document_replace(form.fields)?;
    let doc = docs::replace(&s.db, &s.files, &id, &doc_id, f, form.file, &actor(a.user())).await?;
    Ok((StatusCode::CREATED, Json(doc.created_json())))
}

pub async fn docs_update(
    State(s): State<AppState>,
    _a: Authorized<perm::Employees>,
    Path((id, doc_id)): Path<(String, String)>,
    body: JsonBody,
) -> AppResult<Json<docs::Document>> {
    parse_uuid(&id)?;
    parse_uuid(&doc_id)?;
    let f = hr::document_patch(body.object())?;
    Ok(Json(docs::update(&s.db, &id, &doc_id, f).await?))
}

/// `?inline=1` opens it in the browser (PDFs, images); otherwise it downloads.
pub async fn docs_download(
    State(s): State<AppState>,
    _a: Authorized<perm::Employees>,
    Path((id, doc_id)): Path<(String, String)>,
    Query(q): Pairs,
) -> AppResult<Response> {
    parse_uuid(&id)?;
    parse_uuid(&doc_id)?;
    let inline = q.iter().find(|(k, _)| k == "inline").is_some_and(|(_, v)| v == "1");
    let (doc, file) = docs::open(&s.db, &s.files, &id, &doc_id).await?;
    Ok(send(file, Some(&doc.original_name), inline))
}

pub async fn docs_remove(
    State(s): State<AppState>,
    _a: Authorized<perm::Employees>,
    Path((id, doc_id)): Path<(String, String)>,
) -> AppResult<StatusCode> {
    parse_uuid(&id)?;
    parse_uuid(&doc_id)?;
    docs::remove(&s.db, &s.files, &id, &doc_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ─── Payroll: salaries, increments, bank details ────────────────────────────

pub async fn payroll_get(
    State(s): State<AppState>,
    _a: Authorized<perm::Payroll>,
    Path(id): Path<String>,
    Query(q): Pairs,
) -> AppResult<Json<Value>> {
    parse_uuid(&id)?;
    let as_of = q.into_iter().find(|(k, _)| k == "asOf").map(|(_, v)| v);
    Ok(Json(salaries::employee_payroll(&s.db, &id, as_of.as_deref()).await?))
}

pub async fn payroll_bank(
    State(s): State<AppState>,
    _a: Authorized<perm::Payroll>,
    Path(id): Path<String>,
    body: JsonBody,
) -> AppResult<Json<Value>> {
    parse_uuid(&id)?;
    let f = hr::bank(body.object())?;
    Ok(Json(salaries::update_bank(&s.db, &id, f).await?))
}

pub async fn salary_add(
    State(s): State<AppState>,
    a: Authorized<perm::Payroll>,
    Path(id): Path<String>,
    body: JsonBody,
) -> AppResult<(StatusCode, Json<Value>)> {
    parse_uuid(&id)?;
    let f = hr::salary(body.object(), false)?;
    Ok((StatusCode::CREATED, Json(salaries::add(&s.db, &id, f, &actor(a.user())).await?)))
}

pub async fn salary_update(
    State(s): State<AppState>,
    _a: Authorized<perm::Payroll>,
    Path((id, salary_id)): Path<(String, String)>,
    body: JsonBody,
) -> AppResult<Json<Value>> {
    parse_uuid(&id)?;
    parse_uuid(&salary_id)?;
    let f = hr::salary(body.object(), true)?;
    Ok(Json(salaries::update(&s.db, &id, &salary_id, f).await?))
}

pub async fn salary_remove(
    State(s): State<AppState>,
    _a: Authorized<perm::Payroll>,
    Path((id, salary_id)): Path<(String, String)>,
) -> AppResult<StatusCode> {
    parse_uuid(&id)?;
    parse_uuid(&salary_id)?;
    salaries::remove(&s.db, &id, &salary_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_disposition_matches_the_reference() {
        let file = StoredFile { bytes: b"x".to_vec(), content_type: "application/pdf".into() };
        let res = send(file, Some("Visa \"2026\"\r\n.pdf"), false);
        let cd = res.headers().get(CONTENT_DISPOSITION).and_then(|v| v.to_str().ok()).unwrap_or_default();
        assert_eq!(cd, "attachment; filename=\"Visa _2026___.pdf\"; filename*=UTF-8''Visa%20%222026%22%0D%0A.pdf");
        assert_eq!(encode_uri_component("a-b_c.d!~*'()"), "a-b_c.d!~*'()");
        assert_eq!(encode_uri_component("ශ්"), "%E0%B7%81%E0%B7%8A");
    }
}
