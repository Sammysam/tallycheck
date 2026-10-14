//! The HTTP API (reference NestJS controllers re-implemented in Axum) plus the static frontend.
//!
//! Request pipeline, outermost first:
//! host guard → security headers → tracing → Nest error shaping → audit → routes.
//! Inside a route: authentication (401) → permission (403) → body validation (400) → handler,
//! the same order as the reference's guards and `ValidationPipe`.

pub mod body;
pub mod errors;
pub mod extract;
pub mod middleware;
pub mod routes;
pub mod server;
pub mod state;
pub mod validate;

pub use server::{build_router, init_logging, start, ServerHandle};
pub use state::AppState;
