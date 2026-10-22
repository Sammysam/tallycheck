//! D40: Stripe balance activity (integrations plan, phase 4).
//!
//! Connect with a secret or restricted key (Stripe Dashboard → Developers → API keys; a restricted
//! key needs read access to Balance, Charges and Payouts). A sync reads the balance transactions
//! and brings in, on the Stripe money account: payments, Stripe's fee on each (suggested as an
//! expense), refunds, and payouts to the bank (suggested as a transfer). Stripe sends amounts in
//! the currency's smallest unit; currencies without decimals (JPY, KRW…) are converted.

use crate::common::{self, text, Config, Connected, Fetched, MoneyKind, MoneyLine};
use crate::Connector;
use app_core::money::Money;
use app_core::{AppError, AppResult};
use chrono::{NaiveDate, TimeZone, Utc};
use serde_json::{json, Map, Value};

const SERVICE: &str = "Stripe";

/// Stripe's zero-decimal currencies (amounts are whole units).
const ZERO_DECIMAL: &[&str] =
    &["BIF", "CLP", "DJF", "GNF", "JPY", "KMF", "KRW", "MGA", "PYG", "RWF", "UGX", "VND", "VUV", "XAF", "XOF", "XPF"];

/// A Stripe amount (smallest unit) as cents.
pub fn to_money(amount: i64, currency: &str) -> Option<Money> {
    if ZERO_DECIMAL.contains(&currency.to_ascii_uppercase().as_str()) {
        amount.checked_mul(100).map(Money::from_cents)
    } else {
        Some(Money::from_cents(amount))
    }
}

fn base() -> String {
    common::base_url(Connector::Stripe, "https://api.stripe.com".into())
}

pub async fn connect(body: &Map<String, Value>) -> AppResult<Connected> {
    let key = body.get("secretKey").and_then(Value::as_str).unwrap_or("").trim().to_string();
    if !(key.starts_with("sk_") || key.starts_with("rk_")) || key.len() < 20 {
        return Err(AppError::bad_request("Paste a secret key (sk_…) or restricted key (rk_…) from Stripe"));
    }
    if key.starts_with("pk_") {
        return Err(AppError::bad_request("That is a publishable key; use a secret or restricted key"));
    }
    let http = common::http()?;
    let (status, _, balance) =
        common::send(SERVICE, http.get(format!("{}/v1/balance", base())).bearer_auth(&key)).await?;
    if status == 401 || status == 403 {
        return Err(common::refused(SERVICE, "key"));
    }
    if !(200..300).contains(&status) {
        return Err(AppError::bad_request(format!("Stripe answered {status}")));
    }
    let currency =
        balance["available"].as_array().and_then(|a| a.first()).map(|b| text(b, "currency").to_ascii_uppercase());
    // The account's name, when the key may read it.
    let (status, _, account) =
        common::send(SERVICE, http.get(format!("{}/v1/account", base())).bearer_auth(&key)).await?;
    let (id, name) = if (200..300).contains(&status) {
        let name =
            [text(&account["settings"]["dashboard"], "display_name"), text(&account["business_profile"], "name")]
                .into_iter()
                .find(|n| !n.is_empty())
                .map(str::to_string);
        (text(&account, "id").to_string(), name)
    } else {
        (String::new(), None)
    };
    let test = key.contains("_test_");
    Ok(Connected {
        remote_id: if id.is_empty() { format!("stripe{}", if test { "-test" } else { "" }) } else { id },
        remote_name: Some(format!(
            "{}{}",
            name.unwrap_or_else(|| "Stripe".into()),
            if test { " (test mode)" } else { "" }
        )),
        remote_currency: currency,
        secret: json!({ "key": key }),
        options: Vec::new(),
    })
}

