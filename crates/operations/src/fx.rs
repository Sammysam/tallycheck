//! Foreign-currency documents (D30). The books are kept in the company's home currency; an
//! invoice or bill in another currency keeps its own figures and converts at its exchange rate
//! (home units per one foreign unit, in hundred-millionths). Conversions round half-up to the
//! cent, or to a whole unit for a home currency without decimals (D29).

use crate::overflow;
use app_core::money::{rounding_step, Money};
use app_core::{AppError, AppResult};
use database::Tx;
use ledger::accounts::{self, AccountType};
use sqlx::SqliteConnection;

/// An exchange rate of exactly 1.
pub const RATE_ONE: i64 = 100_000_000;
/// Account the realised exchange differences post to, created the first time it is needed.
const EXCHANGE_CODE: &str = "7900";
const EXCHANGE_NAME: &str = "Foreign Exchange Gain/Loss";

/// `amount` (document currency) in the home currency at `rate`.
pub fn to_base(amount: Money, rate: i64) -> AppResult<Money> {
    if rate == RATE_ONE {
        return Ok(amount);
    }
    let n = i128::from(amount.cents()) * i128::from(rate);
    let d = i128::from(RATE_ONE) * i128::from(rounding_step());
    let (q, r) = (n / d, n % d);
    let units = if 2 * r.abs() >= d { q + n.signum() } else { q };
    i64::try_from(units * i128::from(rounding_step())).map(Money::from_cents).map_err(|_| overflow())
}

/// The company's home currency code (empty until the company is set up).
pub async fn home_currency(conn: &mut SqliteConnection) -> AppResult<String> {
    let code: Option<String> = sqlx::query_scalar(r#"SELECT "value" FROM "settings" WHERE "key" = 'region.currency'"#)
        .fetch_optional(&mut *conn)
        .await?;
    Ok(code.unwrap_or_default().trim().to_ascii_uppercase())
}

/// The currency and rate a document is saved with: `None` and 1 for the home currency.
pub async fn resolve(
    conn: &mut SqliteConnection,
    currency: Option<&str>,
    rate: Option<i64>,
) -> AppResult<(Option<String>, i64)> {
    let code = currency.map(|c| c.trim().to_ascii_uppercase()).filter(|c| !c.is_empty());
    let home = home_currency(conn).await?;
    match code {
        None => Ok((None, RATE_ONE)),
        Some(c) if c == home => Ok((None, RATE_ONE)),
        Some(c) => {
            if app_core::money::THREE_DECIMAL.contains(&c.as_str()) {
                return Err(AppError::bad_request(format!(
                    "{c} has three decimal places, which the app cannot keep yet"
                )));
            }
            let Some(rate) = rate.filter(|r| *r > 0) else {
                return Err(AppError::bad_request(format!("Enter the exchange rate: how much one {c} is in {home}")));
            };
            Ok((Some(c), rate))
        }
    }
}

/// Remembers the last rate used for a currency, so the next document can start with it.
pub async fn remember_rate(tx: &mut Tx, currency: &Option<String>, rate: i64) -> AppResult<()> {
    if let Some(c) = currency {
        database::settings::upsert(tx, &format!("fx.rate.{c}"), &rate_text(rate)).await?;
    }
    Ok(())
}

/// The rate as plain decimal text: 100000000 → "1", 30012345678 → "300.12345678".
pub fn rate_text(rate: i64) -> String {
    let text = format!("{}.{:08}", rate / RATE_ONE, rate % RATE_ONE);
    text.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// The rate as a JSON number.
pub fn rate_json(rate: i64) -> serde_json::Value {
    serde_json::from_str(&rate_text(rate)).unwrap_or(serde_json::Value::Null)
}

/// The id of the account exchange differences post to, creating it the first time.
pub async fn exchange_account(tx: &mut Tx) -> AppResult<String> {
    if let Ok(a) = accounts::find_by_code(tx, EXCHANGE_CODE).await {
        return Ok(a.id);
    }
    accounts::insert_chart(tx, &[(EXCHANGE_CODE, EXCHANGE_NAME, AccountType::Expense, "operating_expense")]).await?;
    Ok(accounts::find_by_code(tx, EXCHANGE_CODE).await?.id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(s: &str) -> Money {
        Money::parse_decimal(s).unwrap_or_default()
    }

    #[test]
    fn conversions_round_half_up_and_rates_print_plainly() -> AppResult<()> {
        // 100.00 at 300.12 = 30,012.00; 0.05 at 0.5 = 0.025 → 0.03; 1.00 at 1 stays 1.00.
        assert_eq!(to_base(m("100"), 30_012_000_000)?, m("30012"));
        assert_eq!(to_base(m("0.05"), 50_000_000)?, m("0.03"));
        assert_eq!(to_base(m("1"), RATE_ONE)?, m("1"));
        assert_eq!(rate_text(30_012_345_678), "300.12345678");
        assert_eq!(rate_text(RATE_ONE), "1");
        assert_eq!(rate_text(670_000), "0.0067");
        Ok(())
    }
}
