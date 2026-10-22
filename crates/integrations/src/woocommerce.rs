//! D40: WooCommerce orders (integrations plan, phase 4).
//!
//! Connect with the site address and a REST API key (WooCommerce → Settings → Advanced → REST API,
//! permission Read). Orders that were paid or are being fulfilled (processing, completed, on-hold,
//! refunded) become draft invoices; unpaid, cancelled and failed orders are skipped. WooCommerce
//! reports line amounts without tax, before (`subtotal`) and after (`total`) discounts, so the
//! invoice is written without tax included and the discount is their difference. Payments arrive
//! through the payment service the shop uses (Stripe, PayPal…), which has its own connector.

use crate::common::{self, money, text, Config, Connected, Fetched, Order, OrderLine};
use crate::Connector;
use app_core::money::Money;
use app_core::{AppError, AppResult};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;

const SERVICE: &str = "WooCommerce";
const PAID: &[&str] = &["processing", "completed", "on-hold", "refunded"];

/// "shop.example.lk" or "https://shop.example.lk/" → "https://shop.example.lk". The keys go only
/// over HTTPS.
pub fn normalize_site(input: &str) -> AppResult<String> {
    let s = input.trim().trim_end_matches('/');
    let s = if s.starts_with("http://") || s.starts_with("https://") { s.to_string() } else { format!("https://{s}") };
    let host = s.split("://").nth(1).unwrap_or("").split('/').next().unwrap_or("");
    if host.is_empty() || !host.contains('.') || host.contains(char::is_whitespace) {
        return Err(AppError::bad_request("Enter the shop's web address, e.g. https://shop.example.lk"));
    }
    if s.starts_with("http://") {
        return Err(AppError::bad_request("The shop must use https:// so the key is never sent in the clear"));
    }
    Ok(s.to_lowercase())
}

struct Api {
    base: String,
    key: String,
    secret: String,
    http: reqwest::Client,
}

impl Api {
    fn new(site: &str, key: &str, secret: &str) -> AppResult<Api> {
        Ok(Api {
            base: common::base_url(Connector::WooCommerce, site.to_string()),
            key: key.to_string(),
            secret: secret.to_string(),
            http: common::http()?,
        })
    }

    /// GET with the key; if the site strips the Authorization header (common on shared hosting),
    /// once more with the key in the query, as WooCommerce allows over HTTPS.
    async fn get(&self, path: &str, query: &[(&str, String)]) -> AppResult<(Value, reqwest::header::HeaderMap)> {
        let url = format!("{}/wp-json/wc/v3/{path}", self.base);
        let req = self.http.get(&url).basic_auth(&self.key, Some(&self.secret)).query(query);
        let (mut status, mut headers, mut body) = common::send(SERVICE, req).await?;
        if status == 401 {
            let mut q: Vec<(&str, String)> = query.to_vec();
            q.push(("consumer_key", self.key.clone()));
            q.push(("consumer_secret", self.secret.clone()));
            (status, headers, body) = common::send(SERVICE, self.http.get(&url).query(&q)).await?;
        }
        if status == 401 || status == 403 {
            return Err(common::refused(SERVICE, "REST API key"));
        }
        if status == 404 {
            return Err(AppError::bad_request(
                "No WooCommerce REST API at this address. Check the site address, and that permalinks are not set to \"Plain\".",
            ));
        }
        if !(200..300).contains(&status) {
            return Err(AppError::bad_request(format!("WooCommerce answered {status} for {path}")));
        }
        Ok((body, headers))
    }
}

pub async fn connect(body: &Map<String, Value>) -> AppResult<Connected> {
    let s = |k: &str| body.get(k).and_then(Value::as_str).unwrap_or("").trim().to_string();
    let site = normalize_site(&s("site"))?;
    let (key, secret) = (s("consumerKey"), s("consumerSecret"));
    if !key.starts_with("ck_") || !secret.starts_with("cs_") {
        return Err(AppError::bad_request(
            "Paste the consumer key (ck_…) and consumer secret (cs_…) of the REST API key",
        ));
    }
    let api = Api::new(&site, &key, &secret)?;
    let (general, _) = api.get("settings/general", &[]).await?;
    let currency = general
        .as_array()
        .and_then(|a| a.iter().find(|x| x["id"] == "woocommerce_currency"))
        .and_then(|x| x["value"].as_str())
        .map(str::to_string);
    let name = site.split("://").nth(1).map(str::to_string);
    Ok(Connected {
        remote_id: site.clone(),
        remote_name: name,
        remote_currency: currency,
        secret: json!({ "key": key, "secret": secret }),
        options: vec![("siteUrl", site)],
    })
}

