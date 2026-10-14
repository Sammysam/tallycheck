//! Errors with the same HTTP shape as the reference NestJS app.
//!
//! The reference's `HttpExceptionFilter` answers every `HttpException` with
//! `{statusCode, timestamp, path, message}`. `timestamp` and `path` depend on the request, so
//! `IntoResponse` only attaches a [`NestError`] extension; the api-server's error middleware,
//! which knows the request URL, turns it into the final JSON body.
//!
//! Anything that is not an HTTP-level error becomes `Internal`, is logged, and is answered the
//! way Nest's base filter answers unknown exceptions: `{"statusCode":500,"message":"Internal
//! server error"}` with no detail leaked.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::{json, Value};

/// The `message` field: a single string, or the per-field list produced by validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ErrorMessage {
    Text(String),
    List(Vec<String>),
}

impl ErrorMessage {
    pub fn to_json(&self) -> Value {
        match self {
            ErrorMessage::Text(s) => Value::String(s.clone()),
            ErrorMessage::List(v) => json!(v),
        }
    }
}

impl From<&str> for ErrorMessage {
    fn from(s: &str) -> Self {
        ErrorMessage::Text(s.to_string())
    }
}

impl From<String> for ErrorMessage {
    fn from(s: String) -> Self {
        ErrorMessage::Text(s)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("bad request: {0:?}")]
    BadRequest(ErrorMessage),
    #[error("unauthorized: {0}")]
    Unauthorized(String),
    #[error("forbidden: {0}")]
    Forbidden(String),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("conflict: {0}")]
    Conflict(String),
    #[error("too many requests: {0}")]
    TooManyRequests(String),
    #[error("payload too large: {0}")]
    PayloadTooLarge(String),
    /// Detail is for the log only and is never sent to the client.
    #[error("internal error: {0}")]
    Internal(String),
}

pub type AppResult<T> = Result<T, AppError>;

impl AppError {
    pub fn bad_request(msg: impl Into<String>) -> Self {
        AppError::BadRequest(ErrorMessage::Text(msg.into()))
    }

    pub fn validation(messages: Vec<String>) -> Self {
        AppError::BadRequest(ErrorMessage::List(messages))
    }

    pub fn internal(detail: impl std::fmt::Display) -> Self {
        AppError::Internal(detail.to_string())
    }

    /// Nest's `new UnauthorizedException()` with no argument.
    pub fn unauthorized_default() -> Self {
        AppError::Unauthorized("Unauthorized".to_string())
    }

    pub fn status(&self) -> StatusCode {
        match self {
            AppError::BadRequest(_) => StatusCode::BAD_REQUEST,
            AppError::Unauthorized(_) => StatusCode::UNAUTHORIZED,
            AppError::Forbidden(_) => StatusCode::FORBIDDEN,
            AppError::NotFound(_) => StatusCode::NOT_FOUND,
            AppError::Conflict(_) => StatusCode::CONFLICT,
            AppError::TooManyRequests(_) => StatusCode::TOO_MANY_REQUESTS,
            AppError::PayloadTooLarge(_) => StatusCode::PAYLOAD_TOO_LARGE,
            AppError::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    fn message(&self) -> Option<ErrorMessage> {
        match self {
            AppError::BadRequest(m) => Some(m.clone()),
            AppError::Unauthorized(s)
            | AppError::Forbidden(s)
            | AppError::NotFound(s)
            | AppError::Conflict(s)
            | AppError::TooManyRequests(s)
            | AppError::PayloadTooLarge(s) => Some(ErrorMessage::Text(s.clone())),
            AppError::Internal(_) => None,
        }
    }
}

impl From<sqlx::Error> for AppError {
    fn from(e: sqlx::Error) -> Self {
        AppError::Internal(format!("database: {e}"))
    }
}

impl From<sqlx::migrate::MigrateError> for AppError {
    fn from(e: sqlx::migrate::MigrateError) -> Self {
        AppError::Internal(format!("migration: {e}"))
    }
}

/// Response extension read by the api-server error middleware.
#[derive(Debug, Clone)]
pub struct NestError {
    pub status: StatusCode,
    pub message: ErrorMessage,
}

/// Body for errors that never reached an HTTP-exception path (Nest's base filter).
pub fn internal_error_body() -> Value {
    json!({ "statusCode": 500, "message": "Internal server error" })
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let status = self.status();
        match self.message() {
            Some(message) => {
                let mut res = status.into_response();
                res.extensions_mut().insert(NestError { status, message });
                res
            }
            None => {
                tracing::error!(error = %self, "internal error");
                (status, Json(internal_error_body())).into_response()
            }
        }
    }
}

/// Builds the final `{statusCode, timestamp, path, message}` body.
pub fn nest_error_body(status: StatusCode, path: &str, message: &ErrorMessage) -> Value {
    json!({
        "statusCode": status.as_u16(),
        "timestamp": crate::time::now_iso(),
        "path": path,
        "message": message.to_json(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn http_errors_carry_extension_and_internal_errors_do_not_leak() {
        let res = AppError::NotFound("User x not found".into()).into_response();
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
        let ext = res.extensions().get::<NestError>().cloned();
        assert_eq!(ext.map(|e| e.message), Some(ErrorMessage::Text("User x not found".into())));

        let res = AppError::internal("secret detail").into_response();
        assert_eq!(res.status(), StatusCode::INTERNAL_SERVER_ERROR);
        assert!(res.extensions().get::<NestError>().is_none());
    }

    #[test]
    fn body_shape_matches_nest() {
        let body = nest_error_body(
            StatusCode::BAD_REQUEST,
            "/api/users?x=1",
            &ErrorMessage::List(vec!["a".into(), "b".into()]),
        );
        assert_eq!(body["statusCode"], 400);
        assert_eq!(body["path"], "/api/users?x=1");
        assert_eq!(body["message"], json!(["a", "b"]));
        assert!(body["timestamp"].as_str().is_some_and(|t| t.ends_with('Z')));
    }
}
