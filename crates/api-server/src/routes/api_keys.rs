//! D41: keys for Claude (MCP). Managing keys needs `settings`; a key cannot manage keys.

use crate::body::JsonBody;
use crate::extract::{parse_uuid, perm, Authorized, CurrentUser};
use crate::state::AppState;
use app_core::{AppError, AppResult};
use auth::api_keys::{self, ApiKey};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde_json::{json, Value};

pub async fn list(State(s): State<AppState>, _auth: Authorized<perm::Settings>) -> AppResult<Json<Vec<ApiKey>>> {
    Ok(Json(api_keys::list(&s.db).await?))
}

/// Creates a key acting as the signed-in user. The key text is in this answer only.
pub async fn create(
    State(s): State<AppState>,
    auth: Authorized<perm::Settings>,
    body: JsonBody,
) -> AppResult<(StatusCode, Json<Value>)> {
    let m = body.object();
    let name = m.get("name").and_then(Value::as_str).unwrap_or("");
    let can_write = match m.get("canWrite") {
        None => false,
        Some(Value::Bool(b)) => *b,
        Some(_) => return Err(AppError::bad_request("canWrite must be a boolean value")),
    };
    let (key, text) = api_keys::create(&s.db, &auth.user().id, name, can_write).await?;
    let mut out = serde_json::to_value(&key).map_err(AppError::internal)?;
    out["key"] = json!(text);
    Ok((StatusCode::CREATED, Json(out)))
}

pub async fn update(
    State(s): State<AppState>,
    _auth: Authorized<perm::Settings>,
    Path(id): Path<String>,
    body: JsonBody,
) -> AppResult<Json<ApiKey>> {
    parse_uuid(&id)?;
    let m = body.object();
    let can_write = match m.get("canWrite") {
        None => None,
        Some(Value::Bool(b)) => Some(*b),
        Some(_) => return Err(AppError::bad_request("canWrite must be a boolean value")),
    };
    Ok(Json(api_keys::update(&s.db, &id, m.get("name").and_then(Value::as_str), can_write).await?))
}

pub async fn revoke(
    State(s): State<AppState>,
    _auth: Authorized<perm::Settings>,
    Path(id): Path<String>,
) -> AppResult<Json<ApiKey>> {
    parse_uuid(&id)?;
    Ok(Json(api_keys::revoke(&s.db, &id).await?))
}

/// Where the connector program is, and the address it should call, for the setup instructions.
pub async fn setup(State(s): State<AppState>, _auth: Authorized<perm::Settings>) -> AppResult<Json<Value>> {
    let exe = std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.join("cenango-mcp.exe")));
    Ok(Json(json!({
        "mcpPath": exe.as_ref().map(|p| p.display().to_string()),
        "mcpFound": exe.as_ref().is_some_and(|p| p.exists()),
        "apiUrl": format!("http://127.0.0.1:{}", s.port),
    })))
}

/// For the MCP connector: which key this is, whose, and what it may do.
pub async fn me(user: CurrentUser) -> AppResult<Json<Value>> {
    let key = user.via_key.ok_or_else(|| AppError::bad_request("This is for requests made with a key"))?;
    let permissions = user.user.role.as_ref().map(|r| r.permissions.clone()).unwrap_or_default();
    Ok(Json(json!({
        "name": key.name,
        "canWrite": key.can_write,
        "user": user.user.username,
        "role": user.user.role.as_ref().map(|r| r.name.clone()),
        "permissions": permissions,
    })))
}
