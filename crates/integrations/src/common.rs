//! D39/D40: what every connector shares: settings, the encrypted credentials, HTTP with retries,
//! and writing what was fetched — orders as DRAFT invoices, money movements as bank lines on the
//! connector's money (clearing) account — once each, inside one transaction, as an import that
//! can be undone.

use crate::Connector;
use app_core::money::Money;
use app_core::time::now_iso;
use app_core::{AppError, AppResult};
use database::{Db, Tx};
use operations::bank_lines::{self, NewBankLine};
use operations::imports::{self, Kind};
use operations::invoices::{self, CreateInvoice, LineInput};
use operations::parties::{self, PartyFields, PartyKind};
use reqwest::header::HeaderMap;
use reqwest::RequestBuilder;
use serde_json::{json, Value};
use sqlx::SqliteConnection;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Orders re-read before the last sync, to catch late payments and edits.
const OVERLAP_DAYS: u64 = 7;
const MAX_REPORT_ROWS: usize = 300;

// ─── What a connector fetches ───────────────────────────────────────────────

/// An order from a shop, in the shop's currency.
#[derive(Debug, Clone, Default)]
pub struct Order {
    /// The shop's own id (part of the no-duplicates key).
    pub id: String,
    /// What people call it: "#1001".
    pub number: String,
    /// YYYY-MM-DD.
    pub date: String,
    pub currency: String,
    pub customer: String,
    pub email: Option<String>,
    pub lines: Vec<OrderLine>,
    pub taxes_included: bool,
    /// The shop's own total, to report a difference.
    pub total: Option<Money>,
    /// Why it is left out (cancelled, failed…), if it is.
    pub skip: Option<String>,
    pub refunded: bool,
}

