//! D40: PayPal balance activity (integrations plan, phase 4).
//!
//! Connect with a REST app's client id and secret (developer.paypal.com → Apps & Credentials; the
//! app needs "Transaction search"). A sync reads the Transaction Search API in 31-day windows and
//! brings in, on the PayPal money account: payments received, PayPal's fee on each (suggested as an
//! expense), refunds and reversals, and withdrawals to the bank (suggested as a transfer). Only
//! completed transactions in the account's currency come in. PayPal shows transactions up to
//! about three hours after they happen.

use crate::common::{self, money, text, Config, Connected, Fetched, MoneyKind, MoneyLine};
use crate::Connector;
use app_core::money::Money;
use app_core::{AppError, AppResult};
use chrono::{Days, NaiveDate, Utc};
use serde_json::{json, Map, Value};

const SERVICE: &str = "PayPal";

fn base(environment: &str) -> String {
    common::base_url(
        Connector::PayPal,
        if environment == "sandbox" { "https://api-m.sandbox.paypal.com" } else { "https://api-m.paypal.com" }.into(),
    )
}

async fn token(http: &reqwest::Client, base: &str, id: &str, secret: &str) -> AppResult<String> {
    let req = http
        .post(format!("{base}/v1/oauth2/token"))
        .basic_auth(id, Some(secret))
        .header("Accept", "application/json")
        .form(&[("grant_type", "client_credentials")]);
    let (status, _, body) = common::send(SERVICE, req).await?;
    if status == 401 || status == 403 {
        return Err(common::refused(SERVICE, "client id and secret"));
    }
    body["access_token"]
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| AppError::bad_request(format!("PayPal did not sign in (answered {status})")))
}

pub async fn connect(body: &Map<String, Value>) -> AppResult<Connected> {
    let s = |k: &str| body.get(k).and_then(Value::as_str).unwrap_or("").trim().to_string();
    let environment = if s("environment") == "sandbox" { "sandbox" } else { "live" };
    let (id, secret) = (s("clientId"), s("clientSecret"));
    if id.len() < 10 || secret.len() < 10 {
        return Err(AppError::bad_request("Paste the client id and secret of the PayPal REST app"));
    }
    let http = common::http()?;
    let base = base(environment);
    let token = token(&http, &base, &id, &secret).await?;
    let (status, _, balances) =
        common::send(SERVICE, http.get(format!("{base}/v1/reporting/balances")).bearer_auth(&token)).await?;
    if status == 401 || status == 403 {
        return Err(AppError::bad_request(
            "PayPal signed in, but the app may not read the balance. Turn on \"Transaction search\" for the REST app.",
        ));
    }
    let primary = balances["balances"]
        .as_array()
        .and_then(|a| a.iter().find(|b| b["primary"].as_bool() == Some(true)).or_else(|| a.first()))
        .and_then(|b| b["currency"].as_str())
        .map(str::to_string);
    Ok(Connected {
        remote_id: format!("{environment}:{}", &id[..8.min(id.len())]),
        remote_name: Some(if environment == "sandbox" { "PayPal sandbox".into() } else { "PayPal".into() }),
        remote_currency: primary,
        secret: json!({ "clientId": id, "clientSecret": secret }),
        options: vec![("environment", environment.to_string())],
    })
}

fn payer(t: &Value) -> String {
    let p = &t["payer_info"]["payer_name"];
    let full = [
        text(p, "alternate_full_name").to_string(),
        format!("{} {}", text(p, "given_name"), text(p, "surname")).trim().to_string(),
    ]
    .into_iter()
    .find(|n| !n.is_empty());
    full.or_else(|| Some(text(&t["payer_info"], "email_address").to_string()).filter(|e| !e.is_empty()))
        .unwrap_or_default()
}

