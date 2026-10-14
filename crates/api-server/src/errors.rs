//! Turns `AppError` markers into the reference's error JSON and reproduces Express/Nest
//! behaviour for unknown routes.

use app_core::error::{nest_error_body, NestError};
use app_core::{AppError, ErrorMessage};
use axum::extract::{OriginalUri, Request};
use axum::http::{header, HeaderValue, Method, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

fn full_url(req: &Request) -> String {
    let uri = req.extensions().get::<OriginalUri>().map(|o| o.0.clone()).unwrap_or_else(|| req.uri().clone());
    uri.path_and_query().map(|p| p.as_str().to_string()).unwrap_or_else(|| uri.path().to_string())
}

/// Nest's router answers unmatched paths and methods with `Cannot <METHOD> <originalUrl>`.
pub fn not_found(method: &Method, url: &str) -> AppError {
    AppError::NotFound(format!("Cannot {method} {url}"))
}

pub async fn unknown_route(method: Method, OriginalUri(uri): OriginalUri) -> AppError {
    let url = uri.path_and_query().map(|p| p.as_str().to_string()).unwrap_or_else(|| uri.path().to_string());
    not_found(&method, &url)
}

/// Fills `{statusCode, timestamp, path, message}` for every HTTP error.
pub async fn nest_errors(req: Request, next: Next) -> Response {
    let method = req.method().clone();
    let url = full_url(&req);
    let res = next.run(req).await;

    if let Some(err) = res.extensions().get::<NestError>().cloned() {
        return json_error(err.status, &url, &err.message, res);
    }
    if res.status() == StatusCode::METHOD_NOT_ALLOWED {
        // Express has no 405: a known path with the wrong method is simply not found.
        let msg = ErrorMessage::Text(format!("Cannot {method} {url}"));
        return json_error(StatusCode::NOT_FOUND, &url, &msg, Response::new(axum::body::Body::empty()));
    }
    res
}

fn json_error(status: StatusCode, url: &str, message: &ErrorMessage, original: Response) -> Response {
    let body = nest_error_body(status, url, message);
    let mut out = (status, axum::Json(body)).into_response();
    // Keep headers the handler set deliberately (e.g. Retry-After), but not the old body's.
    for (name, value) in original.headers() {
        if name != header::CONTENT_TYPE && name != header::CONTENT_LENGTH {
            out.headers_mut().insert(name.clone(), value.clone());
        }
    }
    out.headers_mut().insert(header::CONTENT_TYPE, HeaderValue::from_static("application/json; charset=utf-8"));
    out
}
