//! D39: Shopify (integrations plan, phase 3), on the shared connector framework (D40).
//!
//! Connect with the store address and an Admin API access key from a Shopify custom app
//! (read_orders, read_shopify_payments_payouts; read_all_orders for orders older than 60 days).
//! Orders become draft invoices (order number as PO number; product lines with their discounts and
//! tax; shipping as a line; cancelled orders skipped). Shopify Payments balance transactions become
//! lines on the clearing account: payments, fees, refunds and payouts (test ones skipped).

use crate::common::{self, money, text, Config, Connected, Fetched, MoneyKind, MoneyLine, Order, OrderLine};
use crate::Connector;
use app_core::money::Money;
use app_core::{AppError, AppResult};
use serde_json::{Map, Value};
use std::collections::BTreeMap;

pub const DEFAULT_API_VERSION: &str = "2026-07";
const SERVICE: &str = "Shopify";

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

/// The `rel="next"` page of a `Link` header.
pub fn next_link(header: Option<&str>) -> Option<String> {
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

/// The key a Shopify tax is matched by: "VAT|0.15", several joined with "+".
pub fn tax_key(tax_lines: &Value) -> Option<String> {
    let parts: Vec<String> = tax_lines
        .as_array()?
        .iter()
        .filter(|t| money(&t["price"]).is_some_and(|p| p != Money::ZERO))
        .map(|t| common::tax_part(text(t, "title"), t["rate"].as_f64().unwrap_or_default()))
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

struct Api {
    base: String,
    token: String,
    version: String,
    http: reqwest::Client,
}

impl Api {
    fn new(shop: &str, token: &str, version: &str) -> AppResult<Api> {
        Ok(Api {
            base: common::base_url(Connector::Shopify, format!("https://{shop}")),
            token: token.to_string(),
            version: version.to_string(),
            http: common::http()?,
        })
    }

    fn url(&self, path: &str) -> String {
        format!("{}/admin/api/{}/{}", self.base, self.version, path)
    }

    /// One page and the next page's address; `Ok(None)` for 404 / 403 when `missing_ok`.
    async fn get(&self, url: &str, missing_ok: bool) -> AppResult<Option<(Value, Option<String>)>> {
        if !url.starts_with(&self.base) {
            return Err(AppError::internal("Shopify answered with a page on another site"));
        }
        let req = self.http.get(url).header("X-Shopify-Access-Token", &self.token).header("Accept", "application/json");
        let (status, headers, body) = common::send(SERVICE, req).await?;
        if (status == 404 || status == 403) && missing_ok {
            return Ok(None);
        }
        if status == 401 || status == 403 {
            return Err(common::refused(SERVICE, "access key"));
        }
        if !(200..300).contains(&status) {
            return Err(AppError::bad_request(format!(
                "Shopify answered {status} for {}",
                url.split('?').next().unwrap_or(url)
            )));
        }
        Ok(Some((body, next_link(headers.get("Link").and_then(|v| v.to_str().ok())))))
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
}

fn version(cfg: &Config) -> String {
    cfg.option("apiVersion").unwrap_or(DEFAULT_API_VERSION).to_string()
}

pub async fn connect(cfg: &Config, body: &Map<String, Value>) -> AppResult<Connected> {
    let shop = normalize_shop(body.get("shop").and_then(Value::as_str).unwrap_or(""))?;
    let token = body.get("token").and_then(Value::as_str).unwrap_or("").trim().to_string();
    if token.len() < 10 || token.contains(char::is_whitespace) {
        return Err(AppError::bad_request(
            "Paste the Admin API access key from the custom app (it starts with shpat_)",
        ));
    }
    let api = Api::new(&shop, &token, &version(cfg))?;
    let (info, _) = api.get(&api.url("shop.json"), false).await?.unwrap_or_default();
    let info = &info["shop"];
    Ok(Connected {
        remote_id: shop,
        remote_name: info["name"].as_str().map(str::to_string),
        remote_currency: info["currency"].as_str().map(str::to_string),
        secret: serde_json::json!({ "token": token }),
        options: vec![("apiVersion", version(cfg))],
    })
}

fn order(o: &Value) -> Order {
    let mut lines = Vec::new();
    for li in o["line_items"].as_array().cloned().unwrap_or_default() {
        let allocated = li["discount_allocations"].as_array().filter(|a| !a.is_empty()).map(|a| {
            a.iter().filter_map(|d| money(&d["amount"])).fold(Money::ZERO, |s, m| s.checked_add(m).unwrap_or(s))
        });
        let (title, variant) = (text(&li, "title"), text(&li, "variant_title"));
        lines.push(OrderLine {
            description: if variant.is_empty() || variant == "Default Title" {
                title.to_string()
            } else {
                format!("{title} — {variant}")
            },
            sku: Some(text(&li, "sku").to_string()),
            quantity: Money::from_cents(li["quantity"].as_i64().unwrap_or(1).max(1) * 100),
            unit_price: money(&li["price"]).unwrap_or(Money::ZERO),
            discount: allocated.or_else(|| money(&li["total_discount"])).unwrap_or(Money::ZERO),
            tax_key: tax_key(&li["tax_lines"]),
            shipping: false,
        });
    }
    for sl in o["shipping_lines"].as_array().cloned().unwrap_or_default() {
        let price = money(&sl["discounted_price"]).or_else(|| money(&sl["price"])).unwrap_or(Money::ZERO);
        if price == Money::ZERO {
            continue;
        }
        lines.push(OrderLine {
            description: format!("Shipping: {}", text(&sl, "title")),
            quantity: Money::from_cents(100),
            unit_price: price,
            tax_key: tax_key(&sl["tax_lines"]),
            shipping: true,
            ..Default::default()
        });
    }
    let date = [text(o, "processed_at"), text(o, "created_at")]
        .into_iter()
        .find(|d| d.len() >= 10)
        .map(|d| d[..10].to_string());
    let skip = if !o["cancelled_at"].is_null() || text(o, "financial_status") == "voided" {
        Some("Cancelled in Shopify".to_string())
    } else if date.is_none() {
        Some("The order has no date".to_string())
    } else {
        None
    };
    Order {
        id: o["id"].to_string(),
        number: text(o, "name").to_string(),
        date: date.unwrap_or_default(),
        currency: text(o, "currency").to_string(),
        customer: customer_name(o),
        email: [text(&o["customer"], "email"), text(o, "email")]
            .into_iter()
            .find(|e| !e.is_empty())
            .map(str::to_string),
        lines,
        taxes_included: o["taxes_included"].as_bool().unwrap_or(false),
        total: money(&o["total_price"]),
        skip,
        refunded: text(o, "financial_status").contains("refunded"),
    }
}

/// One Shopify Payments balance transaction as one or two money lines.
fn balance(t: &Value, orders: &BTreeMap<String, (String, String)>, currency: &str) -> Vec<MoneyLine> {
    let date = text(t, "processed_at");
    if t["test"].as_bool() == Some(true) || date.len() < 10 {
        return Vec::new();
    }
    let id = t["id"].to_string();
    let (order_name, customer) =
        orders.get(&t["source_order_id"].to_string()).map(|(n, c)| (n.as_str(), c.as_str())).unwrap_or(("", ""));
    let (kind, description) = match text(t, "type") {
        "charge" => (MoneyKind::Payment, format!("Shopify payment {order_name} {customer}")),
        "refund" => (MoneyKind::Refund, format!("Shopify refund {order_name} {customer}")),
        "payout" => (MoneyKind::Payout, "Shopify payout to the bank".to_string()),
        other => (MoneyKind::Other, format!("Shopify {} {order_name}", other.replace('_', " "))),
    };
    let reference = (!order_name.is_empty()).then(|| order_name.to_string());
    let currency =
        [text(t, "currency"), currency].into_iter().find(|c| !c.is_empty()).unwrap_or("").to_ascii_uppercase();
    let mut out = vec![MoneyLine {
        id: format!("bt:{id}"),
        date: date[..10].to_string(),
        amount: money(&t["amount"]).unwrap_or(Money::ZERO),
        currency: currency.clone(),
        description: description.trim().to_string(),
        reference: reference.clone(),
        kind,
    }];
    if let Some(fee) = money(&t["fee"]).filter(|f| *f > Money::ZERO) {
        out.push(MoneyLine {
            id: format!("fee:{id}"),
            date: date[..10].to_string(),
            amount: Money::ZERO.checked_sub(fee).unwrap_or(fee),
            currency,
            description: format!("Shopify fee {order_name}").trim().to_string(),
            reference,
            kind: MoneyKind::Fee,
        });
    }
    out
}

pub async fn fetch(cfg: &Config, secret: &Value, since: &str) -> AppResult<Fetched> {
    let shop = cfg.remote_id.clone().unwrap_or_default();
    let api = Api::new(&shop, secret["token"].as_str().unwrap_or(""), &version(cfg))?;
    let raw = api
        .all(api.url(&format!("orders.json?status=any&limit=250&created_at_min={since}T00:00:00Z")), "orders", false)
        .await?
        .unwrap_or_default();
    let names: BTreeMap<String, (String, String)> =
        raw.iter().map(|o| (o["id"].to_string(), (text(o, "name").to_string(), customer_name(o)))).collect();
    let orders = raw.iter().map(order).collect();
    let mut notes = Vec::new();
    let money = if cfg.clearing_account_id.is_some() {
        match api.all(api.url("shopify_payments/balance/transactions.json?limit=250"), "transactions", true).await? {
            Some(list) => {
                let cur = cfg.remote_currency.clone().unwrap_or_default();
                Some(list.iter().flat_map(|t| balance(t, &names, &cur)).collect())
            }
            None => {
                notes.push(
                    "Shopify Payments transactions were not available (not used by this store, or not allowed for the key)."
                        .into(),
                );
                None
            }
        }
    } else {
        None
    };
    Ok(Fetched { orders, money, notes })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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
