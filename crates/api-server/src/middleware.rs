//! Cross-cutting middleware: Host check, audit logging and security headers.

use crate::state::AppState;
use axum::body::{to_bytes, Body};
use axum::extract::{ConnectInfo, Request, State};
use axum::http::header::{AUTHORIZATION, HOST};
use axum::http::{HeaderValue, Method, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use std::net::SocketAddr;

/// Who may talk to the server.
///
/// * LAN off: only requests addressed to this machine's loopback name (defeats DNS-rebinding
///   pages that try to reach the local server from a browser).
/// * LAN on: clients must come from loopback or an allowed network (private ranges by
///   default), and the address-bar host must be an IP address, `localhost`, this PC's name or a
///   configured name — so rebinding through an attacker's domain is still refused.
pub async fn host_guard(State(state): State<AppState>, req: Request, next: Next) -> Response {
    let host = req.headers().get(HOST).and_then(|h| h.to_str().ok()).unwrap_or("").to_string();
    let peer = req.extensions().get::<ConnectInfo<SocketAddr>>().map(|c| c.0.ip());
    if !state.lan_enabled {
        let port = state.port;
        let allowed = [format!("127.0.0.1:{port}"), format!("localhost:{port}"), format!("[::1]:{port}")];
        if allowed.iter().any(|a| a.eq_ignore_ascii_case(&host)) {
            return next.run(req).await;
        }
        tracing::warn!(host, "refused request with non-loopback Host header");
        return (StatusCode::FORBIDDEN, "Host not allowed").into_response();
    }
    if let Some(ip) = peer.filter(|ip| !ip.is_loopback()) {
        if !state.lan_allowed.iter().any(|n| n.contains(ip)) {
            tracing::warn!(%ip, "refused LAN client outside the allowed networks");
            return (StatusCode::FORBIDDEN, "Address not allowed").into_response();
        }
    }
    if lan_host_allowed(&host, &state.lan_host_names) {
        next.run(req).await
    } else {
        tracing::warn!(host, "refused request with an unknown Host name");
        (StatusCode::FORBIDDEN, "Host not allowed").into_response()
    }
}

/// The host part of a `Host` header (without the port), lower-case.
fn host_name(host: &str) -> String {
    let h = host.trim();
    let name = if let Some(rest) = h.strip_prefix('[') {
        rest.split(']').next().unwrap_or("")
    } else {
        h.rsplit_once(':').map(|(n, _)| n).unwrap_or(h)
    };
    name.to_ascii_lowercase()
}

pub fn lan_host_allowed(host: &str, extra: &[String]) -> bool {
    let name = host_name(host);
    if name.is_empty() {
        return false;
    }
    if name == "localhost" || name.parse::<std::net::IpAddr>().is_ok() {
        return true;
    }
    let this_pc = std::env::var("COMPUTERNAME").unwrap_or_default().to_ascii_lowercase();
    (!this_pc.is_empty()
        && (name == this_pc || name == format!("{this_pc}.local") || name.starts_with(&format!("{this_pc}."))))
        || extra.iter().any(|e| e.trim().eq_ignore_ascii_case(&name))
}

/// Reference `AuditInterceptor`: every successful POST/PATCH/PUT/DELETE outside `/auth/` is
/// recorded with the JWT's user, a summarised action, the path without query string, the
/// response status, and a detail picked from the response body.
pub async fn audit_log(State(state): State<AppState>, req: Request, next: Next) -> Response {
    let method = req.method().clone();
    let path_and_query = req.uri().path_and_query().map(|p| p.as_str().to_string()).unwrap_or_default();
    let mutating = matches!(method, Method::POST | Method::PATCH | Method::PUT | Method::DELETE);
    // D37: reading a file and previewing an import change nothing.
    let read_only =
        path_and_query.starts_with("/api/imports/read") || path_and_query.starts_with("/api/imports/preview");
    if !mutating || !path_and_query.starts_with("/api/") || path_and_query.contains("/auth/") || read_only {
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
