//! Request bodies, parsed the way Express' JSON body parser feeds Nest.
//!
//! * `application/json` (or `+json`) bodies are parsed; an empty body counts as `{}`.
//! * Other content types are not parsed and validate as `{}`.
//! * Bodies above 100 KB are rejected (body-parser's default limit) with 413.
//!
//! The extractor must be the **last** handler argument so authentication and permission
//! extractors run first, as the reference's guards run before its `ValidationPipe`.

use app_core::AppError;
use axum::extract::{FromRequest, Request};
use axum::http::header::CONTENT_TYPE;
use serde_json::{Map, Value};

pub const BODY_LIMIT: usize = 100 * 1024;

pub struct JsonBody(pub Value);

impl JsonBody {
    /// The body as an object; non-object JSON validates as an empty object.
    pub fn object(&self) -> Map<String, Value> {
        self.0.as_object().cloned().unwrap_or_default()
    }
}

impl<S: Send + Sync> FromRequest<S> for JsonBody {
    type Rejection = AppError;

    async fn from_request(req: Request, _state: &S) -> Result<Self, Self::Rejection> {
        let is_json = req
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(|ct| {
                let mime = ct.split(';').next().unwrap_or("").trim().to_ascii_lowercase();
                mime == "application/json" || mime.ends_with("+json")
            })
            .unwrap_or(false);
        let bytes = axum::body::to_bytes(req.into_body(), BODY_LIMIT)
            .await
            .map_err(|_| AppError::PayloadTooLarge("request entity too large".into()))?;
        if !is_json || bytes.iter().all(u8::is_ascii_whitespace) {
            return Ok(JsonBody(Value::Object(Map::new())));
        }
        serde_json::from_slice(&bytes)
            .map(JsonBody)
            .map_err(|e| AppError::bad_request(format!("Malformed JSON body: {e}")))
    }
}