#[derive(Debug, Clone, Default)]
pub struct OrderLine {
    pub description: String,
    pub sku: Option<String>,
    /// Hundredths: 300 = 3.
    pub quantity: Money,
    pub unit_price: Money,
    pub discount: Money,
    /// "VAT|0.15" (several joined with "+"), None for no tax.
    pub tax_key: Option<String>,
    pub shipping: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MoneyKind {
    Payment,
    Fee,
    Refund,
    Payout,
    Other,
}

/// A movement on the connector's balance (PayPal, Stripe, Shopify Payments).
#[derive(Debug, Clone)]
pub struct MoneyLine {
    /// Unique within the connector ("bt:9001", "fee:9001").
    pub id: String,
    pub date: String,
    /// Signed: positive = money into the balance.
    pub amount: Money,
    pub currency: String,
    pub description: String,
    pub reference: Option<String>,
    pub kind: MoneyKind,
}

#[derive(Debug, Default)]
pub struct Fetched {
    pub orders: Vec<Order>,
    /// None when the connector has no balance, or it could not be read (see `notes`).
    pub money: Option<Vec<MoneyLine>>,
    pub notes: Vec<String>,
}

/// What a connector learns when it connects.
pub struct Connected {
    pub remote_id: String,
    pub remote_name: Option<String>,
    pub remote_currency: Option<String>,
    /// Kept encrypted on this PC.
    pub secret: Value,
    /// Connector settings to keep (environment, site address…).
    pub options: Vec<(&'static str, String)>,
}

// ─── Settings ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Default)]
pub struct Config {
    pub remote_id: Option<String>,
    pub remote_name: Option<String>,
    pub remote_currency: Option<String>,
    pub options: BTreeMap<String, String>,
    pub start_date: Option<String>,
    pub clearing_account_id: Option<String>,
    pub payout_account_id: Option<String>,
    pub fee_account_id: Option<String>,
    pub income_account_id: Option<String>,
    pub shipping_account_id: Option<String>,
    pub tax_map: BTreeMap<String, String>,
    pub auto_sync: bool,
    pub last_sync_at: Option<String>,
    pub last_result: Value,
    pub last_error: Option<String>,
}

impl Config {
    pub fn option(&self, k: &str) -> Option<&str> {
        self.options.get(k).map(String::as_str).filter(|v| !v.is_empty())
    }
}

/// Connector settings other than the common ones.
pub const OPTION_KEYS: &[&str] = &["apiVersion", "siteUrl", "environment"];

fn prefix(c: Connector) -> String {
    format!("integration.{}.", c.id())
}

pub async fn load_config(db: &Db, c: Connector) -> AppResult<Config> {
    let all = database::settings::find_all(db).await?;
    let p = prefix(c);
    let get = |k: &str| all.get(&format!("{p}{k}")).map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    Ok(Config {
        remote_id: get("remoteId"),
        remote_name: get("remoteName"),
        remote_currency: get("remoteCurrency"),
        options: OPTION_KEYS.iter().filter_map(|k| get(k).map(|v| (k.to_string(), v))).collect(),
        start_date: get("startDate"),
        clearing_account_id: get("clearingAccountId"),
        payout_account_id: get("payoutAccountId"),
        fee_account_id: get("feeAccountId"),
        income_account_id: get("incomeAccountId"),
        shipping_account_id: get("shippingAccountId"),
        tax_map: get("taxMap").and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default(),
        auto_sync: get("autoSync").as_deref() == Some("true"),
        last_sync_at: get("lastSyncAt"),
        last_result: get("lastResult").and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(Value::Null),
        last_error: get("lastError"),
    })
}

/// Saves settings (`None` clears the key).
pub async fn save(db: &Db, c: Connector, values: &[(&str, Option<String>)]) -> AppResult<()> {
    let p = prefix(c);
    let mut tx = db.begin().await?;
    for (k, v) in values {
        database::settings::upsert(&mut tx, &format!("{p}{k}"), v.as_deref().unwrap_or("")).await?;
    }
    tx.commit().await?;
    Ok(())
}

pub fn status_json(c: Connector, cfg: &Config, connected: bool) -> Value {
    json!({
        "id": c.id(),
        "label": c.label(),
        "orders": c.brings_orders(),
        "money": c.brings_money(),
        "connected": connected,
        "remoteId": cfg.remote_id,
        "remoteName": cfg.remote_name,
        "remoteCurrency": cfg.remote_currency,
        "options": cfg.options,
        "startDate": cfg.start_date,
        "clearingAccountId": cfg.clearing_account_id,
        "payoutAccountId": cfg.payout_account_id,
        "feeAccountId": cfg.fee_account_id,
        "incomeAccountId": cfg.income_account_id,
        "shippingAccountId": cfg.shipping_account_id,
        "taxMap": cfg.tax_map,
        "autoSync": cfg.auto_sync,
        "lastSyncAt": cfg.last_sync_at,
        "lastResult": cfg.last_result,
        "lastError": cfg.last_error,
    })
}

// ─── Credentials (DPAPI, this Windows user) ─────────────────────────────────

pub fn secret_path(data_dir: &Path, c: Connector) -> PathBuf {
    data_dir.join("secrets").join(format!("{}-credentials.dpapi", c.id()))
}

pub fn connected(data_dir: &Path, c: Connector) -> bool {
    secret_path(data_dir, c).exists()
}

pub fn store_secret(data_dir: &Path, c: Connector, secret: &Value) -> AppResult<()> {
    app_core::secrets::store(&secret_path(data_dir, c), secret.to_string().as_bytes())
        .map_err(|e| AppError::internal(format!("could not keep the {} credentials: {e}", c.label())))
}

pub fn load_secret(data_dir: &Path, c: Connector) -> AppResult<Value> {
    let bytes = app_core::secrets::load(&secret_path(data_dir, c))
        .map_err(|e| AppError::internal(e.to_string()))?
        .ok_or_else(|| AppError::bad_request(format!("Connect {} first", c.label())))?;
    serde_json::from_slice(&bytes).map_err(AppError::internal)
}

pub fn remove_secret(data_dir: &Path, c: Connector) -> AppResult<()> {
    app_core::secrets::remove(&secret_path(data_dir, c)).map_err(|e| AppError::internal(e.to_string()))
}

// ─── HTTP ───────────────────────────────────────────────────────────────────

/// The service's address, or `ACCOUNTING_<ID>_BASE_URL` when it names a local stand-in (tests).
pub fn base_url(c: Connector, real: String) -> String {
    match std::env::var(format!("ACCOUNTING_{}_BASE_URL", c.id().to_ascii_uppercase())) {
        Ok(b) if b.starts_with("http://127.0.0.1:") || b.starts_with("http://localhost:") => {
            b.trim_end_matches('/').to_string()
        }
        _ => real,
    }
}

pub fn http() -> AppResult<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(60))
        .user_agent("CenangoAccounting")
        .build()
        .map_err(AppError::internal)
}

