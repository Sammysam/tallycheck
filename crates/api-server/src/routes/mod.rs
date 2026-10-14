//! `/api` routes implemented so far (W0, W1). Routes of later waves answer 404 like any unknown path.

pub mod accounts;
pub mod audit_logs;
pub mod auth;
pub mod ledger;
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
        .route("/accounts", get(accounts::list).post(accounts::create))
        .route("/accounts/seed", post(accounts::seed))
        .route("/accounts/{id}", get(accounts::find_one).patch(accounts::update).delete(accounts::remove))
        .route("/ledger/entries", get(ledger::list_entries).post(ledger::post_entry))
        .route("/ledger/entries/{id}", get(ledger::get_entry))
        .route("/ledger/entries/{id}/reverse", post(ledger::reverse))
        .route("/ledger/trial-balance", get(ledger::trial_balance))
        .route("/ledger/balance/{account_id}", get(ledger::account_balance))
        .fallback(unknown_route)
}