/// "18" → "0.18", "2.5" → "0.025", "100" → "1": a percentage as the fraction Shopify's keys use,
/// moved by text so no rounding creeps in.
pub fn percent_to_fraction(percent: &str) -> String {
    let p = percent.trim().trim_matches('"');
    let (int, frac) = p.split_once('.').unwrap_or((p, ""));
    if int.is_empty() || !int.bytes().all(|b| b.is_ascii_digit()) || !frac.bytes().all(|b| b.is_ascii_digit()) {
        return p.to_string();
    }
    let digits = format!("{int}{frac}");
    let point = int.len() as i64 - 2;
    let joined = if point <= 0 {
        format!("0.{}{digits}", "0".repeat((-point) as usize))
    } else {
        format!("{}.{}", &digits[..point as usize], &digits[point as usize..])
    };
    let joined = joined.trim_start_matches('0');
    let joined = if joined.starts_with('.') { format!("0{joined}") } else { joined.to_string() };
    joined.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// Rate id → "label|rate" from the order's tax lines.
fn tax_rates(o: &Value) -> BTreeMap<String, String> {
    o["tax_lines"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|t| {
                    let rate = percent_to_fraction(&t["rate_percent"].to_string());
                    (t["rate_id"].to_string(), format!("{}|{rate}", text(t, "label")))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn tax_key(taxes: &Value, rates: &BTreeMap<String, String>) -> Option<String> {
    let parts: Vec<String> = taxes
        .as_array()?
        .iter()
        .filter(|t| money(&t["total"]).is_some_and(|m| m != Money::ZERO))
        .map(|t| rates.get(&t["id"].to_string()).cloned().unwrap_or_else(|| format!("rate {}|", t["id"])))
        .collect();
    (!parts.is_empty()).then(|| parts.join("+"))
}

/// Quantity and unit price that give exactly `subtotal`: the real ones when it divides evenly,
/// otherwise one line of the whole amount (named "× qty").
fn split(name: &str, qty: i64, subtotal: Money) -> (String, Money, Money) {
    let qty = qty.max(1);
    if subtotal.cents() % qty == 0 {
        (name.to_string(), Money::from_cents(qty * 100), Money::from_cents(subtotal.cents() / qty))
    } else {
        (format!("{name} × {qty}"), Money::from_cents(100), subtotal)
    }
}

fn order(o: &Value) -> Order {
    let rates = tax_rates(o);
    let mut lines = Vec::new();
    for li in o["line_items"].as_array().cloned().unwrap_or_default() {
        let subtotal = money(&li["subtotal"]).unwrap_or(Money::ZERO);
        let total = money(&li["total"]).unwrap_or(subtotal);
        let (description, quantity, unit_price) =
            split(text(&li, "name"), li["quantity"].as_i64().unwrap_or(1), subtotal);
        lines.push(OrderLine {
            description,
            sku: Some(text(&li, "sku").to_string()),
            quantity,
            unit_price,
            discount: subtotal.checked_sub(total).filter(|d| *d > Money::ZERO).unwrap_or(Money::ZERO),
            tax_key: tax_key(&li["taxes"], &rates),
            shipping: false,
        });
    }
    for sl in o["shipping_lines"].as_array().cloned().unwrap_or_default() {
        let total = money(&sl["total"]).unwrap_or(Money::ZERO);
        if total > Money::ZERO {
            lines.push(OrderLine {
                description: format!("Shipping: {}", text(&sl, "method_title")),
                quantity: Money::from_cents(100),
                unit_price: total,
                tax_key: tax_key(&sl["taxes"], &rates),
                shipping: true,
                ..Default::default()
            });
        }
    }
    for fl in o["fee_lines"].as_array().cloned().unwrap_or_default() {
        let total = money(&fl["total"]).unwrap_or(Money::ZERO);
        if total > Money::ZERO {
            lines.push(OrderLine {
                description: format!("Fee: {}", text(&fl, "name")),
                quantity: Money::from_cents(100),
                unit_price: total,
                tax_key: tax_key(&fl["taxes"], &rates),
                ..Default::default()
            });
        }
    }
    let b = &o["billing"];
    let customer = [
        format!("{} {}", text(b, "first_name"), text(b, "last_name")).trim().to_string(),
        text(b, "company").into(),
        text(b, "email").into(),
    ]
    .into_iter()
    .find(|v: &String| !v.is_empty())
    .unwrap_or_else(|| "WooCommerce customer".into());
    let status = text(o, "status");
    let date = text(o, "date_created");
    Order {
        id: o["id"].to_string(),
        number: format!(
            "#{}",
            [text(o, "number"), &o["id"].to_string()].into_iter().find(|n| !n.is_empty()).unwrap_or("")
        ),
        date: date.get(..10).unwrap_or("").to_string(),
        currency: text(o, "currency").to_string(),
        customer,
        email: Some(text(b, "email").to_string()).filter(|e| !e.is_empty()),
        lines,
        taxes_included: false,
        total: money(&o["total"]),
        skip: if !PAID.contains(&status) {
            Some(format!("Status \"{status}\" in WooCommerce (not paid)"))
        } else if date.len() < 10 {
            Some("The order has no date".into())
        } else {
            None
        },
        refunded: status == "refunded",
    }
}

pub async fn fetch(cfg: &Config, secret: &Value, since: &str) -> AppResult<Fetched> {
    let site = cfg.option("siteUrl").map(str::to_string).or_else(|| cfg.remote_id.clone()).unwrap_or_default();
    let api = Api::new(&site, secret["key"].as_str().unwrap_or(""), secret["secret"].as_str().unwrap_or(""))?;
    let mut orders = Vec::new();
    let mut page = 1;
    loop {
        let (body, headers) = api
            .get(
                "orders",
                &[
                    ("after", format!("{since}T00:00:00")),
                    ("per_page", "100".into()),
                    ("page", page.to_string()),
                    ("orderby", "date".into()),
                    ("order", "asc".into()),
                ],
            )
            .await?;
        orders.extend(body.as_array().cloned().unwrap_or_default().iter().map(order));
        let pages: i64 =
            headers.get("X-WP-TotalPages").and_then(|v| v.to_str().ok()).and_then(|v| v.parse().ok()).unwrap_or(1);
        page += 1;
        if page > pages || page > 200 {
            break;
        }
    }
    Ok(Fetched { orders, money: None, notes: Vec::new() })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn site_addresses_need_https() {
        assert_eq!(normalize_site("Shop.Example.lk/").ok().as_deref(), Some("https://shop.example.lk"));
        assert!(normalize_site("http://shop.example.lk").is_err());
        assert!(normalize_site("not a site").is_err());
    }

    #[test]
    fn percentages_become_fractions_without_rounding() {
        assert_eq!(percent_to_fraction("18"), "0.18");
        assert_eq!(percent_to_fraction("2.5"), "0.025");
        assert_eq!(percent_to_fraction("15.0000"), "0.15");
        assert_eq!(percent_to_fraction("100"), "1");
        assert_eq!(percent_to_fraction("8"), "0.08");
    }

    #[test]
    fn quantities_that_do_not_divide_become_one_line() {
        let (d, q, p) = split("Tea", 3, Money::from_cents(1000));
        assert_eq!((d.as_str(), q.cents(), p.cents()), ("Tea × 3", 100, 1000));
        let (d, q, p) = split("Tea", 4, Money::from_cents(1000));
        assert_eq!((d.as_str(), q.cents(), p.cents()), ("Tea", 400, 250));
    }

    #[test]
    fn an_order_with_a_coupon_and_vat() {
        let o = json!({"id": 77, "number": "1077", "status": "completed", "currency": "LKR", "date_created": "2026-10-10T10:00:00",
            "total": "2360.00", "billing": {"first_name": "Ruwan", "last_name": "Jay", "email": "r@x.lk"},
            "tax_lines": [{"rate_id": 5, "label": "VAT", "rate_percent": 18}],
            "line_items": [{"name": "Tea", "sku": "T1", "quantity": 2, "subtotal": "2000.00", "total": "1800.00",
                            "taxes": [{"id": 5, "total": "324.00"}]}],
            "shipping_lines": [{"method_title": "Courier", "total": "200.00", "taxes": [{"id": 5, "total": "36.00"}]}]});
        let ord = order(&o);
        assert_eq!((ord.number.as_str(), ord.customer.as_str(), ord.skip.is_none()), ("#1077", "Ruwan Jay", true));
        assert_eq!(ord.lines[0].discount.cents(), 20000);
        assert_eq!(ord.lines[0].unit_price.cents(), 100000);
        assert_eq!(ord.lines[0].tax_key.as_deref(), Some("VAT|0.18"));
        assert!(ord.lines[1].shipping);
        let pending = order(&json!({"id": 1, "status": "pending", "date_created": "2026-10-10T10:00:00"}));
        assert!(pending.skip.is_some());
    }
}