pub fn unreachable(service: &str, e: impl std::fmt::Display) -> AppError {
    AppError::bad_request(format!("Could not reach {service}: {e}. Check the internet connection and try again."))
}

/// Sends a request, waiting and retrying when the service says it is busy (429). Returns the
/// status, headers and JSON body (Null when the body is not JSON).
pub async fn send(service: &str, req: RequestBuilder) -> AppResult<(u16, HeaderMap, Value)> {
    for attempt in 0..5 {
        let r = req.try_clone().ok_or_else(|| AppError::internal("request cannot be repeated"))?;
        let res = r.send().await.map_err(|e| unreachable(service, e))?;
        let status = res.status().as_u16();
        if status == 429 && attempt < 4 {
            let wait = res
                .headers()
                .get("Retry-After")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.split('.').next().and_then(|w| w.trim().parse::<u64>().ok()))
                .unwrap_or(2);
            tokio::time::sleep(Duration::from_secs(wait.clamp(1, 10))).await;
            continue;
        }
        let headers = res.headers().clone();
        let body = res.text().await.map_err(|e| unreachable(service, e))?;
        return Ok((status, headers, serde_json::from_str(&body).unwrap_or(Value::Null)));
    }
    Err(AppError::bad_request(format!("{service} is busy (too many requests). Try again in a minute.")))
}

/// A refusal of the credentials, in plain words.
pub fn refused(service: &str, what: &str) -> AppError {
    AppError::bad_request(format!("{service} refused the {what}. Check it, and what it is allowed to read."))
}

pub fn money(v: &Value) -> Option<Money> {
    match v {
        Value::String(s) => Money::parse_decimal(s.trim()),
        Value::Number(_) => Money::from_json(v),
        _ => None,
    }
}

pub fn text<'a>(v: &'a Value, k: &str) -> &'a str {
    v[k].as_str().map(str::trim).unwrap_or("")
}

/// "VAT" and 0.15 → "VAT|0.15".
pub fn tax_part(title: &str, rate: f64) -> String {
    format!("{}|{}", title.trim(), rate)
}

// ─── Connect / disconnect ───────────────────────────────────────────────────

pub async fn connected_status(db: &Db, data_dir: &Path, c: Connector) -> AppResult<Value> {
    Ok(status_json(c, &load_config(db, c).await?, connected(data_dir, c)))
}

pub async fn keep_connection(db: &Db, data_dir: &Path, c: Connector, conn: Connected) -> AppResult<Value> {
    store_secret(data_dir, c, &conn.secret)?;
    let cfg = load_config(db, c).await?;
    let start = cfg.start_date.clone().or_else(|| Some(app_core::time::today_local().format("%Y-%m-01").to_string()));
    let mut values: Vec<(&str, Option<String>)> = vec![
        ("remoteId", Some(conn.remote_id)),
        ("remoteName", conn.remote_name),
        ("remoteCurrency", conn.remote_currency),
        ("startDate", start),
        ("lastError", None),
    ];
    values.extend(conn.options.into_iter().map(|(k, v)| (k, Some(v))));
    save(db, c, &values).await?;
    connected_status(db, data_dir, c).await
}

pub async fn disconnect(db: &Db, data_dir: &Path, c: Connector) -> AppResult<Value> {
    remove_secret(data_dir, c)?;
    save(db, c, &[("remoteId", None), ("autoSync", Some("false".into()))]).await?;
    connected_status(db, data_dir, c).await
}

