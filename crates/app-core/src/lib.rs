//! Shared foundations: configuration, the Nest-compatible error type, money, time, ids,
//! permission keys and DPAPI-protected secrets. Depends on no other workspace crate.

pub mod config;
pub mod error;
pub mod ids;
pub mod money;
pub mod permissions;
pub mod secrets;
pub mod time;

pub use error::{AppError, AppResult, ErrorMessage};
