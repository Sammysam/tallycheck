//! `/api` routes implemented so far (W0–W2). Routes of later waves answer 404 like any unknown path.

pub mod accounts;
pub mod audit_logs;
pub mod auth;
pub mod ledger;
pub mod operations;
pub mod roles;
pub mod settings;
pub mod users;

use crate::errors::unknown_route;
use crate::state::AppState;
use axum::routing::{get, patch, post};
use axum::Router;
use operations as op;

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
        .route("/customers", get(op::customers_list).post(op::customers_create))
        .route("/customers/{id}", get(op::customers_get).patch(op::customers_update).delete(op::customers_remove))
        .route("/vendors", get(op::vendors_list).post(op::vendors_create))
        .route("/vendors/{id}", get(op::vendors_get).patch(op::vendors_update).delete(op::vendors_remove))
        .route("/banking", get(op::banking_list).post(op::banking_create))
        .route("/banking/{id}", get(op::banking_get).patch(op::banking_update).delete(op::banking_deactivate))
        .route("/invoices", get(op::invoices_list).post(op::invoices_create))
        .route("/invoices/{id}", get(op::invoices_get).patch(op::invoices_update).delete(op::invoices_remove))
        .route("/invoices/{id}/duplicate", post(op::invoices_duplicate))
        .route("/invoices/{id}/status", patch(op::invoices_status))
        .route("/invoices/{id}/post", post(op::invoices_post))
        .route("/invoices/{id}/unpost", post(op::invoices_unpost))
        .route("/invoices/{id}/void", post(op::invoices_void))
        .route("/bills", get(op::bills_list).post(op::bills_create))
        .route("/bills/{id}", get(op::bills_get))
        .route("/bills/{id}/post", post(op::bills_post))
        .route("/bills/{id}/void", post(op::bills_void))
        .route("/expenses", get(op::expenses_list).post(op::expenses_create))
        .route("/expenses/{id}", get(op::expenses_get))
        .route("/expenses/{id}/post", post(op::expenses_post))
        .route("/expenses/{id}/void", post(op::expenses_void))
        .route("/payments", get(op::payments_list).post(op::payments_create))
        .route("/payments/{id}", get(op::payments_get))
        .fallback(unknown_route)
}
