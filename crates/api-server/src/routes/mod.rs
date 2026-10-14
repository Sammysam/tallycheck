//! `/api` routes implemented in W0. Routes of later waves answer 404 like any unknown path.

pub mod audit_logs;
pub mod auth;
pub mod roles;
pub mod settings;
pub mod users;

use crate::errors::unknown_route;
use crate::state::AppState;
use axum::routing::{get, patch, post};
use axum::Router;

pub fn api_router() -> Router<AppState> {
    Router::new()
        .route("/auth/login", post(auth::login))
        .route("/auth/2fa/setup", get(auth::setup_2fa))
        .route("/auth/2fa/enable", post(auth::enable_2fa))
        .route("/auth/2fa/verify", post(auth::verify_2fa))
        .route("/auth/me", get(auth::me))
        .route("/users", get(users::list).post(users::create))
        .route("/users/{id}", patch(users::update))
        .route("/users/{id}/reset-password", post(users::reset_password))
        .route("/users/{id}/reset-2fa", post(users::reset_two_factor))
        .route("/roles/permissions", get(roles::permission_catalog))
        .route("/roles", get(roles::list).post(roles::create))
        .route("/roles/{id}", patch(roles::update).delete(roles::remove))
        .route("/audit-logs/users", get(audit_logs::users))
        .route("/audit-logs", get(audit_logs::list))
        .route("/settings", get(settings::find_all).patch(settings::update))
        .fallback(unknown_route)
}
