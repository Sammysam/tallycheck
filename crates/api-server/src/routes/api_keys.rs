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

/// D52: an Administrator sees and manages every key; anyone else only their own.
pub async fn list(State(s): State<AppState>, auth: Authorized<perm::Settings>) -> AppResult<Json<Vec<ApiKey>>> {
    let me = auth.user();
    Ok(Json(api_keys::list(&s.db).await?.into_iter().filter(|k| me.is_admin() || k.user_id == me.id).collect()))
}

/// Someone else's key is "not found" to a user who is not an Administrator.
async fn own_key(s: &AppState, me: &auth::User, id: &str) -> AppResult<()> {
    if me.is_admin() {
        return Ok(());
    }
    let mine = api_keys::list(&s.db).await?.into_iter().any(|k| k.id == id && k.user_id == me.id);
    if mine {
        Ok(())
    } else {
        Err(AppError::NotFound(format!("Key with ID {id} not found")))
    }
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
    auth: Authorized<perm::Settings>,
    Path(id): Path<String>,
    body: JsonBody,
) -> AppResult<Json<ApiKey>> {
    parse_uuid(&id)?;
    own_key(&s, auth.user(), &id).await?;
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
    auth: Authorized<perm::Settings>,
    Path(id): Path<String>,
) -> AppResult<Json<ApiKey>> {
    parse_uuid(&id)?;
    own_key(&s, auth.user(), &id).await?;
    Ok(Json(api_keys::revoke(&s.db, &id).await?))
}

/// Where the connector program is, and the address it should call, for the setup instructions.
pub async fn setup(State(s): State<AppState>, _auth: Authorized<perm::Settings>) -> AppResult<Json<Value>> {
    // `tallycheck-mcp.exe`; an installation from before it shipped has only `cenango-mcp.exe`.
    let dir = std::env::current_exe().ok().and_then(|p| p.parent().map(std::path::Path::to_path_buf));
    let exe = dir.map(|d| {
        let exe = std::env::consts::EXE_SUFFIX;
        let new = d.join(format!("tallycheck-mcp{exe}"));
        let old = d.join(format!("cenango-mcp{exe}"));
        if !new.exists() && old.exists() {
            old
        } else {
            new
        }
    });
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
