//! US payroll state income tax (D43, ported from the web edition's wave U5b): withholding withholding from rules the company enters for each
//! state (Payroll → US payroll → State income tax). The app ships no state's rates: states
//! publish new ones every year and many change mid-year, so the company copies them from its
//! state's withholding guide. The states without an income tax on wages are built in as NONE.
//!
//! One annualized method fits most state guides:
//!   annual wages = state wages this period × pay periods
//!   taxable      = annual wages − standard deduction (single / married) − allowances × deduction
//!   annual tax   = flat rate × taxable, or the progressive brackets on taxable
//!                  − allowances × credit (not below zero)
//!   this period  = annual tax ÷ pay periods, half-up to the cent, + extra withholding
//! State wages are gross less section 125, and less 401(k) unless the state taxes 401(k)
//! deferrals (`excludes_401k` false, e.g. Pennsylvania). Integer cents; rates in ppm.

use crate::us::PPM;
use crate::us_setup::STATES;
use app_core::money::Money;
use app_core::{AppError, AppResult};
use database::Db;
use serde_json::{json, Value};
use sqlx::sqlite::SqliteRow;
use sqlx::{Row, SqliteConnection};

/// States with no income tax on wages (built in; a company can't set rates for them).
pub const NO_INCOME_TAX: &[&str] = &["AK", "FL", "NV", "NH", "SD", "TN", "TX", "WA", "WY"];

pub const METHODS: &[&str] = &["NONE", "FLAT", "BRACKETS"];

/// `(upper bound of annual taxable income in cents, None = no limit; rate in ppm)`, ascending.
pub type Bracket = (Option<i64>, i64);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StateRule {
    pub method: String,
    pub flat_rate: i64,
    pub brackets: Vec<Bracket>,
    /// Brackets for married employees; None = the same as `brackets`.
    pub brackets_married: Option<Vec<Bracket>>,
    pub standard_deduction_single: i64,
    pub standard_deduction_married: i64,
    pub allowance_deduction: i64,
    pub allowance_credit: i64,
    pub excludes_401k: bool,
}

impl StateRule {
    pub fn none() -> StateRule {
        StateRule {
            method: "NONE".into(),
            flat_rate: 0,
            brackets: Vec::new(),
            brackets_married: None,
            standard_deduction_single: 0,
            standard_deduction_married: 0,
            allowance_deduction: 0,
            allowance_credit: 0,
            excludes_401k: true,
        }
    }

    /// As stored (rates in ppm, amounts in cents) — the `stateRule` snapshot on a pay line.
    pub fn to_snapshot(&self) -> Value {
        let b = |v: &[Bracket]| v.iter().map(|(u, r)| json!([u, r])).collect::<Vec<_>>();
        json!({
            "method": self.method,
            "flatRate": self.flat_rate,
            "brackets": b(&self.brackets),
            "bracketsMarried": self.brackets_married.as_deref().map(b),
            "standardDeductionSingle": self.standard_deduction_single,
            "standardDeductionMarried": self.standard_deduction_married,
            "allowanceDeduction": self.allowance_deduction,
            "allowanceCredit": self.allowance_credit,
            "excludes401k": self.excludes_401k,
        })
    }

    pub fn from_snapshot(v: &Value) -> Option<StateRule> {
        let method = v["method"].as_str().filter(|m| METHODS.contains(m))?.to_string();
        let brackets = |x: &Value| -> Option<Vec<Bracket>> {
            x.as_array()?.iter().map(|p| Some((p.get(0)?.as_i64(), p.get(1)?.as_i64()?))).collect()
        };
        Some(StateRule {
            method,
            flat_rate: v["flatRate"].as_i64().unwrap_or(0),
            brackets: brackets(&v["brackets"]).unwrap_or_default(),
            brackets_married: if v["bracketsMarried"].is_null() { None } else { brackets(&v["bracketsMarried"]) },
            standard_deduction_single: v["standardDeductionSingle"].as_i64().unwrap_or(0),
            standard_deduction_married: v["standardDeductionMarried"].as_i64().unwrap_or(0),
            allowance_deduction: v["allowanceDeduction"].as_i64().unwrap_or(0),
            allowance_credit: v["allowanceCredit"].as_i64().unwrap_or(0),
            excludes_401k: v["excludes401k"].as_bool().unwrap_or(true),
        })
    }
}

