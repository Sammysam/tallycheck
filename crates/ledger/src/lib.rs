//! The double-entry ledger (reference `modules/accounts` and `modules/ledger`).
//!
//! Every financial module posts through [`entries::post`] and corrects through
//! [`entries::reverse`], always inside the caller's write transaction, so a business document
//! and its journal entry commit or roll back together. Balances come only from journal lines.

pub mod accounts;
pub mod entries;

pub use accounts::{Account, AccountType};
pub use entries::{JournalEntry, JournalLine, NewEntry, NewLine};

#[cfg(test)]
mod tests;
