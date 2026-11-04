//! Reference `users.controller.ts` — all routes need the `users` permission.
//!
//! D52: the `users` permission no longer makes its holder as strong as an Administrator. Someone
//! who is not an Admin may only give, or manage, rights they hold themselves: never the Admin
//! role, never an Admin, never a user or role with a right they lack. Admins are unchanged.

use crate::body::JsonBody;
use crate::extract::{perm, Authorized};
use crate::state::AppState;
use crate::validate;
use app_core::{AppError, AppResult};
use auth::users::{self, CreateUser, UpdateUser, UserView};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde_json::Value;

pub(crate) const BEYOND_OWN_RIGHTS: &str = "Only an administrator can give or manage rights you don't have yourself";

/// Whether `caller` holds every right in `permissions` (an Admin holds all).
pub(crate) fn within_rights(caller: &auth::User, permissions: &[String]) -> bool {
    caller.is_admin() || permissions.iter().all(|p| caller.has_permission(p))
}

/// A role may be given by `caller`: an Admin gives any; others only a non-Admin role whose
/// rights they hold themselves.
async fn may_give_role(state: &AppState, caller: &auth::User, role_id: Option<&str>) -> AppResult<()> {
    if caller.is_admin() {
        return Ok(());
    }
    let Some(role_id) = role_id else { return Ok(()) };
    let mut conn = state.db.read().acquire().await?;
    let role = auth::roles::find_by_id(&mut conn, role_id).await?;
    if role.is_admin() || !within_rights(caller, &role.permissions) {
        return Err(AppError::Forbidden(BEYOND_OWN_RIGHTS.into()));
    }
    Ok(())
}

/// `caller` may change, reset or switch off `target_id`: an Admin may (the reference's own
/// last-Admin rules still apply); others not an Admin, nor anyone with a right they lack.
async fn may_manage_user(state: &AppState, caller: &auth::User, target_id: &str) -> AppResult<()> {
    if caller.is_admin() {
        return Ok(());
    }
    // An unknown user stays the handler's own 404.
    let Some(target) = users::get_by_id(&state.db, target_id).await? else { return Ok(()) };
    let target_rights = target.role.as_ref().map(|r| r.permissions.clone()).unwrap_or_default();
    if target.is_admin() || !within_rights(caller, &target_rights) {
        return Err(AppError::Forbidden(BEYOND_OWN_RIGHTS.into()));
    }
    Ok(())
}

pub async fn list(State(state): State<AppState>, _auth: Authorized<perm::Users>) -> AppResult<Json<Vec<UserView>>> {
    Ok(Json(users::find_all(&state.db).await?.into_iter().map(UserView::from).collect()))
}

pub async fn create(
    State(state): State<AppState>,
    auth: Authorized<perm::Users>,
    body: JsonBody,
) -> AppResult<(StatusCode, Json<Value>)> {
    let dto = validate::create_user(body.object())?;
    may_give_role(&state, auth.user(), dto.role_id.as_deref()).await?;
    let user = users::create_user(
        &state.db,
        CreateUser { username: dto.username, password: dto.password, role_id: dto.role_id },
    )
    .await?;
    // The reference returns the freshly saved entity, whose eager `role` relation was never
    // loaded, so the key is absent (not null) in this one response.
    let mut view = serde_json::to_value(UserView::from(user)).map_err(AppError::internal)?;
    if let Some(obj) = view.as_object_mut() {
        obj.remove("role");
    }
    Ok((StatusCode::CREATED, Json(view)))
}

/// D36: each user's last successful sign-in (from the activity log), for the Users screen.
pub async fn last_sign_ins(
    State(state): State<AppState>,
    _auth: Authorized<perm::Users>,
) -> AppResult<Json<Vec<audit::LastSignIn>>> {
    Ok(Json(audit::last_sign_ins(&state.db).await?))
}

pub async fn update(
    State(state): State<AppState>,
    auth: Authorized<perm::Users>,
    Path(id): Path<String>,
    body: JsonBody,
) -> AppResult<Json<UserView>> {
    let dto = validate::update_user(body.object())?;
    may_manage_user(&state, auth.user(), &id).await?;
    may_give_role(&state, auth.user(), dto.role_id.as_deref()).await?;
    // D36: nobody deactivates themselves or drops their own Admin role. (The last active
    // Admin is protected by `update_user` itself, with the reference's message.)
    if id == auth.user().id {
        let all = users::find_all(&state.db).await?;
        if let Some(me) = all.iter().find(|u| u.id == id) {
            let roles = auth::roles::find_all(&state.db).await?;
            let stays_admin = match &dto.role_id {
                Some(r) => roles.iter().any(|role| &role.id == r && role.is_admin()),
                None => me.is_admin(),
            };
            let deactivating = dto.is_active == Some(false) && me.is_active;
            let demoting = me.is_admin() && !stays_admin;
            let last_admin = me.is_admin() && !all.iter().any(|u| u.id != me.id && u.is_active && u.is_admin());
            if (deactivating || demoting) && !last_admin {
                return Err(AppError::bad_request(
                    "You cannot deactivate your own account or remove your own Admin role. Ask another administrator.",
                ));
            }
        }
    }
    let user = users::update_user(
        &state.db,
        &id,
        UpdateUser { username: dto.username, role_id: dto.role_id, is_active: dto.is_active },
    )
    .await?;
    Ok(Json(user.into()))
}

/// POST without `@HttpCode`, so 201 like the reference.
pub async fn reset_password(
    State(state): State<AppState>,
    auth: Authorized<perm::Users>,
    Path(id): Path<String>,
    body: JsonBody,
) -> AppResult<(StatusCode, Json<UserView>)> {
    let password = validate::reset_password(body.object())?;
    may_manage_user(&state, auth.user(), &id).await?;
    let user = users::reset_password(&state.db, &id, &password).await?;
    Ok((StatusCode::CREATED, Json(user.into())))
}

pub async fn reset_two_factor(
    State(state): State<AppState>,
    auth: Authorized<perm::Users>,
    Path(id): Path<String>,
) -> AppResult<(StatusCode, Json<UserView>)> {
    may_manage_user(&state, auth.user(), &id).await?;
    let user = users::reset_two_factor(&state.db, &id).await?;
    Ok((StatusCode::CREATED, Json(user.into())))
}
