//! D39: connectors to outside services. The app stays offline: a connector reaches the internet
//! only while it syncs, and everything it brings in waits for a person (draft invoices, bank
//! lines to match). Each outside record comes in once (`import_items`), and every sync is an
//! import that can be undone from Import from File while nothing has used it.

pub mod shopify;