/// Brackets are valid when upper bounds rise, only the last is open, and rates are 0–100%.
pub fn brackets_valid(b: &[Bracket]) -> bool {
    let mut prev = 0;
    for (i, (upper, rate)) in b.iter().enumerate() {
        if !(0..=PPM).contains(rate) {
            return false;
        }
        match upper {
            None if i == b.len() - 1 => {}
            None => return false,
            Some(u) if *u > prev => prev = *u,
            Some(_) => return false,
        }
    }
    !b.is_empty()
}

/// The employee's state withholding certificate.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StateEmployee {
    pub married: bool,
    pub allowances: i64,
    pub extra: Money,
    pub exempt: bool,
}

fn too_large() -> AppError {
    AppError::bad_request("Amount is too large")
}

/// The wages the state taxes this period.
pub fn state_wages(gross: Money, pretax_401k: Money, pretax_125: Money, rule: &StateRule) -> Money {
    let off = pretax_125.cents() + if rule.excludes_401k { pretax_401k.cents() } else { 0 };
    Money::from_cents((gross.cents() - off).max(0))
}

/// State income tax to withhold this period.
pub fn withholding(wages: Money, periods: i64, rule: &StateRule, e: &StateEmployee) -> AppResult<Money> {
    if periods <= 0 {
        return Err(AppError::bad_request("Pay periods per year must be positive"));
    }
    if e.exempt || rule.method == "NONE" {
        return Ok(Money::ZERO);
    }
    let allowances = i128::from(e.allowances.max(0));
    let std = if e.married { rule.standard_deduction_married } else { rule.standard_deduction_single };
    let taxable = (i128::from(wages.cents().max(0)) * i128::from(periods)
        - i128::from(std.max(0))
        - allowances * i128::from(rule.allowance_deduction.max(0)))
    .max(0);
    // Annual tax in cents × ppm.
    let mut tax: i128 = match rule.method.as_str() {
        "FLAT" => taxable * i128::from(rule.flat_rate),
        _ => {
            let brackets = match (&rule.brackets_married, e.married) {
                (Some(m), true) => m,
                _ => &rule.brackets,
            };
            let mut t: i128 = 0;
            let mut lower: i128 = 0;
            for (upper, rate) in brackets {
                if taxable <= lower {
                    break;
                }
                let top = upper.map(i128::from).map_or(taxable, |u| u.min(taxable));
                t += (top - lower) * i128::from(*rate);
                match upper {
                    Some(u) => lower = i128::from(*u),
                    None => break,
                }
            }
            t
        }
    };
    tax = (tax - allowances * i128::from(rule.allowance_credit.max(0)) * i128::from(PPM)).max(0);
    let d = i128::from(PPM) * i128::from(periods);
    let per_period = i64::try_from((2 * tax + d) / (2 * d)).map_err(|_| too_large())?;
    Money::from_cents(per_period).checked_add(e.extra).ok_or_else(too_large)
}

// ─── Stored rules ───────────────────────────────────────────────────────────

fn brackets_of(v: &Value) -> Vec<Bracket> {
    v.as_array()
        .map(|a| a.iter().filter_map(|p| Some((p.get(0)?.as_i64(), p.get(1)?.as_i64()?))).collect())
        .unwrap_or_default()
}