/// One PayPal transaction as one or two money lines (its fee apart).
fn lines(t: &Value) -> Vec<MoneyLine> {
    let info = &t["transaction_info"];
    let id = text(info, "transaction_id");
    let date = text(info, "transaction_initiation_date");
    // S = completed; pending, denied and the like are left for a later sync.
    if id.is_empty() || date.len() < 10 || text(info, "transaction_status") != "S" {
        return Vec::new();
    }
    let amount = money(&info["transaction_amount"]["value"]).unwrap_or(Money::ZERO);
    let currency = text(&info["transaction_amount"], "currency_code").to_ascii_uppercase();
    let code = text(info, "transaction_event_code");
    let who = payer(t);
    let invoice = text(info, "invoice_id");
    let (kind, description) = match &code.get(..3).unwrap_or("") {
        &"T00" if amount > Money::ZERO => (MoneyKind::Payment, format!("PayPal payment {who} {invoice}")),
        &"T00" => (MoneyKind::Other, format!("PayPal payment sent {who} {invoice}")),
        &"T11" | &"T12" => (MoneyKind::Refund, format!("PayPal refund {who} {invoice}")),
        &"T04" => (MoneyKind::Payout, "PayPal withdrawal to the bank".to_string()),
        &"T03" => (MoneyKind::Other, "PayPal money added from the bank".to_string()),
        _ => (MoneyKind::Other, format!("PayPal {} {who}", text(info, "transaction_subject"))),
    };
    let reference = Some(if invoice.is_empty() { id.to_string() } else { invoice.to_string() });
    let mut out = vec![MoneyLine {
        id: format!("tx:{id}"),
        date: date[..10].to_string(),
        amount,
        currency: currency.clone(),
        description: description.split_whitespace().collect::<Vec<_>>().join(" "),
        reference: reference.clone(),
        kind,
    }];
    // PayPal gives the fee as a negative amount.
    if let Some(fee) = money(&info["fee_amount"]["value"]).filter(|f| *f != Money::ZERO) {
        out.push(MoneyLine {
            id: format!("fee:{id}"),
            date: date[..10].to_string(),
            amount: fee,
            currency,
            description: format!("PayPal fee {who}").trim().to_string(),
            reference,
            kind: MoneyKind::Fee,
        });
    }
    out
}

pub async fn fetch(cfg: &Config, secret: &Value, since: &str) -> AppResult<Fetched> {
    let http = common::http()?;
    let base = base(cfg.option("environment").unwrap_or("live"));
    let token =
        token(&http, &base, secret["clientId"].as_str().unwrap_or(""), secret["clientSecret"].as_str().unwrap_or(""))
            .await?;
    let mut from = NaiveDate::parse_from_str(since, "%Y-%m-%d").map_err(AppError::internal)?;
    let now = Utc::now();
    let mut money_lines = Vec::new();
    // PayPal answers at most 31 days per request.
    while from <= now.date_naive() {
        let to = from.checked_add_days(Days::new(30)).unwrap_or(from);
        let end = if to >= now.date_naive() {
            now.format("%Y-%m-%dT%H:%M:%SZ").to_string()
        } else {
            format!("{to}T23:59:59Z")
        };
        let mut page = 1;
        loop {
            let req = http.get(format!("{base}/v1/reporting/transactions")).bearer_auth(&token).query(&[
                ("start_date", format!("{from}T00:00:00Z")),
                ("end_date", end.clone()),
                ("fields", "transaction_info,payer_info".into()),
                ("page_size", "500".into()),
                ("page", page.to_string()),
            ]);
            let (status, _, body) = common::send(SERVICE, req).await?;
            if status == 401 || status == 403 {
                return Err(AppError::bad_request(
                    "PayPal may not search transactions for this app. Turn on \"Transaction search\".",
                ));
            }
            if !(200..300).contains(&status) {
                return Err(AppError::bad_request(format!("PayPal answered {status}: {}", text(&body, "message"))));
            }
            for t in body["transaction_details"].as_array().cloned().unwrap_or_default() {
                money_lines.extend(lines(&t));
            }
            let pages = body["total_pages"].as_i64().unwrap_or(1);
            page += 1;
            if page > pages || page > 100 {
                break;
            }
        }
        from = to.checked_add_days(Days::new(1)).unwrap_or(to);
    }
    Ok(Fetched { orders: Vec::new(), money: Some(money_lines), notes: Vec::new() })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_payment_and_its_fee() {
        let t = json!({"transaction_info": {"transaction_id": "9AB", "transaction_event_code": "T0006",
            "transaction_initiation_date": "2026-10-10T05:00:00+0000", "transaction_status": "S", "invoice_id": "INV-77",
            "transaction_amount": {"currency_code": "USD", "value": "100.00"}, "fee_amount": {"currency_code": "USD", "value": "-4.20"}},
            "payer_info": {"payer_name": {"given_name": "Ann", "surname": "Lee"}}});
        let l = lines(&t);
        assert_eq!(l.len(), 2);
        assert_eq!(
            (l[0].amount.cents(), l[0].kind, l[0].description.as_str()),
            (10000, MoneyKind::Payment, "PayPal payment Ann Lee INV-77")
        );
        assert_eq!((l[1].amount.cents(), l[1].kind), (-420, MoneyKind::Fee));
        let w = json!({"transaction_info": {"transaction_id": "W1", "transaction_event_code": "T0400", "transaction_status": "S",
            "transaction_initiation_date": "2026-10-11T05:00:00+0000", "transaction_amount": {"currency_code": "USD", "value": "-95.80"}}});
        assert_eq!(lines(&w)[0].kind, MoneyKind::Payout);
        let pending = json!({"transaction_info": {"transaction_id": "P", "transaction_status": "P", "transaction_initiation_date": "2026-10-11T05:00:00+0000"}});
        assert!(lines(&pending).is_empty());
    }
}
