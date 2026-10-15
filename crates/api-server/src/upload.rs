//! Multipart uploads the way the reference's `FileInterceptor('file', { limits: { fileSize } })`
//! (multer) receives them.
//!
//! * One file part named `file`; text parts become body fields (a repeated name becomes an array).
//! * A file larger than the route's limit is refused with 413 `File too large` while it streams,
//!   so an oversized upload is never held in memory in full.
//! * A file part under another name, or a second `file`, is 400 `Unexpected field`.
//! * A request that is not `multipart/form-data` has no file; a JSON body still supplies fields.
//!
//! Like the other body extractors it must be the last handler argument, so authentication and
//! permission checks run first (Nest's guards run before interceptors).

use crate::body::JsonBody;
use app_core::AppError;
use axum::extract::{FromRequest, Multipart, Request};
use axum::http::header::CONTENT_TYPE;
use employees::{Upload, MAX_DOCUMENT_BYTES, MAX_PHOTO_BYTES};
use serde_json::{Map, Value};

/// Room for the text fields and multipart framing on top of the largest file.
pub const UPLOAD_BODY_LIMIT: usize = MAX_DOCUMENT_BYTES + 1024 * 1024;

pub struct UploadForm<const LIMIT: usize> {
    pub fields: Map<String, Value>,
    pub file: Option<Upload>,
}

pub type PhotoForm = UploadForm<MAX_PHOTO_BYTES>;
pub type DocumentForm = UploadForm<MAX_DOCUMENT_BYTES>;

fn too_large() -> AppError {
    AppError::PayloadTooLarge("File too large".into())
}

fn malformed(e: &axum::extract::multipart::MultipartError) -> AppError {
    if e.status() == axum::http::StatusCode::PAYLOAD_TOO_LARGE {
        too_large()
    } else {
        AppError::bad_request("Malformed multipart body")
    }
}

impl<S: Send + Sync, const LIMIT: usize> FromRequest<S> for UploadForm<LIMIT> {
    type Rejection = AppError;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        let is_multipart = req
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|ct| ct.trim_start().to_ascii_lowercase().starts_with("multipart/form-data"));
        if !is_multipart {
            let body = JsonBody::from_request(req, state).await?;
            return Ok(UploadForm { fields: body.object(), file: None });
        }
        let mut multipart =
            Multipart::from_request(req, state).await.map_err(|_| AppError::bad_request("Malformed multipart body"))?;
        let mut fields = Map::new();
        let mut file: Option<Upload> = None;
        while let Some(mut field) = multipart.next_field().await.map_err(|e| malformed(&e))? {
            let name = field.name().unwrap_or_default().to_string();
            if let Some(original_name) = field.file_name().map(str::to_string) {
                if name != "file" || file.is_some() {
                    return Err(AppError::bad_request("Unexpected field"));
                }
                let mime_type = field.content_type().unwrap_or("application/octet-stream").to_string();
                let mut bytes = Vec::new();
                while let Some(chunk) = field.chunk().await.map_err(|e| malformed(&e))? {
                    if bytes.len() + chunk.len() > LIMIT {
                        return Err(too_large());
                    }
                    bytes.extend_from_slice(&chunk);
                }
                file = Some(Upload { original_name, mime_type, bytes });
            } else {
                let text = field.text().await.map_err(|e| malformed(&e))?;
                match fields.get_mut(&name) {
                    Some(Value::Array(items)) => items.push(Value::String(text)),
                    Some(existing) => {
                        let first = existing.take();
                        *existing = Value::Array(vec![first, Value::String(text)]);
                    }
                    None => {
                        fields.insert(name, Value::String(text));
                    }
                }
            }
        }
        Ok(UploadForm { fields, file })
    }
}