/// One balance transaction as one or two money lines (its fee apart).
pub fn lines(t: &Value) -> Vec<MoneyLine> {
    let id = text(t, "id");
    let currency = text(t, "currency").to_ascii_uppercase();
    let (Some(created), false) = (t["created"].as_i64(), id.is_empty()) else { return Vec::new() };
    let Some(date) =
        Utc.timestamp_opt(created, 0).single().map(|d| app_core::time::local_date(d).format("%Y-%m-%d").to_string())
    else {
        return Vec::new();
    };
    let amount = t["amount"].as_i64().and_then(|a| to_money(a, &currency)).unwrap_or(Money::ZERO);
    let src = &t["source"];
    let who = [text(&src["billing_details"], "name"), text(src, "receipt_email")]
        .into_iter()
        .find(|n| !n.is_empty())
        .unwrap_or("");
    let what = text(t, "description");
    let (kind, description) = match text(t, "type") {
        "charge" | "payment" => (MoneyKind::Payment, format!("Stripe payment {who} {what}")),
        "refund" | "payment_refund" => (MoneyKind::Refund, format!("Stripe refund {who} {what}")),
        "payout" => (MoneyKind::Payout, "Stripe payout to the bank".to_string()),
        "stripe_fee" | "application_fee" => {
            (MoneyKind::Fee, format!("Stripe {}", if what.is_empty() { "fee" } else { what }))
        }
        other => (MoneyKind::Other, format!("Stripe {} {what}", other.replace('_', " "))),
    };
    let reference =
        Some(text(t, "source").to_string()).filter(|s| !s.is_empty()).or_else(|| Some(text(src, "id").to_string()));
    let mut out = vec![MoneyLine {
        id: format!("txn:{id}"),
        date: date.clone(),
        amount,
        currency: currency.clone(),
        description: description.split_whitespace().collect::<Vec<_>>().join(" "),
        reference: reference.clone().filter(|r| !r.is_empty()),
        kind,
    }];
    if let Some(fee) = t["fee"].as_i64().filter(|f| *f > 0).and_then(|f| to_money(f, &currency)) {
        out.push(MoneyLine {
            id: format!("fee:{id}"),
            date,
            amount: Money::ZERO.checked_sub(fee).unwrap_or(fee),
            currency,
            description: format!("Stripe fee {who}").trim().to_string(),
            reference: reference.filter(|r| !r.is_empty()),
            kind: MoneyKind::Fee,
        });
    }
    out
}

pub async fn fetch(_cfg: &Config, secret: &Value, since: &str) -> AppResult<Fetched> {
    let http = common::http()?;
    let key = secret["key"].as_str().unwrap_or("");
    let from = NaiveDate::parse_from_str(since, "%Y-%m-%d")
        .map_err(AppError::internal)?
        .and_hms_opt(0, 0, 0)
        .map(|d| d.and_utc().timestamp())
        .unwrap_or_default();
    let mut out = Vec::new();
    let mut after: Option<String> = None;
    for _ in 0..500 {
        let mut q: Vec<(&str, String)> =
            vec![("limit", "100".into()), ("created[gte]", from.to_string()), ("expand[]", "data.source".into())];
        if let Some(a) = &after {
            q.push(("starting_after", a.clone()));
        }
        let req = http.get(format!("{}/v1/balance_transactions", base())).bearer_auth(key).query(&q);
        let (status, _, body) = common::send(SERVICE, req).await?;
        if status == 401 || status == 403 {
            return Err(common::refused(SERVICE, "key"));
        }
        if !(200..300).contains(&status) {
            return Err(AppError::bad_request(format!(
                "Stripe answered {status}: {}",
                text(&body["error"], "message")
            )));
        }
        let data = body["data"].as_array().cloned().unwrap_or_default();
        for t in &data {
            out.extend(lines(t));
        }
        after = data.last().map(|t| text(t, "id").to_string());
        if body["has_more"].as_bool() != Some(true) || after.is_none() {
            break;
        }
    }
    Ok(Fetched { orders: Vec::new(), money: Some(out), notes: Vec::new() })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smallest_units_become_cents() {
        assert_eq!(to_money(12345, "usd").map(Money::cents), Some(12345));
        assert_eq!(to_money(500, "JPY").map(Money::cents), Some(50000));
    }

    #[test]
    fn a_charge_with_its_fee_and_a_payout() {
        let c = json!({"id": "txn_1", "type": "charge", "amount": 250000, "fee": 7550, "currency": "lkr", "created": 1791100000,
            "description": "Order 55", "source": {"id": "ch_1", "billing_details": {"name": "Sam Perera"}}});
        let l = lines(&c);
        assert_eq!((l[0].amount.cents(), l[0].kind), (250000, MoneyKind::Payment));
        assert_eq!(l[0].description, "Stripe payment Sam Perera Order 55");
        assert_eq!((l[1].amount.cents(), l[1].kind), (-7550, MoneyKind::Fee));
        let p = json!({"id": "txn_2", "type": "payout", "amount": -242450, "fee": 0, "currency": "lkr", "created": 1791200000, "source": "po_1"});
        assert_eq!(lines(&p)[0].kind, MoneyKind::Payout);
        assert_eq!(lines(&p).len(), 1);
    }
}
