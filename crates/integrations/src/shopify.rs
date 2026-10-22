//! D39: the Shopify connector (integrations plan, phase 3).
//!
//! Connect with the store address and an Admin API access key from a Shopify custom app (read
//! orders, read Shopify Payments). The key is stored with DPAPI in the data folder, never in
//! the database and never sent back to the screen. A sync:
//!
//! * **Orders** → DRAFT invoices (one per order, its number as the PO number): each product line
//!   with its discount and tax, shipping as a line, products matched to Products & Services by
//!   SKU, the customer matched by name or created. Cancelled orders are skipped. A Shopify tax
//!   must be matched to one of the app's tax codes first; until then the order is reported and
//!   left out, so totals never silently differ.
//! * **Shopify Payments balance** → bank lines on a clearing account: each payment (money in),
//!   its fee (money out, suggested as an expense on the fee account), refunds, and payouts
//!   (money out, suggested as a transfer to the bank account the payouts reach).
//!
//! Nothing is posted. Invoices are checked and posted on the Invoices screen; the clearing
//! account's lines are matched on Bank Transactions (a payment matches its posted invoice).
//! Refunds still need a credit note, which the sync report points out.

use app_core::money::Money;
use app_core::time::now_iso;
use app_core::{AppError, AppResult};
use database::{Db, Tx};
use operations::bank_lines::{self, NewBankLine};
use operations::imports::{self, Kind};
use operations::invoices::{self, CreateInvoice, LineInput};
use operations::parties::{self, PartyFields, PartyKind};
use serde_json::{json, Value};
use sqlx::SqliteConnection;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const DEFAULT_API_VERSION: &str = "2026-07";
const PREFIX: &str = "integration.shopify.";
const TOKEN_FILE: &str = "shopify-token.dpapi";
/// Orders re-read before the last sync, to catch late payments and edits.
const OVERLAP_DAYS: u64 = 7;
const MAX_REPORT_ROWS: usize = 300;

/// Where the access key is kept (DPAPI, this Windows user).
pub fn token_path(data_dir: &Path) -> PathBuf {
    data_dir.join("secrets").join(TOKEN_FILE)
}

#[derive(Debug, Clone, Default)]
pub struct Config {
    pub shop: Option<String>,
    pub shop_name: Option<String>,
    pub shop_currency: Option<String>,
    pub api_version: String,
    pub start_date: Option<String>,
    pub clearing_account_id: Option<String>,
    pub payout_account_id: Option<String>,
    pub fee_account_id: Option<String>,
    pub income_account_id: Option<String>,
    pub shipping_account_id: Option<String>,
    /// Shopify tax ("VAT|0.15", "GST|0.05+PST|0.07") → tax code id.
    pub tax_map: BTreeMap<String, String>,
    pub auto_sync: bool,
    pub last_sync_at: Option<String>,
    pub last_result: Value,
    pub last_error: Option<String>,
}