/// Adds the connector's money account ("1090 Shopify Clearing", "1091 PayPal", "1092 Stripe")
/// unless an account of that name exists, and chooses it.
pub async fn ensure_clearing_account(db: &Db, c: Connector) -> AppResult<String> {
    let (code, name) =
        c.clearing_account().ok_or_else(|| AppError::bad_request(format!("{} has no money account", c.label())))?;
    let mut tx = db.begin().await?;
    let found: Option<String> =
        sqlx::query_scalar(r#"SELECT "id" FROM "accounts" WHERE lower("name") = lower(?1) LIMIT 1"#)
            .bind(name)
            .fetch_optional(&mut *tx)
            .await?;
    let id = match found {
        Some(id) => id,
        None => {
            if ledger::accounts::find_by_code(&mut tx, code).await.is_ok() {
                return Err(AppError::bad_request(format!(
                    "Account code {code} is taken. Add a \"{name}\" asset account on the Chart of Accounts and choose it."
                )));
            }
            ledger::accounts::insert_chart(&mut tx, &[(code, name, ledger::AccountType::Asset, "current_asset")])
                .await?;
            ledger::accounts::find_by_code(&mut tx, code).await?.id
        }
    };
    tx.commit().await?;
    save(db, c, &[("clearingAccountId", Some(id.clone()))]).await?;
    Ok(id)
}

// ─── Writing what was fetched ───────────────────────────────────────────────

fn row(kind: &str, reference: &str, status: &str, message: impl Into<String>) -> Value {
    json!({ "kind": kind, "ref": reference, "status": status, "message": message.into() })
}

async fn savepoint(tx: &mut Tx, sql: &str) -> AppResult<()> {
    sqlx::query(sql).execute(&mut **tx).await?;
    Ok(())
}

async fn find_id(conn: &mut SqliteConnection, sql: &str, value: &str) -> AppResult<Option<String>> {
    Ok(sqlx::query_scalar(sql).bind(value).fetch_optional(conn).await?)
}

async fn home_currency(conn: &mut SqliteConnection) -> AppResult<String> {
    let c: Option<String> = sqlx::query_scalar(r#"SELECT "value" FROM "settings" WHERE "key" = 'region.currency'"#)
        .fetch_optional(conn)
        .await?;
    Ok(c.filter(|c| !c.is_empty()).unwrap_or_else(|| "LKR".into()))
}

fn tax_label(key: &str) -> String {
    key.replace('|', " ")
}

struct Ctx<'a> {
    c: Connector,
    cfg: &'a Config,
    home: String,
    created_by: Option<&'a str>,
    unmapped: BTreeSet<String>,
}

/// One order as a draft invoice: Ok(Some(message)) created, Ok(None) already there.
async fn order_to_invoice(tx: &mut Tx, ctx: &mut Ctx<'_>, import_id: &str, order: &Order) -> AppResult<Option<String>> {
    let key = format!("{}:{}:order:{}", ctx.c.id(), ctx.cfg.remote_id.clone().unwrap_or_default(), order.id);
    if imports::imported_as(tx, Kind::Invoices, &key).await?.is_some() {
        return Ok(None);
    }
    if !order.currency.is_empty() && order.currency != ctx.home {
        return Err(AppError::bad_request(format!(
            "The order is in {} and the company's currency is {}; such orders are not brought in yet",
            order.currency, ctx.home
        )));
    }
    let mut lines = Vec::new();
    let mut missing = BTreeSet::new();
    for l in &order.lines {
        let tax_code_id = match &l.tax_key {
            None => None,
            Some(k) => match ctx.cfg.tax_map.get(k) {
                Some(code) => Some(code.clone()),
                None => {
                    ctx.unmapped.insert(k.clone());
                    missing.insert(k.clone());
                    None
                }
            },
        };
        let item_id = match l.sku.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            Some(sku) if !l.shipping => {
                find_id(tx, r#"SELECT "id" FROM "items" WHERE lower(trim("sku")) = lower(?1) AND "isActive" = 1"#, sku)
                    .await?
            }
            _ => None,
        };
        let account_id = if item_id.is_some() {
            None
        } else if l.shipping {
            ctx.cfg.shipping_account_id.clone().or_else(|| ctx.cfg.income_account_id.clone())
        } else {
            ctx.cfg.income_account_id.clone()
        };
        lines.push(LineInput {
            description: l.description.clone(),
            quantity: Some(l.quantity),
            unit_price: Some(l.unit_price),
            account_id,
            tax_code_id,
            item_id,
            discount: l.discount,
        });
    }
    if !missing.is_empty() {
        return Err(AppError::bad_request(format!(
            "Match the {} tax {} to a tax code under Settings → Integrations, then sync again",
            ctx.c.label(),
            missing.iter().map(|k| format!("\"{}\"", tax_label(k))).collect::<Vec<_>>().join(", ")
        )));
    }
    if lines.is_empty() {
        return Err(AppError::bad_request("The order has no lines"));
    }
    let existing = find_id(
        tx,
        r#"SELECT "id" FROM "customers" WHERE lower(trim("name")) = lower(trim(?1)) ORDER BY "isActive" DESC LIMIT 1"#,
        &order.customer,
    )
    .await?;
    let customer_id = match existing {
        Some(c) => c,
        None => {
            let p = parties::create_in(
                tx,
                PartyKind::Customer,
                PartyFields {
                    name: Some(order.customer.clone()),
                    email: Some(order.email.clone()),
                    phone: None,
                    address: None,
                    tax_id: None,
                },
            )
            .await?;
            imports::remember_item(tx, import_id, Kind::Invoices, &format!("{key}:customer"), "CUSTOMER", &p.id)
                .await?;
            p.id
        }
    };
    let inv = invoices::create_in(
        tx,
        CreateInvoice {
            customer_id: Some(customer_id),
            date: order.date.clone(),
            po_number: (!order.number.is_empty()).then(|| order.number.clone()),
            notes: Some(format!("{} order {}", ctx.c.label(), order.number)),
            lines,
            prices_include_tax: order.taxes_included,
            ..Default::default()
        },
        ctx.created_by,
    )
    .await?;
    imports::remember_item(tx, import_id, Kind::Invoices, &key, "INVOICE", &inv.id).await?;
    let mut msg = format!("Draft {} for {}", inv.invoice_number, order.customer);
    if let Some(total) = order.total {
        if total != inv.total_amount {
            let diff = inv.total_amount.checked_sub(total).unwrap_or(Money::ZERO);
            msg.push_str(&format!(
                " — differs from {}'s total {} by {} (tax rounding)",
                ctx.c.label(),
                total.to_db_string(),
                diff.to_db_string()
            ));
        }
    }
    if order.refunded {
        msg.push_str(&format!(" — refunded in {}: issue a credit note", ctx.c.label()));
    }
    Ok(Some(msg))
}

/// The date orders are read from: the start date, or a week before the last sync.
pub fn since(cfg: &Config) -> String {
    let start = cfg.start_date.clone().unwrap_or_else(|| app_core::time::today_local().format("%Y-%m-01").to_string());
    match cfg
        .last_sync_at
        .as_deref()
        .and_then(|d| chrono::NaiveDate::parse_from_str(d.get(..10).unwrap_or(d), "%Y-%m-%d").ok())
    {
        Some(last) => {
            let back =
                last.checked_sub_days(chrono::Days::new(OVERLAP_DAYS)).unwrap_or(last).format("%Y-%m-%d").to_string();
            if back > start {
                back
            } else {
                start
            }
        }
        None => start,
    }
}

/// Writes what a connector fetched; returns the report.
pub async fn apply(
    db: &Db,
    c: Connector,
    cfg: &Config,
    fetched: Fetched,
    since: &str,
    created_by: Option<&str>,
) -> AppResult<Value> {
    let mut tx = db.begin().await?;
    let home = home_currency(&mut tx).await?;
    let source =
        format!("{} ({})", c.label(), cfg.remote_name.clone().or_else(|| cfg.remote_id.clone()).unwrap_or_default());
    let mut ctx = Ctx { c, cfg, home, created_by, unmapped: BTreeSet::new() };
    let mut rows = Vec::new();
    let mut notes = fetched.notes;
    let start = cfg.start_date.clone().unwrap_or_else(|| since.to_string());

    let (mut o_new, mut o_skip, mut o_err) = (0i64, 0i64, 0i64);
    if c.brings_orders() {
        let import = imports::open_import(&mut tx, Kind::Invoices, &source, None, None, created_by).await?;
        for order in &fetched.orders {
            if let Some(why) = &order.skip {
                o_skip += 1;
                rows.push(row("ORDER", &order.number, "SKIPPED", why.clone()));
                continue;
            }
            savepoint(&mut tx, "SAVEPOINT connector_order").await?;
            match order_to_invoice(&mut tx, &mut ctx, &import, order).await {
                Ok(Some(msg)) => {
                    savepoint(&mut tx, "RELEASE connector_order").await?;
                    o_new += 1;
                    rows.push(row("ORDER", &order.number, "NEW", msg));
                }
                Ok(None) => {
                    savepoint(&mut tx, "RELEASE connector_order").await?;
                    o_skip += 1;
                }
                Err(e) => {
                    savepoint(&mut tx, "ROLLBACK TO connector_order").await?;
                    savepoint(&mut tx, "RELEASE connector_order").await?;
                    if let AppError::Internal(d) = &e {
                        tracing::error!("{} order {}: {d}", c.id(), order.number);
                    }
                    o_err += 1;
                    rows.push(row("ORDER", &order.number, "ERROR", e.user_message()));
                }
            }
        }
        imports::close_import(&mut tx, &import, o_new, o_skip).await?;
    }

    let (mut l_new, mut l_skip, mut l_other) = (0i64, 0i64, 0i64);
    if c.brings_money() {
        match (&cfg.clearing_account_id, fetched.money) {
            (None, _) => {
                notes.push(format!("Choose the {} money account to bring in payments, fees and payouts.", c.label()))
            }
            (Some(_), None) => {}
            (Some(clearing), Some(list)) => {
                let account = ledger::accounts::find_one(&mut tx, clearing).await?;
                let account_currency = account.currency.clone().unwrap_or_else(|| ctx.home.clone());
                let import =
                    imports::open_import(&mut tx, Kind::Bank, &source, None, Some(clearing), created_by).await?;
                let remote = cfg.remote_id.clone().unwrap_or_default();
                for m in list {
                    if m.date.as_str() < start.as_str() || m.amount == Money::ZERO {
                        continue;
                    }
                    if !m.currency.is_empty() && m.currency != account_currency {
                        l_other += 1;
                        continue;
                    }
                    let key = format!("{}:{remote}:{}", c.id(), m.id);
                    if imports::imported_as(&mut tx, Kind::Bank, &key).await?.is_some() {
                        l_skip += 1;
                        continue;
                    }
                    let hint = match m.kind {
                        MoneyKind::Fee if m.amount < Money::ZERO => {
                            cfg.fee_account_id.as_deref().map(|a| ("EXPENSE", a))
                        }
                        MoneyKind::Payout => cfg.payout_account_id.as_deref().map(|a| ("TRANSFER", a)),
                        _ => None,
                    };
                    let line_id = bank_lines::insert(
                        &mut tx,
                        NewBankLine {
                            import_id: &import,
                            account_id: clearing,
                            date: &m.date,
                            description: &m.description,
                            reference: m.reference.as_deref(),
                            amount: m.amount,
                            hint,
                        },
                    )
                    .await?;
                    imports::remember_item(&mut tx, &import, Kind::Bank, &key, "BANK_LINE", &line_id).await?;
                    l_new += 1;
                }
                imports::close_import(&mut tx, &import, l_new, l_skip).await?;
                if l_other > 0 {
                    notes.push(format!(
                        "{l_other} movement(s) in another currency than {account_currency} were left out; give that currency its own account."
                    ));
                }
            }
        }
    }
    tx.commit().await?;

    if !ctx.unmapped.is_empty() {
        notes.push(format!("Some {} taxes are not matched to a tax code yet — see the list below.", c.label()));
    }
    rows.truncate(MAX_REPORT_ROWS);
    Ok(json!({
        "at": now_iso(),
        "since": since,
        "orders": { "created": o_new, "skipped": o_skip, "errors": o_err },
        "lines": { "created": l_new, "skipped": l_skip },
        "unmappedTaxes": ctx.unmapped.iter().collect::<Vec<_>>(),
        "notes": notes,
        "rows": rows,
        "name": format!("{}: {o_new} orders, {l_new} money lines", c.label()),
    }))
}

/// Keeps the outcome of a sync (or its error) for the screen.
pub async fn remember_outcome(db: &Db, c: Connector, result: &AppResult<Value>) -> AppResult<()> {
    let now = now_iso();
    match result {
        Ok(report) => {
            save(db, c, &[("lastSyncAt", Some(now)), ("lastResult", Some(report.to_string())), ("lastError", None)])
                .await
        }
        Err(e) => save(db, c, &[("lastError", Some(format!("{now} {}", e.user_message())))]).await,
    }
}
