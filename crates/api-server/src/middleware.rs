//! Cross-cutting middleware: Host check, audit logging and security headers.

use crate::state::AppState;
use axum::body::{to_bytes, Body};
use axum::extract::{Request, State};
use axum::http::header::{AUTHORIZATION, HOST};
use axum::http::{HeaderValue, Method, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

/// With LAN access off, only requests addressed to this machine's loopback name are served.
/// This defeats DNS-rebinding pages that try to reach the local server from a browser.
pub async fn host_guard(State(state): State<AppState>, req: Request, next: Next) -> Response {
    if state.lan_enabled {
        return next.run(req).await;
    }
    let host = req.headers().get(HOST).and_then(|h| h.to_str().ok()).unwrap_or("");
    let port = state.port;
    let allowed = [format!("127.0.0.1:{port}"), format!("localhost:{port}"), format!("[::1]:{port}")];
    if allowed.iter().any(|a| a.eq_ignore_ascii_case(host)) {
        next.run(req).await
    } else {
        tracing::warn!(host, "refused request with non-loopback Host header");
        (StatusCode::FORBIDDEN, "Host not allowed").into_response()
    }
}

/// Reference `AuditInterceptor`: every successful POST/PATCH/PUT/DELETE outside `/auth/` is
/// recorded with the JWT's user, a summarised action, the path without query string, the
/// response status, and a detail picked from the response body.
pub async fn audit_log(State(state): State<AppState>, req: Request, next: Next) -> Response {
    let method = req.method().clone();
    let path_and_query = req.uri().path_and_query().map(|p| p.as_str().to_string()).unwrap_or_default();
    let mutating = matches!(method, Method::POST | Method::PATCH | Method::PUT | Method::DELETE);
    if !mutating || !path_and_query.starts_with("/api/") || path_and_query.contains("/auth/") {
        return next.run(req).await;
    }
    let claims = req
        .headers()
        .get(AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split_whitespace().nth(1))
        .and_then(|t| state.jwt.verify(t));

    let res = next.run(req).await;
    if res.status().as_u16() >= 400 {
        return res;
    }
    let (parts, body) = res.into_parts();
    let bytes = match to_bytes(body, usize::MAX).await {
        Ok(b) => b,
        Err(e) => {
            tracing::error!("could not buffer response for audit: {e}");
            return (StatusCode::INTERNAL_SERVER_ERROR, Body::empty()).into_response();
        }
    };
    let detail = serde_json::from_slice(&bytes).ok().and_then(|v| audit::detail_from(&v));
    let path = path_and_query.split('?').next().unwrap_or_default().to_string();
    audit::record(
        &state.db,
        audit::NewEntry {
            user_id: claims.as_ref().map(|c| c.sub.clone()),
            username: Some(claims.as_ref().map(|c| c.username.clone()).unwrap_or_else(|| "unknown".into())),
            action: audit::summarize(method.as_str(), &path),
            method: Some(method.to_string()),
            path: Some(path),
            status_code: Some(parts.status.as_u16()),
            detail,
        },
    )
    .await;
    Response::from_parts(parts, Body::from(bytes))
}

pub fn security_headers() -> [(axum::http::HeaderName, HeaderValue); 3] {
    [
        (axum::http::header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff")),
        (axum::http::header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY")),
        (axum::http::header::REFERRER_POLICY, HeaderValue::from_static("no-referrer")),
    ]
}