fn non_empty(v: Option<&String>) -> Option<String> {
    v.map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

pub async fn load_config(db: &Db) -> AppResult<Config> {
    let all = database::settings::find_all(db).await?;
    let get = |k: &str| non_empty(all.get(&format!("{PREFIX}{k}")));
    Ok(Config {
        shop: get("shop"),
        shop_name: get("shopName"),
        shop_currency: get("shopCurrency"),
        api_version: get("apiVersion").unwrap_or_else(|| DEFAULT_API_VERSION.into()),
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
pub async fn save(db: &Db, values: &[(&str, Option<String>)]) -> AppResult<()> {
    let mut tx = db.begin().await?;
    for (k, v) in values {
        database::settings::upsert(&mut tx, &format!("{PREFIX}{k}"), v.as_deref().unwrap_or("")).await?;
    }
    tx.commit().await?;
    Ok(())
}

pub fn status_json(cfg: &Config, connected: bool) -> Value {
    json!({
        "connected": connected,
        "shop": cfg.shop,
        "shopName": cfg.shop_name,
        "shopCurrency": cfg.shop_currency,
        "apiVersion": cfg.api_version,
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

/// "mystore", "mystore.myshopify.com" or "https://mystore.myshopify.com/admin" →
/// "mystore.myshopify.com".
pub fn normalize_shop(input: &str) -> AppResult<String> {
    let s = input.trim().to_ascii_lowercase();
    let s = s.strip_prefix("https://").or_else(|| s.strip_prefix("http://")).unwrap_or(&s);
    let host = s.split('/').next().unwrap_or("");
    let handle = host.strip_suffix(".myshopify.com").unwrap_or(host);
    if handle.is_empty() || handle.len() > 60 || !handle.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
        return Err(AppError::bad_request(
            "Enter the store's address as it appears in Shopify, e.g. mystore.myshopify.com",
        ));
    }
    Ok(format!("{handle}.myshopify.com"))
}

// ─── The Shopify Admin API ──────────────────────────────────────────────────

pub struct Client {
    base: String,
    token: String,
    version: String,
    http: reqwest::Client,
}

fn unreachable(e: impl std::fmt::Display) -> AppError {
    AppError::bad_request(format!("Could not reach Shopify: {e}. Check the internet connection and try again."))
}

/// The `rel="next"` page of a `Link` header.
fn next_link(header: Option<&str>) -> Option<String> {
    header?.split(',').find_map(|part| {
        let part = part.trim();
        if !part.contains("rel=\"next\"") {
            return None;
        }
        let start = part.find('<')? + 1;
        let end = part.find('>')?;
        Some(part[start..end].to_string())
    })
}

impl Client {
    /// `ACCOUNTING_SHOPIFY_BASE_URL` (a local test server only) replaces https://<shop>.
    pub fn new(shop: &str, token: &str, version: &str) -> AppResult<Client> {
        let base = match std::env::var("ACCOUNTING_SHOPIFY_BASE_URL") {
            Ok(b) if b.starts_with("http://127.0.0.1:") || b.starts_with("http://localhost:") => {
                b.trim_end_matches('/').to_string()
            }
            _ => format!("https://{shop}"),
        };
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(60))
            .user_agent("CenangoAccounting")
            .build()
            .map_err(AppError::internal)?;
        Ok(Client { base, token: token.to_string(), version: version.to_string(), http })
    }

    fn url(&self, path: &str) -> String {
        format!("{}/admin/api/{}/{}", self.base, self.version, path)
    }

    /// One page: the JSON and the next page's address. 404 / 403 are `Ok(None)` where the caller
    /// treats the resource as not available (Shopify Payments not used, or not allowed).
    async fn get(&self, url: &str, missing_ok: bool) -> AppResult<Option<(Value, Option<String>)>> {
        if !url.starts_with(&self.base) {
            return Err(AppError::internal("Shopify answered with a page on another site"));
        }
        for attempt in 0..5 {
            let res = self
                .http
                .get(url)
                .header("X-Shopify-Access-Token", &self.token)
                .header("Accept", "application/json")
                .send()
                .await
                .map_err(unreachable)?;
            let status = res.status().as_u16();
            if status == 429 {
                let wait = res
                    .headers()
                    .get("Retry-After")
                    .and_then(|v| v.to_str().ok())
                    // "2.0" → 2 whole seconds.
                    .and_then(|v| v.split('.').next().and_then(|w| w.trim().parse::<u64>().ok()))
                    .unwrap_or(2);
                tokio::time::sleep(Duration::from_secs(wait.clamp(1, 10))).await;
                if attempt < 4 {
                    continue;
                }
            }
            if (status == 404 || status == 403) && missing_ok {
                return Ok(None);
            }
            if status == 401 || status == 403 {
                return Err(AppError::bad_request(
                    "Shopify refused the access key. Check it, and that the custom app may read orders and Shopify Payments.",
                ));
            }
            if !(200..300).contains(&status) {
                return Err(AppError::bad_request(format!(
                    "Shopify answered {status} for {}",
                    url.split('?').next().unwrap_or(url)
                )));
            }
            let next = next_link(res.headers().get("Link").and_then(|v| v.to_str().ok()));
            let body: Value = res.json().await.map_err(unreachable)?;
            return Ok(Some((body, next)));
        }
        Err(AppError::bad_request("Shopify is busy (too many requests). Try again in a minute."))
    }

    async fn all(&self, first: String, key: &str, missing_ok: bool) -> AppResult<Option<Vec<Value>>> {
        let mut out = Vec::new();
        let mut url = Some(first);
        let mut pages = 0;
        while let Some(u) = url {
            let Some((body, next)) = self.get(&u, missing_ok).await? else { return Ok(None) };
            out.extend(body[key].as_array().cloned().unwrap_or_default());
            url = next;
            pages += 1;
            if pages >= 200 {
                break;
            }
        }
        Ok(Some(out))
    }

    pub async fn shop(&self) -> AppResult<Value> {
        let (body, _) = self.get(&self.url("shop.json"), false).await?.unwrap_or_default();
        Ok(body["shop"].clone())
    }

    pub async fn orders(&self, created_min: &str) -> AppResult<Vec<Value>> {
        let url = self.url(&format!("orders.json?status=any&limit=250&created_at_min={created_min}T00:00:00Z"));
        Ok(self.all(url, "orders", false).await?.unwrap_or_default())
    }

    /// None when the store does not use Shopify Payments (or the key may not read it).
    pub async fn balance_transactions(&self) -> AppResult<Option<Vec<Value>>> {
        self.all(self.url("shopify_payments/balance/transactions.json?limit=250"), "transactions", true).await
    }
}

/// Connects: checks the key against the store, then keeps the key and the store's details.
pub async fn connect(db: &Db, data_dir: &Path, shop: &str, token: &str) -> AppResult<Value> {
    let shop = normalize_shop(shop)?;
    let token = token.trim();
    if token.len() < 10 || token.contains(char::is_whitespace) {
        return Err(AppError::bad_request(
            "Paste the Admin API access key from the custom app (it starts with shpat_)",
        ));
    }
    let cfg = load_config(db).await?;
    let client = Client::new(&shop, token, &cfg.api_version)?;
    let info = client.shop().await?;
    app_core::secrets::store(&token_path(data_dir), token.as_bytes())
        .map_err(|e| AppError::internal(format!("could not keep the Shopify key: {e}")))?;
    let text = |k: &str| info[k].as_str().map(str::to_string);
    let start =
        cfg.start_date.clone().or_else(|| Some(format!("{}-01", app_core::time::today_local().format("%Y-%m"))));
    save(
        db,
        &[
            ("shop", Some(shop)),
            ("shopName", text("name")),
            ("shopCurrency", text("currency")),
            ("startDate", start),
            ("lastError", None),
        ],
    )
    .await?;
    let cfg = load_config(db).await?;
    Ok(status_json(&cfg, true))
}

pub async fn disconnect(db: &Db, data_dir: &Path) -> AppResult<Value> {
    app_core::secrets::remove(&token_path(data_dir)).map_err(|e| AppError::internal(e.to_string()))?;
    save(db, &[("shop", None), ("autoSync", Some("false".into()))]).await?;
    Ok(status_json(&load_config(db).await?, false))
}

/// Adds "1090 Shopify Clearing" (an asset account the store's money passes through) unless an
/// account of that name exists, and chooses it.
pub async fn ensure_clearing_account(db: &Db) -> AppResult<String> {
    let mut tx = db.begin().await?;
    let found: Option<String> =
        sqlx::query_scalar(r#"SELECT "id" FROM "accounts" WHERE lower("name") = 'shopify clearing' LIMIT 1"#)
            .fetch_optional(&mut *tx)
            .await?;
    let id = match found {
        Some(id) => id,
        None => {
            if ledger::accounts::find_by_code(&mut tx, "1090").await.is_ok() {
                return Err(AppError::bad_request(
                    "Account code 1090 is taken. Add a \"Shopify Clearing\" asset account on the Chart of Accounts and choose it.",
                ));
            }
            ledger::accounts::insert_chart(
                &mut tx,
                &[("1090", "Shopify Clearing", ledger::AccountType::Asset, "current_asset")],
            )
            .await?;
            ledger::accounts::find_by_code(&mut tx, "1090").await?.id
        }
    };
    tx.commit().await?;
    save(db, &[("clearingAccountId", Some(id.clone()))]).await?;
    Ok(id)
}

pub fn connected(data_dir: &Path) -> bool {
    token_path(data_dir).exists()
}

// ─── Sync ───────────────────────────────────────────────────────────────────

fn money(v: &Value) -> Option<Money> {
    match v {
        Value::String(s) => Money::parse_decimal(s),
        Value::Number(_) => Money::from_json(v),
        _ => None,
    }
}

fn text<'a>(v: &'a Value, k: &str) -> &'a str {
    v[k].as_str().map(str::trim).unwrap_or("")
}

/// The key a Shopify tax is mapped by: "VAT|0.15", several joined with "+".
pub fn tax_key(tax_lines: &Value) -> Option<String> {
    let parts: Vec<String> = tax_lines
        .as_array()?
        .iter()
        .filter(|t| money(&t["price"]).is_some_and(|p| p != Money::ZERO))
        .map(|t| format!("{}|{}", text(t, "title"), t["rate"].as_f64().map(|r| r.to_string()).unwrap_or_default()))
        .collect();
    (!parts.is_empty()).then(|| parts.join("+"))
}

fn customer_name(order: &Value) -> String {
    let c = &order["customer"];
    let full = format!("{} {}", text(c, "first_name"), text(c, "last_name")).trim().to_string();
    if !full.is_empty() {
        return full;
    }
    for v in [text(&order["billing_address"], "name"), text(&order["shipping_address"], "name"), text(order, "email")] {
        if !v.is_empty() {
            return v.to_string();
        }
    }
    "Shopify customer".into()
}

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

struct Ctx<'a> {
    cfg: &'a Config,
    shop: &'a str,
    home: String,
    created_by: Option<&'a str>,
    unmapped: std::collections::BTreeSet<String>,
}

/// One order as a draft invoice: Ok(Some(message)) created, Ok(None) already there.
async fn order_to_invoice(tx: &mut Tx, ctx: &mut Ctx<'_>, import_id: &str, order: &Value) -> AppResult<Option<String>> {
    let id = order["id"].to_string();
    let key = format!("shopify:{}:order:{id}", ctx.shop);
    if imports::imported_as(tx, Kind::Invoices, &key).await?.is_some() {
        return Ok(None);
    }
    let name = text(order, "name");
    let currency = text(order, "currency");
    if !currency.is_empty() && currency != ctx.home {
        return Err(AppError::bad_request(format!(
            "The order is in {currency} and the company's currency is {}; such orders are not brought in yet",
            ctx.home
        )));
    }
    let date = [text(order, "processed_at"), text(order, "created_at")]
        .into_iter()
        .find(|d| d.len() >= 10)
        .map(|d| d[..10].to_string())
        .ok_or_else(|| AppError::bad_request("The order has no date"))?;
    let mut lines = Vec::new();
    let tax_for =
        |tax_lines: &Value, unmapped: &mut std::collections::BTreeSet<String>| -> Result<Option<String>, String> {
            match tax_key(tax_lines) {
                None => Ok(None),
                Some(k) => match ctx.cfg.tax_map.get(&k) {
                    Some(code) => Ok(Some(code.clone())),
                    None => {
                        unmapped.insert(k.clone());
                        Err(k)
                    }
                },
            }
        };
    let mut missing_tax = Vec::new();
    for li in order["line_items"].as_array().cloned().unwrap_or_default() {
        let qty = li["quantity"].as_i64().unwrap_or(1).max(1);
        let price = money(&li["price"]).ok_or_else(|| AppError::bad_request("A line has no price"))?;
        let allocated: Option<Money> = li["discount_allocations"].as_array().filter(|a| !a.is_empty()).map(|a| {
            a.iter().filter_map(|d| money(&d["amount"])).fold(Money::ZERO, |s, m| s.checked_add(m).unwrap_or(s))
        });
        let discount = allocated.or_else(|| money(&li["total_discount"])).unwrap_or(Money::ZERO);
        let tax_code_id = match tax_for(&li["tax_lines"], &mut ctx.unmapped) {
            Ok(t) => t,
            Err(k) => {
                missing_tax.push(k);
                None
            }
        };
        let sku = text(&li, "sku");
        let item_id = if sku.is_empty() {
            None
        } else {
            find_id(tx, r#"SELECT "id" FROM "items" WHERE lower(trim("sku")) = lower(?1) AND "isActive" = 1"#, sku)
                .await?
        };
        let variant = text(&li, "variant_title");
        let title = text(&li, "title");
        lines.push(LineInput {
            description: if variant.is_empty() || variant == "Default Title" {
                title.to_string()
            } else {
                format!("{title} — {variant}")
            },
            quantity: Some(Money::from_cents(qty * 100)),
            unit_price: Some(price),
            account_id: if item_id.is_some() { None } else { ctx.cfg.income_account_id.clone() },
            tax_code_id,
            item_id,
            discount,
        });
    }
    for sl in order["shipping_lines"].as_array().cloned().unwrap_or_default() {
        let price = money(&sl["discounted_price"]).or_else(|| money(&sl["price"])).unwrap_or(Money::ZERO);
        if price == Money::ZERO {
            continue;
        }
        let tax_code_id = match tax_for(&sl["tax_lines"], &mut ctx.unmapped) {
            Ok(t) => t,
            Err(k) => {
                missing_tax.push(k);
                None
            }
        };
        lines.push(LineInput {
            description: format!("Shipping: {}", text(&sl, "title")),
            quantity: None,
            unit_price: Some(price),
            account_id: ctx.cfg.shipping_account_id.clone().or_else(|| ctx.cfg.income_account_id.clone()),
            tax_code_id,
            item_id: None,
            discount: Money::ZERO,
        });
    }
    if !missing_tax.is_empty() {
        missing_tax.dedup();
        return Err(AppError::bad_request(format!(
            "Match the Shopify tax {} to a tax code under Settings → Integrations, then sync again",
            missing_tax.iter().map(|k| format!("\"{}\"", k.replace('|', " "))).collect::<Vec<_>>().join(", ")
        )));
    }
    if lines.is_empty() {
        return Err(AppError::bad_request("The order has no lines"));
    }
    let who = customer_name(order);
    let existing = find_id(
        tx,
        r#"SELECT "id" FROM "customers" WHERE lower(trim("name")) = lower(trim(?1)) ORDER BY "isActive" DESC LIMIT 1"#,
        &who,
    )
    .await?;
    let customer_id = match existing {
        Some(c) => c,
        None => {
            let email = [text(&order["customer"], "email"), text(order, "email")].into_iter().find(|e| !e.is_empty());
            let p = parties::create_in(
                tx,
                PartyKind::Customer,
                PartyFields {
                    name: Some(who.clone()),
                    email: Some(email.map(str::to_string)),
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
            date,
            po_number: (!name.is_empty()).then(|| name.to_string()),
            notes: Some(format!("Shopify order {name}")),
            lines,
            prices_include_tax: order["taxes_included"].as_bool().unwrap_or(false),
            ..Default::default()
        },
        ctx.created_by,
    )
    .await?;
    imports::remember_item(tx, import_id, Kind::Invoices, &key, "INVOICE", &inv.id).await?;
    let mut msg = format!("Draft {} for {who}", inv.invoice_number);
    if let Some(total) = money(&order["total_price"]) {
        if total != inv.total_amount {
            let diff = inv.total_amount.checked_sub(total).unwrap_or(Money::ZERO);
            msg.push_str(&format!(
                " — differs from Shopify's total {} by {} (tax rounding)",
                total.to_db_string(),
                diff.to_db_string()
            ));
        }
    }
    if text(order, "financial_status").contains("refunded") {
        msg.push_str(" — refunded in Shopify: issue a credit note");
    }
    Ok(Some(msg))
}

/// A line to add: its key, amount, description and hint.
type LinePlan<'a> = (String, Money, String, Option<(&'a str, &'a str)>);

/// One Shopify Payments balance transaction as one or two clearing-account lines.
async fn balance_lines(
    tx: &mut Tx,
    ctx: &Ctx<'_>,
    import_id: &str,
    clearing: &str,
    order_names: &BTreeMap<String, (String, String)>,
    t: &Value,
) -> AppResult<(i64, i64)> {
    let id = t["id"].to_string();
    let date = text(t, "processed_at");
    if date.len() < 10 {
        return Ok((0, 0));
    }
    let date = &date[..10];
    let kind = text(t, "type");
    let order = order_names.get(&t["source_order_id"].to_string());
    let (order_name, customer) = order.map(|(n, c)| (n.as_str(), c.as_str())).unwrap_or(("", ""));
    let amount = money(&t["amount"]).unwrap_or(Money::ZERO);
    let fee = money(&t["fee"]).unwrap_or(Money::ZERO);
    let (description, hint) = match kind {
        "charge" => (format!("Shopify payment {order_name} {customer}").trim().to_string(), None),
        "refund" => (format!("Shopify refund {order_name} {customer}").trim().to_string(), None),
        "payout" => {
            ("Shopify payout to the bank".to_string(), ctx.cfg.payout_account_id.as_deref().map(|a| ("TRANSFER", a)))
        }
        other => (format!("Shopify {} {order_name}", other.replace('_', " ")).trim().to_string(), None),
    };
    let mut entries: Vec<LinePlan<'_>> = Vec::new();
    if amount != Money::ZERO {
        entries.push((format!("shopify:{}:bt:{id}", ctx.shop), amount, description, hint));
    }
    if fee > Money::ZERO {
        entries.push((
            format!("shopify:{}:fee:{id}", ctx.shop),
            Money::ZERO.checked_sub(fee).unwrap_or(fee),
            format!("Shopify fee {order_name}").trim().to_string(),
            ctx.cfg.fee_account_id.as_deref().map(|a| ("EXPENSE", a)),
        ));
    }
    let (mut created, mut skipped) = (0, 0);
    for (key, amount, description, hint) in entries {
        if imports::imported_as(tx, Kind::Bank, &key).await?.is_some() {
            skipped += 1;
            continue;
        }
        let reference = if order_name.is_empty() { None } else { Some(order_name) };
        let line_id = bank_lines::insert(
            tx,
            NewBankLine { import_id, account_id: clearing, date, description: &description, reference, amount, hint },
        )
        .await?;
        imports::remember_item(tx, import_id, Kind::Bank, &key, "BANK_LINE", &line_id).await?;
        created += 1;
    }
    Ok((created, skipped))
}

async fn home_currency(conn: &mut SqliteConnection) -> AppResult<String> {
    let c: Option<String> = sqlx::query_scalar(r#"SELECT "value" FROM "settings" WHERE "key" = 'region.currency'"#)
        .fetch_optional(conn)
        .await?;
    Ok(c.filter(|c| !c.is_empty()).unwrap_or_else(|| "LKR".into()))
}

/// Brings in new orders and Shopify Payments lines; returns the report (also kept as the last
/// result). Network errors are reported and leave the books unchanged.
pub async fn sync(db: &Db, data_dir: &Path, created_by: Option<&str>) -> AppResult<Value> {
    let result = run_sync(db, data_dir, created_by).await;
    let now = now_iso();
    match &result {
        Ok(report) => {
            save(db, &[("lastSyncAt", Some(now)), ("lastResult", Some(report.to_string())), ("lastError", None)])
                .await?
        }
        Err(e) => save(db, &[("lastError", Some(format!("{now} {}", e.user_message())))]).await?,
    }
    result
}

async fn run_sync(db: &Db, data_dir: &Path, created_by: Option<&str>) -> AppResult<Value> {
    let cfg = load_config(db).await?;
    let shop = cfg.shop.clone().ok_or_else(|| AppError::bad_request("Connect Shopify first"))?;
    let token = app_core::secrets::load(&token_path(data_dir))
        .map_err(|e| AppError::internal(e.to_string()))?
        .ok_or_else(|| AppError::bad_request("Connect Shopify first"))?;
    let token = String::from_utf8(token).map_err(AppError::internal)?;
    let client = Client::new(&shop, &token, &cfg.api_version)?;

    // Everything is fetched before anything is written.
    let start =
        cfg.start_date.clone().unwrap_or_else(|| format!("{}-01", app_core::time::today_local().format("%Y-%m")));
    let since = match cfg
        .last_sync_at
        .as_deref()
        .and_then(|d| chrono::NaiveDate::parse_from_str(&d[..10.min(d.len())], "%Y-%m-%d").ok())
    {
        Some(last) => {
            let back =
                last.checked_sub_days(chrono::Days::new(OVERLAP_DAYS)).unwrap_or(last).format("%Y-%m-%d").to_string();
            if back > start {
                back
            } else {
                start.clone()
            }
        }
        None => start.clone(),
    };
    let orders = client.orders(&since).await?;
    let balance = if cfg.clearing_account_id.is_some() { client.balance_transactions().await? } else { None };

    let mut tx = db.begin().await?;
    let home = home_currency(&mut tx).await?;
    let source = format!("Shopify ({})", cfg.shop_name.clone().unwrap_or_else(|| shop.clone()));
    let mut ctx = Ctx { cfg: &cfg, shop: &shop, home, created_by, unmapped: Default::default() };
    let mut rows = Vec::new();
    let mut notes = Vec::new();

    let inv_import = imports::open_import(&mut tx, Kind::Invoices, &source, None, None, created_by).await?;
    let (mut o_new, mut o_skip, mut o_err) = (0i64, 0i64, 0i64);
    let mut order_names = BTreeMap::new();
    for order in &orders {
        let name = text(order, "name").to_string();
        order_names.insert(order["id"].to_string(), (name.clone(), customer_name(order)));
        if !order["cancelled_at"].is_null() || text(order, "financial_status") == "voided" {
            o_skip += 1;
            rows.push(row("ORDER", &name, "SKIPPED", "Cancelled in Shopify"));
            continue;
        }
        savepoint(&mut tx, "SAVEPOINT shopify_order").await?;
        match order_to_invoice(&mut tx, &mut ctx, &inv_import, order).await {
            Ok(Some(msg)) => {
                savepoint(&mut tx, "RELEASE shopify_order").await?;
                o_new += 1;
                rows.push(row("ORDER", &name, "NEW", msg));
            }
            Ok(None) => {
                savepoint(&mut tx, "RELEASE shopify_order").await?;
                o_skip += 1;
            }
            Err(e) => {
                savepoint(&mut tx, "ROLLBACK TO shopify_order").await?;
                savepoint(&mut tx, "RELEASE shopify_order").await?;
                if let AppError::Internal(d) = &e {
                    tracing::error!("shopify order {name}: {d}");
                }
                o_err += 1;
                rows.push(row("ORDER", &name, "ERROR", e.user_message()));
            }
        }
    }
    imports::close_import(&mut tx, &inv_import, o_new, o_skip).await?;

    let (mut l_new, mut l_skip) = (0i64, 0i64);
    match (&cfg.clearing_account_id, &balance) {
        (None, _) => {
            notes.push("Choose a clearing account to bring in Shopify Payments payments, fees and payouts.".to_string())
        }
        (Some(_), None) => notes.push(
            "Shopify Payments transactions were not available (not used by this store, or not allowed for the key)."
                .to_string(),
        ),
        (Some(clearing), Some(list)) => {
            let account = ledger::accounts::find_one(&mut tx, clearing).await?;
            let shop_currency = cfg.shop_currency.clone().unwrap_or_else(|| ctx.home.clone());
            let account_currency = account.currency.clone().unwrap_or_else(|| ctx.home.clone());
            if shop_currency != account_currency {
                notes.push(format!("The clearing account is in {account_currency} but Shopify pays out in {shop_currency}; payments were not brought in."));
            } else {
                let bank_import =
                    imports::open_import(&mut tx, Kind::Bank, &source, None, Some(clearing), created_by).await?;
                for t in list {
                    if t["test"].as_bool() == Some(true)
                        || text(t, "processed_at").get(..10).is_none_or(|d| d < start.as_str())
                    {
                        continue;
                    }
                    let (c, s) = balance_lines(&mut tx, &ctx, &bank_import, clearing, &order_names, t).await?;
                    l_new += c;
                    l_skip += s;
                }
                imports::close_import(&mut tx, &bank_import, l_new, l_skip).await?;
            }
        }
    }
    tx.commit().await?;

    if !ctx.unmapped.is_empty() {
        notes.push("Some Shopify taxes are not matched to a tax code yet — see the list in the settings.".into());
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
        // The activity log's detail.
        "name": format!("{o_new} orders, {l_new} payment lines"),
    }))
}

/// The background loop: when automatic sync is on and Shopify is connected, once an hour.
pub fn spawn(db: Db, data_dir: PathBuf) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(120)).await;
        loop {
            match load_config(&db).await {
                Ok(cfg) if cfg.auto_sync && cfg.shop.is_some() && connected(&data_dir) => {
                    match sync(&db, &data_dir, None).await {
                        Ok(r) => {
                            tracing::info!(detail = %r["name"], "automatic Shopify sync");
                            audit::record(
                                &db,
                                audit::NewEntry {
                                    user_id: None,
                                    username: Some("system".into()),
                                    action: "Synced with Shopify (automatic)".into(),
                                    method: None,
                                    path: None,
                                    status_code: None,
                                    detail: r["name"].as_str().map(str::to_string),
                                },
                            )
                            .await;
                        }
                        Err(e) => tracing::warn!("automatic Shopify sync failed: {}", e.user_message()),
                    }
                }
                Ok(_) => {}
                Err(e) => tracing::error!("Shopify settings unreadable: {}", e.user_message()),
            }
            tokio::time::sleep(Duration::from_secs(3600)).await;
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn store_addresses_are_normalised() {
        assert_eq!(normalize_shop("MyStore").ok().as_deref(), Some("mystore.myshopify.com"));
        assert_eq!(
            normalize_shop("https://my-store.myshopify.com/admin").ok().as_deref(),
            Some("my-store.myshopify.com")
        );
        assert!(normalize_shop("not a shop!").is_err());
        assert!(normalize_shop("").is_err());
    }

    #[test]
    fn next_page_comes_from_the_link_header() {
        let h = r#"<https://s.myshopify.com/admin/api/2026-07/orders.json?page_info=abc&limit=250>; rel="next", <https://x>; rel="previous""#;
        assert_eq!(
            next_link(Some(h)).as_deref(),
            Some("https://s.myshopify.com/admin/api/2026-07/orders.json?page_info=abc&limit=250")
        );
        assert_eq!(next_link(Some(r#"<https://x>; rel="previous""#)), None);
    }

    #[test]
    fn tax_keys_ignore_zero_lines_and_join_several() {
        let one =
            json!([{"title": "VAT", "rate": 0.15, "price": "12.75"}, {"title": "NBT", "rate": 0.02, "price": "0.00"}]);
        assert_eq!(tax_key(&one).as_deref(), Some("VAT|0.15"));
        let two =
            json!([{"title": "GST", "rate": 0.05, "price": "1.00"}, {"title": "PST", "rate": 0.07, "price": "1.40"}]);
        assert_eq!(tax_key(&two).as_deref(), Some("GST|0.05+PST|0.07"));
        assert_eq!(tax_key(&json!([])), None);
    }
}
