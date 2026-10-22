//! D39/D40: connectors to outside services. The app stays offline: a connector reaches the internet
//! only while it syncs, and everything it brings in waits for a person (draft invoices, bank lines
//! to match). Each outside record comes in once (`import_items`), and every sync is an import that
//! can be undone from Import from File while nothing has used it.
//!
//! * Shopify — orders and Shopify Payments.
//! * WooCommerce — orders (payments arrive through the payment service the shop uses).
//! * PayPal, Stripe — payments, fees, refunds and payouts on their balance.

pub mod common;
pub mod paypal;
pub mod shopify;
pub mod stripe;
pub mod woocommerce;

use app_core::{AppError, AppResult};
use database::Db;
use serde_json::{Map, Value};
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Connector {
    Shopify,
    WooCommerce,
    PayPal,
    Stripe,
}

impl Connector {
    pub const ALL: [Connector; 4] = [Connector::Shopify, Connector::WooCommerce, Connector::PayPal, Connector::Stripe];

    pub fn parse(s: &str) -> Option<Connector> {
        Self::ALL.into_iter().find(|c| c.id() == s)
    }

    pub fn id(self) -> &'static str {
        match self {
            Connector::Shopify => "shopify",
            Connector::WooCommerce => "woocommerce",
            Connector::PayPal => "paypal",
            Connector::Stripe => "stripe",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Connector::Shopify => "Shopify",
            Connector::WooCommerce => "WooCommerce",
            Connector::PayPal => "PayPal",
            Connector::Stripe => "Stripe",
        }
    }

    pub fn brings_orders(self) -> bool {
        matches!(self, Connector::Shopify | Connector::WooCommerce)
    }

    pub fn brings_money(self) -> bool {
        !matches!(self, Connector::WooCommerce)
    }

    /// The asset account the connector's money sits in until it is paid out.
    pub fn clearing_account(self) -> Option<(&'static str, &'static str)> {
        match self {
            Connector::Shopify => Some(("1090", "Shopify Clearing")),
            Connector::PayPal => Some(("1091", "PayPal")),
            Connector::Stripe => Some(("1092", "Stripe")),
            Connector::WooCommerce => None,
        }
    }
}

/// Connects with the credentials typed on the screen, after checking them with the service.
pub async fn connect(db: &Db, data_dir: &Path, c: Connector, body: &Map<String, Value>) -> AppResult<Value> {
    let cfg = common::load_config(db, c).await?;
    let conn = match c {
        Connector::Shopify => shopify::connect(&cfg, body).await?,
        Connector::WooCommerce => woocommerce::connect(body).await?,
        Connector::PayPal => paypal::connect(body).await?,
        Connector::Stripe => stripe::connect(body).await?,
    };
    common::keep_connection(db, data_dir, c, conn).await
}

/// Fetches and writes; keeps the report (or the error) for the screen.
pub async fn sync(db: &Db, data_dir: &Path, c: Connector, created_by: Option<&str>) -> AppResult<Value> {
    let result = run_sync(db, data_dir, c, created_by).await;
    common::remember_outcome(db, c, &result).await?;
    result
}

async fn run_sync(db: &Db, data_dir: &Path, c: Connector, created_by: Option<&str>) -> AppResult<Value> {
    let cfg = common::load_config(db, c).await?;
    if cfg.remote_id.is_none() {
        return Err(AppError::bad_request(format!("Connect {} first", c.label())));
    }
    let secret = common::load_secret(data_dir, c)?;
    let since = common::since(&cfg);
    // Everything is fetched before anything is written.
    let fetched = match c {
        Connector::Shopify => shopify::fetch(&cfg, &secret, &since).await?,
        Connector::WooCommerce => woocommerce::fetch(&cfg, &secret, &since).await?,
        Connector::PayPal => paypal::fetch(&cfg, &secret, &since).await?,
        Connector::Stripe => stripe::fetch(&cfg, &secret, &since).await?,
    };
    common::apply(db, c, &cfg, fetched, &since, created_by).await
}

/// The background loop: each connector with automatic sync on, once an hour.
pub fn spawn(db: Db, data_dir: PathBuf) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(120)).await;
        loop {
            for c in Connector::ALL {
                let cfg = match common::load_config(&db, c).await {
                    Ok(cfg) => cfg,
                    Err(e) => {
                        tracing::error!("{} settings unreadable: {}", c.label(), e.user_message());
                        continue;
                    }
                };
                if !cfg.auto_sync || cfg.remote_id.is_none() || !common::connected(&data_dir, c) {
                    continue;
                }
                match sync(&db, &data_dir, c, None).await {
                    Ok(r) => {
                        tracing::info!(detail = %r["name"], "automatic {} sync", c.label());
                        audit::record(
                            &db,
                            audit::NewEntry {
                                user_id: None,
                                username: Some("system".into()),
                                action: format!("Synced with {} (automatic)", c.label()),
                                method: None,
                                path: None,
                                status_code: None,
                                detail: r["name"].as_str().map(str::to_string),
                            },
                        )
                        .await;
                    }
                    Err(e) => tracing::warn!("automatic {} sync failed: {}", c.label(), e.user_message()),
                }
            }
            tokio::time::sleep(Duration::from_secs(3600)).await;
        }
    })
}