fn rule_from_row(r: &SqliteRow) -> AppResult<StateRule> {
    let married = crate::us_setup::json_opt(r, "bracketsMarried")?;
    Ok(StateRule {
        method: r.try_get("method")?,
        flat_rate: r.try_get("flatRate")?,
        brackets: brackets_of(&crate::us_setup::json_col(r, "brackets")?),
        brackets_married: married.map(|m| brackets_of(&m)),
        standard_deduction_single: r.try_get("standardDeductionSingle")?,
        standard_deduction_married: r.try_get("standardDeductionMarried")?,
        allowance_deduction: r.try_get("allowanceDeduction")?,
        allowance_credit: r.try_get("allowanceCredit")?,
        excludes_401k: r.try_get("excludes401k")?,
    })
}

/// The rule for a state: NONE for a state without an income tax on wages, the company's rule,
/// or None when the company hasn't entered one yet.
pub async fn load_rule(conn: &mut SqliteConnection, state: &str) -> AppResult<Option<StateRule>> {
    if NO_INCOME_TAX.contains(&state) {
        return Ok(Some(StateRule::none()));
    }
    let row = sqlx::query(r#"SELECT * FROM "us_state_tax_rules" WHERE "state" = ?1"#)
        .bind(state)
        .fetch_optional(&mut *conn)
        .await?;
    row.as_ref().map(rule_from_row).transpose()
}

fn pct(ppm: i64) -> Value {
    crate::us_setup::ppm_to_percent(ppm)
}

fn rule_json(state: &str, r: &StateRule, built_in: bool, notes: Option<String>, updated_at: Option<String>) -> Value {
    let b = |v: &[Bracket]| {
        v.iter()
            .map(|(u, rate)| json!([u.map(|c| Money::from_cents(c).to_json_number()), pct(*rate)]))
            .collect::<Vec<_>>()
    };
    json!({
        "state": state,
        "builtIn": built_in,
        "method": r.method,
        "flatRate": pct(r.flat_rate),
        "brackets": b(&r.brackets),
        "bracketsMarried": r.brackets_married.as_deref().map(b),
        "standardDeductionSingle": Money::from_cents(r.standard_deduction_single).to_json_number(),
        "standardDeductionMarried": Money::from_cents(r.standard_deduction_married).to_json_number(),
        "allowanceDeduction": Money::from_cents(r.allowance_deduction).to_json_number(),
        "allowanceCredit": Money::from_cents(r.allowance_credit).to_json_number(),
        "excludes401k": r.excludes_401k,
        "notes": notes,
        "updatedAt": updated_at,
    })
}

/// `GET /payroll/us/state-taxes`: every state with a rule (built in or entered), and the states
/// employees work in that still need one (`method` null).
pub async fn list_rules(db: &Db) -> AppResult<Value> {
    let mut c = db.read().acquire().await?;
    let rows = sqlx::query(r#"SELECT * FROM "us_state_tax_rules" ORDER BY "state""#).fetch_all(&mut *c).await?;
    let used: Vec<String> = sqlx::query_scalar(r#"SELECT DISTINCT "workState" FROM "employee_us_tax" ORDER BY 1"#)
        .fetch_all(&mut *c)
        .await?;
    let mut out = Vec::new();
    for st in STATES {
        let entered = rows.iter().find(|r| r.try_get::<String, _>("state").ok().as_deref() == Some(*st));
        let employees = used.iter().any(|u| u == st);
        if NO_INCOME_TAX.contains(st) {
            out.push(rule_json(st, &StateRule::none(), true, None, None));
        } else if let Some(r) = entered {
            out.push(rule_json(
                st,
                &rule_from_row(r)?,
                false,
                r.try_get("notes")?,
                Some(r.try_get::<String, _>("updatedAt")?),
            ));
        } else if employees {
            out.push(json!({ "state": st, "builtIn": false, "method": null }));
        }
    }
    Ok(Value::Array(out))
}

/// `PUT /payroll/us/state-taxes/:state`: saves the company's rule for a state.
pub async fn put_rule(db: &Db, state: &str, r: StateRule, notes: Option<String>) -> AppResult<Value> {
    if NO_INCOME_TAX.contains(&state) {
        return Err(AppError::bad_request(format!("{state} has no state income tax on wages")));
    }
    if r.method == "BRACKETS" && !brackets_valid(&r.brackets) {
        return Err(AppError::bad_request(BRACKETS_MESSAGE));
    }
    if r.brackets_married.as_deref().is_some_and(|b| !brackets_valid(b)) {
        return Err(AppError::bad_request(BRACKETS_MESSAGE));
    }
    let b = |v: &[Bracket]| Value::Array(v.iter().map(|(u, rate)| json!([u, rate])).collect()).to_string();
    let mut tx = db.begin().await?;
    sqlx::query(
        r#"INSERT INTO "us_state_tax_rules" ("state","method","flatRate","brackets","bracketsMarried","standardDeductionSingle",
             "standardDeductionMarried","allowanceDeduction","allowanceCredit","excludes401k","notes","updatedAt")
           VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,strftime('%Y-%m-%dT%H:%M:%fZ','now'))
           ON CONFLICT ("state") DO UPDATE SET "method" = excluded."method", "flatRate" = excluded."flatRate",
             "brackets" = excluded."brackets", "bracketsMarried" = excluded."bracketsMarried",
             "standardDeductionSingle" = excluded."standardDeductionSingle",
             "standardDeductionMarried" = excluded."standardDeductionMarried",
             "allowanceDeduction" = excluded."allowanceDeduction", "allowanceCredit" = excluded."allowanceCredit",
             "excludes401k" = excluded."excludes401k", "notes" = excluded."notes", "updatedAt" = strftime('%Y-%m-%dT%H:%M:%fZ','now')"#,
    )
    .bind(state)
    .bind(&r.method)
    .bind(r.flat_rate)
    .bind(b(&r.brackets))
    .bind(r.brackets_married.as_deref().map(b))
    .bind(r.standard_deduction_single)
    .bind(r.standard_deduction_married)
    .bind(r.allowance_deduction)
    .bind(r.allowance_credit)
    .bind(r.excludes_401k)
    .bind(&notes)
    .execute(&mut *tx)
    .await?;
    let row =
        sqlx::query(r#"SELECT * FROM "us_state_tax_rules" WHERE "state" = ?1"#).bind(state).fetch_one(&mut *tx).await?;
    let out = rule_json(
        state,
        &rule_from_row(&row)?,
        false,
        row.try_get("notes")?,
        Some(row.try_get::<String, _>("updatedAt")?),
    );
    tx.commit().await?;
    Ok(out)
}

/// `DELETE /payroll/us/state-taxes/:state`.
pub async fn delete_rule(db: &Db, state: &str) -> AppResult<()> {
    let done =
        sqlx::query(r#"DELETE FROM "us_state_tax_rules" WHERE "state" = ?1"#).bind(state).execute(db.write()).await?;
    if done.rows_affected() == 0 {
        return Err(AppError::NotFound(format!("No state income tax rules for {state}")));
    }
    Ok(())
}

pub const BRACKETS_MESSAGE: &str =
    "brackets must be [[upTo, ratePercent], …] with rising upTo amounts, rates between 0 and 100, and only the last upTo empty";

#[cfg(test)]
mod tests {
    use super::*;

    fn usd(s: &str) -> Money {
        Money::parse_decimal(s).unwrap_or_else(|| panic!("{s}"))
    }

    fn flat(rate: i64) -> StateRule {
        StateRule { method: "FLAT".into(), flat_rate: rate, ..StateRule::none() }
    }

    #[test]
    fn flat_rate_with_deduction_and_allowances() -> AppResult<()> {
        // 4% on (2,000 × 26 − 5,000 − 2 × 1,000) = 45,000 → 1,800 a year → 69.23 a paycheck.
        let mut r = flat(40_000);
        r.standard_deduction_single = 500_000;
        r.allowance_deduction = 100_000;
        let e = StateEmployee { allowances: 2, ..Default::default() };
        assert_eq!(withholding(usd("2000"), 26, &r, &e)?, usd("69.23"));
        // + $10 extra each paycheck.
        assert_eq!(withholding(usd("2000"), 26, &r, &StateEmployee { extra: usd("10"), ..e.clone() })?, usd("79.23"));
        // Exempt: nothing, not even the extra.
        assert_eq!(
            withholding(usd("2000"), 26, &r, &StateEmployee { exempt: true, extra: usd("10"), ..e })?,
            Money::ZERO
        );
        Ok(())
    }

    #[test]
    fn brackets_are_progressive_and_married_can_differ() -> AppResult<()> {
        // 1% to 10,000, 3% to 50,000, 6% above. Monthly 6,000 → 72,000 taxable a year:
        // 100 + 1,200 + 1,320 = 2,620 → 218.33 a month.
        let mut r = StateRule {
            method: "BRACKETS".into(),
            brackets: vec![(Some(1_000_000), 10_000), (Some(5_000_000), 30_000), (None, 60_000)],
            ..StateRule::none()
        };
        let single = StateEmployee::default();
        assert_eq!(withholding(usd("6000"), 12, &r, &single)?, usd("218.33"));
        // Married brackets twice as wide: 72,000 is inside the 3% band:
        // 1% × 20,000 + 3% × 52,000 = 1,760 → 146.67.
        r.brackets_married = Some(vec![(Some(2_000_000), 10_000), (Some(10_000_000), 30_000), (None, 60_000)]);
        let married = StateEmployee { married: true, ..Default::default() };
        assert_eq!(withholding(usd("6000"), 12, &r, &married)?, usd("146.67"));
        Ok(())
    }

    #[test]
    fn credits_reduce_tax_not_below_zero() -> AppResult<()> {
        // 5% of 26,000 = 1,300 a year; 1 allowance credit of 150 → 1,150 → 44.23 biweekly.
        let mut r = flat(50_000);
        r.allowance_credit = 15_000;
        let e = StateEmployee { allowances: 1, ..Default::default() };
        assert_eq!(withholding(usd("1000"), 26, &r, &e)?, usd("44.23"));
        let many = StateEmployee { allowances: 20, ..Default::default() };
        assert_eq!(withholding(usd("1000"), 26, &r, &many)?, Money::ZERO);
        Ok(())
    }

    #[test]
    fn none_and_state_wages() -> AppResult<()> {
        assert_eq!(withholding(usd("5000"), 12, &StateRule::none(), &StateEmployee::default())?, Money::ZERO);
        let r = flat(30_000);
        assert_eq!(state_wages(usd("2000"), usd("100"), usd("50"), &r), usd("1850"));
        let pa = StateRule { excludes_401k: false, ..r };
        assert_eq!(state_wages(usd("2000"), usd("100"), usd("50"), &pa), usd("1950"));
        Ok(())
    }

    #[test]
    fn bracket_checks_and_snapshot() {
        assert!(brackets_valid(&[(Some(100), 10_000), (None, 20_000)]));
        assert!(!brackets_valid(&[]));
        assert!(!brackets_valid(&[(None, 10_000), (Some(100), 20_000)]));
        assert!(!brackets_valid(&[(Some(100), 10_000), (Some(100), 20_000)]));
        assert!(!brackets_valid(&[(Some(100), 1_000_001)]));
        let r = StateRule {
            method: "BRACKETS".into(),
            brackets: vec![(Some(1_000_000), 10_000), (None, 50_000)],
            brackets_married: Some(vec![(None, 40_000)]),
            standard_deduction_married: 900_000,
            allowance_credit: 5_000,
            excludes_401k: false,
            ..StateRule::none()
        };
        assert_eq!(StateRule::from_snapshot(&r.to_snapshot()), Some(r));
        assert_eq!(StateRule::from_snapshot(&json!({"method": "OTHER"})), None);
    }
}
