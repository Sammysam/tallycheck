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
    let this_pc = this_pc_name();
    // The PC's name in its own Windows domain (set on domain-joined PCs), e.g. `pc.office.lan`.
    let domain = std::env::var("USERDNSDOMAIN").unwrap_or_default().to_ascii_lowercase();
    this_pc_names(&this_pc, &domain).contains(&name) || extra.iter().any(|e| e.trim().eq_ignore_ascii_case(&name))
}

/// This computer's name: `COMPUTERNAME` on Windows; on macOS (D53) the host name without
/// `.local` (the `.local` form is added by `this_pc_names`).
fn this_pc_name() -> String {
    let name = std::env::var("COMPUTERNAME").ok().filter(|n| !n.is_empty()).or_else(host_name_of_this_computer);
    let name = name.unwrap_or_default().to_ascii_lowercase();
    name.strip_suffix(".local").map(str::to_string).unwrap_or(name)
}

#[cfg(unix)]
#[allow(unsafe_code)]
fn host_name_of_this_computer() -> Option<String> {
    let mut buf = [0u8; 256];
    // SAFETY: the buffer is valid for its whole length; gethostname writes at most that many bytes.
    let rc = unsafe { libc::gethostname(buf.as_mut_ptr().cast(), buf.len()) };
    if rc != 0 {
        return None;
    }
    let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    std::str::from_utf8(&buf[..end]).ok().map(str::to_string).filter(|n| !n.is_empty())
}

#[cfg(not(unix))]
fn host_name_of_this_computer() -> Option<String> {
    None
}

/// Exact names of this PC. Any other `<pc>.something` is refused: an attacker's domain such as
/// `<pc>.attacker.example` could otherwise be rebound to this PC's address (DNS rebinding).
fn this_pc_names(pc: &str, domain: &str) -> Vec<String> {
    if pc.is_empty() {
        return Vec::new();
    }
    let mut names = vec![pc.to_string(), format!("{pc}.local")];
    if !domain.is_empty() {
        names.push(format!("{pc}.{domain}"));
    }
    names
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
    let bearer = req
        .headers()
        .get(AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split_whitespace().nth(1))
        .map(str::to_string);
    let claims = bearer.as_deref().and_then(|t| state.jwt.verify(t));
    // D41: a request made with a key is logged as its user, "via" the key's name.
    let via_key = match bearer.as_deref() {
        Some(t) if claims.is_none() && auth::api_keys::looks_like_key(t) => {
            auth::api_keys::verify(&state.db, t).await.ok().flatten()
        }
        _ => None,
    };

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
            user_id: claims.as_ref().map(|c| c.sub.clone()).or_else(|| via_key.as_ref().map(|k| k.user_id.clone())),
            username: Some(match (&claims, &via_key) {
                (Some(c), _) => c.username.clone(),
                (None, Some(k)) => format!("{} via {}", k.username.clone().unwrap_or_default(), k.name),
                _ => "unknown".into(),
            }),
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

/// Content-Security-Policy: scripts only from the app itself (no inline scripts, no `eval`), so
/// text that slips into a page as HTML cannot run. Inline styles stay allowed (the printed
/// documents carry `<style>` blocks); pictures may be `data:` / `blob:` (logos, uploads, PDFs).
pub const CONTENT_SECURITY_POLICY: &str = "default-src 'self'; script-src 'self'; \
     style-src 'self' 'unsafe-inline'; img-src 'self' data: blob:; font-src 'self' data:; \
     connect-src 'self'; object-src 'self' blob:; base-uri 'none'; frame-ancestors 'none'; form-action 'self'";

pub fn security_headers() -> [(axum::http::HeaderName, HeaderValue); 4] {
    [
        (axum::http::header::CONTENT_SECURITY_POLICY, HeaderValue::from_static(CONTENT_SECURITY_POLICY)),
        (axum::http::header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff")),
        (axum::http::header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY")),
        (axum::http::header::REFERRER_POLICY, HeaderValue::from_static("no-referrer")),
    ]
}
