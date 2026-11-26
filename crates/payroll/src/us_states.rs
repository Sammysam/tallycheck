//! D63: US payroll in several states.
//!
//! - **Registrations**: the company's details in each state where it has employees: withholding
//!   and unemployment account numbers, the unemployment (SUTA) rate the state assigned it and its
//!   wage base, and optional payable accounts for that state. A work state without a registration
//!   keeps the company-wide unemployment settings (as before D63).
//! - **Reciprocity**: pairs of states whose agreement lets a resident of one who works in the
//!   other pay income tax only to the home state. The company lists them (no pairs ship yet).
//! - **Splitting a paycheck** ([`split`]): each state the employee works in gets its share of the
//!   wages. A share is taxed by the work state — its rule applied to the whole paycheck, times
//!   the share — unless the employee lives there or the two states have reciprocity, in which case
//!   the home state taxes it. When the home state's rule says `residentCredit` DIFFERENCE, it also
//!   takes, on a share taxed elsewhere, its own tax less the other state's tax when that is more.
//!   Extra withholding from the state certificate is added once, to the main work state's tax.
//!   One state at 100% gives exactly the single-state result.

use crate::us_state::{self, StateEmployee, StateRule};
use app_core::money::Money;
use app_core::time::now_iso;
use app_core::{AppError, AppResult};
use database::Db;
use serde_json::{json, Value};
use sqlx::sqlite::SqliteRow;
use sqlx::{Row, SqliteConnection};
use std::collections::BTreeMap;

/// Shares are in hundredths of a percent: 10,000 = the whole paycheck.
pub const WHOLE: i64 = 10_000;

fn too_large() -> AppError {
    AppError::bad_request("Amount is too large")
}

/// `amount × share / 10,000`, half-up.
fn part(amount: Money, share: i64) -> AppResult<Money> {
    let n = i128::from(amount.cents()) * i128::from(share);
    let d = i128::from(WHOLE);
    i64::try_from((2 * n + d).div_euclid(2 * d)).map(Money::from_cents).map_err(|_| too_large())
}

/// The states one paycheck is worked in: the main work state first, with what the other states
/// leave. Refuses shares that are not 0.01–99.99 %, repeat a state, or add up to 100 % or more.
pub fn allocations(work_state: &str, others: &[(String, i64)]) -> AppResult<Vec<(String, i64)>> {
    let mut used = std::collections::BTreeSet::from([work_state.to_string()]);
    let mut rest = WHOLE;
    for (state, share) in others {
        if !(1..WHOLE).contains(share) {
            return Err(AppError::bad_request(format!("The share for {state} must be between 0.01% and 99.99%")));
        }
        if !used.insert(state.clone()) {
            return Err(AppError::bad_request(format!("{state} is listed more than once")));
        }
        rest -= share;
    }
    if rest <= 0 {
        return Err(AppError::bad_request(format!(
            "The other states add up to 100% or more; leave a share for the main work state {work_state}"
        )));
    }
    let mut out = vec![(work_state.to_string(), rest)];
    out.extend(others.iter().cloned());
    Ok(out)
}

/// One state's part of a paycheck: its share, the wages it taxes and the tax withheld for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StateTax {
    pub state: String,
    /// WORK (the work is done there) or RESIDENT (taxed as the employee's home state).
    pub kind: String,
    pub share: i64,
    pub wages: Money,
    pub tax: Money,
}

impl StateTax {
    pub fn to_json(&self) -> Value {
        json!({
            "state": self.state,
            "kind": self.kind,
            "share": self.share,
            "wages": self.wages.cents(),
            "tax": self.tax.cents(),
        })
    }

    pub fn from_json(v: &Value) -> Option<StateTax> {
        Some(StateTax {
            state: v["state"].as_str()?.to_string(),
            kind: v["kind"].as_str().unwrap_or("WORK").to_string(),
            share: v["share"].as_i64().unwrap_or(WHOLE),
            wages: Money::from_cents(v["wages"].as_i64()?),
            tax: Money::from_cents(v["tax"].as_i64()?),
        })
    }
}

/// What a paycheck pays.
pub struct Paycheck {
    pub gross: Money,
    pub pretax_401k: Money,
    pub pretax_125: Money,
    pub periods: i64,
    /// D66: federal income tax and employee FICA on this paycheck (see `PayContext`).
    pub context: us_state::PayContext,
}

/// The rest of the paycheck as state formulas see it (D66).
pub fn context(f: &crate::us::UsFigures) -> AppResult<us_state::PayContext> {
    let fica = f
        .social_security_employee
        .checked_add(f.medicare_employee)
        .and_then(|x| x.checked_add(f.additional_medicare))
        .ok_or_else(too_large)?;
    Ok(us_state::PayContext { federal_income_tax: f.federal_income_tax, fica, fica_ytd: Money::ZERO })
}

/// D71: the paycheck context with the employee's year-to-date social security and Medicare
/// (6.2% and 1.45% of the year-to-date wages, half-up).
pub fn context_with_ytd(f: &crate::us::UsFigures, ytd: &crate::us::YearToDate) -> AppResult<us_state::PayContext> {
    let mut c = context(f)?;
    let part = |wages: Money, ppm: i128| {
        let n = i128::from(wages.cents().max(0)) * ppm;
        i64::try_from((2 * n + 1_000_000) / 2_000_000).unwrap_or(i64::MAX)
    };
    c.fica_ytd =
        Money::from_cents(part(ytd.social_security_wages, 62_000).saturating_add(part(ytd.medicare_wages, 14_500)));
    Ok(c)
}

/// Each state's share, wages and tax for one paycheck (see the module notes). `rules` must hold
/// every state that taxes a share; a missing one is skipped (the caller refuses to post).
pub fn split(
    pay: &Paycheck,
    allocations: &[(String, i64)],
    resident: &str,
    rules: &BTreeMap<String, StateRule>,
    reciprocal: &dyn Fn(&str, &str) -> bool,
    cert: &StateEmployee,
) -> AppResult<Vec<StateTax>> {
    let no_extra = StateEmployee { extra: Money::ZERO, ..cert.clone() };
    // The whole paycheck under a state's rule: (state wages, tax without the extra).
    let whole = |rule: &StateRule| -> AppResult<(Money, Money)> {
        let wages = us_state::state_wages(pay.gross, pay.pretax_401k, pay.pretax_125, rule);
        Ok((wages, us_state::withholding_in(wages, pay.periods, rule, &no_extra, &pay.context)?))
    };
    let mut parts: Vec<StateTax> = Vec::new();
    let mut add = |state: &str, kind: &str, share: i64, wages: Money, tax: Money| -> AppResult<()> {
        match parts.iter_mut().find(|p| p.state == state) {
            Some(p) => {
                p.share += share;
                p.wages = p.wages.checked_add(wages).ok_or_else(too_large)?;
                p.tax = p.tax.checked_add(tax).ok_or_else(too_large)?;
                if kind == "WORK" {
                    p.kind = "WORK".into();
                }
            }
            None => parts.push(StateTax { state: state.into(), kind: kind.into(), share, wages, tax }),
        }
        Ok(())
    };
    for (state, share) in allocations {
        let home_taxes_it = state == resident || reciprocal(state, resident);
        let target = if home_taxes_it { resident } else { state.as_str() };
        let Some(rule) = rules.get(target) else { continue };
        let (wages, tax) = whole(rule)?;
        let (wages, tax) = (part(wages, *share)?, part(tax, *share)?);
        add(target, if target == state { "WORK" } else { "RESIDENT" }, *share, wages, tax)?;
        if !home_taxes_it {
            if let Some(home) = rules.get(resident).filter(|r| r.resident_credit) {
                let (home_wages, home_tax) = whole(home)?;
                let difference = part(home_tax, *share)?.checked_sub(tax).ok_or_else(too_large)?;
                if difference > Money::ZERO {
                    add(resident, "RESIDENT", 0, part(home_wages, *share)?, difference)?;
                }
            }
        }
    }
    // Extra withholding once, on the main work state's tax (none when exempt).
    if !cert.exempt && cert.extra > Money::ZERO {
        if let Some((main, _)) = allocations.first() {
            let target = if main.as_str() == resident || reciprocal(main, resident) { resident } else { main.as_str() };
            if let Some(p) = parts.iter_mut().find(|p| p.state == target) {
                if rules.get(target).is_some_and(|r| r.method != "NONE") {
                    p.tax = p.tax.checked_add(cert.extra).ok_or_else(too_large)?;
                }
            }
        }
    }
    Ok(parts)
}

/// The states whose rules a paycheck needs: every work state, and the home state when it taxes a
/// share (it lives or works there, has reciprocity with a work state) or its rule is unknown yet.
pub fn states_needed(allocations: &[(String, i64)], resident: &str) -> Vec<String> {
    let mut v: Vec<String> = allocations.iter().map(|(s, _)| s.clone()).collect();
    if !v.iter().any(|s| s == resident) {
        v.push(resident.to_string());
    }
    v
}

/// Everything [`split`] needs for one employee, read from the database.
pub struct StatePlan {
    pub allocations: Vec<(String, i64)>,
    pub resident: String,
    pub rules: BTreeMap<String, StateRule>,
    /// States whose income tax rules the company has not entered yet.
    pub missing: Vec<String>,
    pub reciprocity: std::collections::BTreeSet<(String, String)>,
}

impl StatePlan {
    pub fn reciprocal(&self, work: &str, resident: &str) -> bool {
        self.reciprocity.contains(&(work.to_string(), resident.to_string()))
    }

    pub fn split(&self, pay: &Paycheck, cert: &StateEmployee) -> AppResult<Vec<StateTax>> {
        split(pay, &self.allocations, &self.resident, &self.rules, &|w, h| self.reciprocal(w, h), cert)
    }

    /// Every rule used, as stored on a pay line (state → rule snapshot).
    pub fn rules_snapshot(&self) -> Value {
        Value::Object(self.rules.iter().map(|(s, r)| (s.clone(), r.to_snapshot())).collect())
    }
}

/// The plan for someone working in `work_state` (and `others`), living in `resident` (None =
/// the work state), using the rules in force on pay date `date`.
pub async fn plan(
    conn: &mut SqliteConnection,
    work_state: &str,
    others: &[(String, i64)],
    resident: Option<&str>,
    date: &str,
) -> AppResult<StatePlan> {
    let allocations = allocations(work_state, others)?;
    let resident = resident.filter(|r| !r.is_empty()).unwrap_or(work_state).to_string();
    let mut rules = BTreeMap::new();
    let mut missing = Vec::new();
    for s in states_needed(&allocations, &resident) {
        match us_state::load_rule(conn, &s, date).await? {
            Some(r) => {
                rules.insert(s, r);
            }
            None => missing.push(s),
        }
    }
    let reciprocity = reciprocity(conn).await?.into_iter().collect();
    Ok(StatePlan { allocations, resident, rules, missing, reciprocity })
}

/// The same plan from the rules a pay line stored (`stateRules`), for working a line out again.
pub fn plan_from_snapshot(
    allocations: Vec<(String, i64)>,
    resident: String,
    rules: &Value,
    reciprocity: std::collections::BTreeSet<(String, String)>,
) -> StatePlan {
    let mut map = BTreeMap::new();
    let mut missing = Vec::new();
    for s in states_needed(&allocations, &resident) {
        match rules.get(&s).and_then(StateRule::from_snapshot) {
            Some(r) => {
                map.insert(s, r);
            }
            None => missing.push(s),
        }
    }
    StatePlan { allocations, resident, rules: map, missing, reciprocity }
}

// ─── Registrations ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Registration {
    pub state: String,
    pub withholding_account_no: Option<String>,
    pub unemployment_account_no: Option<String>,
    pub suta_rate: i64,
    pub suta_wage_base: Option<Money>,
    pub state_tax_account_id: Option<String>,
    pub suta_account_id: Option<String>,
    /// D67: headcount for the leave programs' small-employer rules, options per program and
    /// an optional payable account for the state's programs.
    pub employees: Option<i64>,
    pub leave_options: Value,
    pub leave_account_id: Option<String>,
    /// D68: (name, rate ppm) billed on the unemployment wages besides `suta_rate`.
    pub suta_surcharges: Vec<(String, i64)>,
}

impl Registration {
    /// The surcharges' total rate (ppm).
    pub fn surcharge_rate(&self) -> i64 {
        self.suta_surcharges.iter().map(|(_, r)| *r).sum()
    }
}

fn registration_from_row(r: &SqliteRow) -> AppResult<Registration> {
    Ok(Registration {
        state: r.try_get("state")?,
        withholding_account_no: r.try_get("withholdingAccountNo")?,
        unemployment_account_no: r.try_get("unemploymentAccountNo")?,
        suta_rate: r.try_get("sutaRate")?,
        suta_wage_base: r.try_get::<Option<i64>, _>("sutaWageBase")?.map(Money::from_cents),
        state_tax_account_id: r.try_get("stateTaxAccountId")?,
        suta_account_id: r.try_get("sutaAccountId")?,
        employees: r.try_get("employees")?,
        leave_options: crate::us_setup::json_col(r, "leaveOptions")?,
        leave_account_id: r.try_get("leaveAccountId")?,
        suta_surcharges: crate::us_setup::json_col(r, "sutaSurcharges")?
            .as_array()
            .map(|a| a.iter().filter_map(|x| Some((x["name"].as_str()?.to_string(), x["rate"].as_i64()?))).collect())
            .unwrap_or_default(),
    })
}

impl Registration {
    pub fn to_json(&self) -> Value {
        json!({
            "state": self.state,
            "withholdingAccountNo": self.withholding_account_no,
            "unemploymentAccountNo": self.unemployment_account_no,
            "sutaRate": crate::us_setup::ppm_to_percent(self.suta_rate),
            "sutaWageBase": self.suta_wage_base.map(Money::to_json_number),
            "stateTaxAccountId": self.state_tax_account_id,
            "sutaAccountId": self.suta_account_id,
            "employees": self.employees,
            "leaveOptions": leave_options_json(&self.leave_options),
            "leaveAccountId": self.leave_account_id,
            "sutaSurcharges": self.suta_surcharges.iter().map(|(n, r)| json!({"name": n, "rate": crate::us_setup::ppm_to_percent(*r)})).collect::<Vec<_>>(),
        })
    }
}

/// Leave options as the API shows them: rates as percentages.
fn leave_options_json(v: &Value) -> Value {
    let mut out = serde_json::Map::new();
    if let Some(o) = v.as_object() {
        for (id, x) in o {
            let mut item = x.clone();
            if let Some(rate) = x["employerRate"].as_i64() {
                item["employerRate"] = crate::us_setup::ppm_to_percent(rate);
            }
            out.insert(id.clone(), item);
        }
    }
    Value::Object(out)
}

pub async fn registration(conn: &mut SqliteConnection, state: &str) -> AppResult<Option<Registration>> {
    let row = sqlx::query(r#"SELECT * FROM "us_state_registrations" WHERE "state" = ?1"#)
        .bind(state)
        .fetch_optional(&mut *conn)
        .await?;
    row.as_ref().map(registration_from_row).transpose()
}

pub async fn registrations(conn: &mut SqliteConnection) -> AppResult<Vec<Registration>> {
    let rows = sqlx::query(r#"SELECT * FROM "us_state_registrations" ORDER BY "state""#).fetch_all(&mut *conn).await?;
    rows.iter().map(registration_from_row).collect()
}

/// The unemployment state, rate and wage base for someone working mainly in `work_state`: that
/// state's registration, or else the company-wide settings (as before D63).
/// D68: the extra employer charges on unemployment wages in `work_state` (ppm; 0 without a
/// registration).
pub async fn surcharge_for(conn: &mut SqliteConnection, work_state: &str) -> AppResult<i64> {
    Ok(registration(conn, work_state).await?.map_or(0, |r| r.surcharge_rate()))
}

pub async fn unemployment_for(
    conn: &mut SqliteConnection,
    settings: &crate::us_setup::UsSettings,
    work_state: &str,
) -> AppResult<(String, i64, Option<Money>)> {
    Ok(match registration(conn, work_state).await? {
        Some(r) => (r.state, r.suta_rate, r.suta_wage_base),
        None => (
            settings.state.clone().unwrap_or_else(|| work_state.to_string()),
            settings.suta_rate,
            settings.suta_wage_base,
        ),
    })
}

/// Checks that an optional per-state payable account is an active liability in the home currency.
async fn check_payable(conn: &mut SqliteConnection, id: &Option<String>, what: &str) -> AppResult<()> {
    if let Some(id) = id {
        let a = ledger::accounts::find_one(conn, id).await?;
        if !a.is_active || a.account_type != ledger::accounts::AccountType::Liability || a.currency.is_some() {
            return Err(AppError::bad_request(format!(
                "The {what} account must be an active liability account in the home currency; {} {} is not",
                a.code, a.name
            )));
        }
    }
    Ok(())
}

pub async fn put_registration(db: &Db, r: Registration) -> AppResult<Value> {
    let mut tx = db.begin().await?;
    check_payable(&mut tx, &r.state_tax_account_id, "state income tax").await?;
    check_payable(&mut tx, &r.suta_account_id, "unemployment").await?;
    check_payable(&mut tx, &r.leave_account_id, "disability and paid leave").await?;
    let now = now_iso();
    sqlx::query(
        r#"INSERT INTO "us_state_registrations" ("state","withholdingAccountNo","unemploymentAccountNo","sutaRate","sutaWageBase",
             "stateTaxAccountId","sutaAccountId","employees","leaveOptions","leaveAccountId","sutaSurcharges","createdAt","updatedAt")
           VALUES (?1,?2,?3,?4,?5,?6,?7,?9,?10,?11,?12,?8,?8)
           ON CONFLICT ("state") DO UPDATE SET "withholdingAccountNo" = excluded."withholdingAccountNo",
             "unemploymentAccountNo" = excluded."unemploymentAccountNo", "sutaRate" = excluded."sutaRate",
             "sutaWageBase" = excluded."sutaWageBase", "stateTaxAccountId" = excluded."stateTaxAccountId",
             "sutaAccountId" = excluded."sutaAccountId", "employees" = excluded."employees",
             "leaveOptions" = excluded."leaveOptions", "leaveAccountId" = excluded."leaveAccountId",
             "sutaSurcharges" = excluded."sutaSurcharges",
             "updatedAt" = excluded."updatedAt""#,
    )
    .bind(&r.state)
    .bind(&r.withholding_account_no)
    .bind(&r.unemployment_account_no)
    .bind(r.suta_rate)
    .bind(r.suta_wage_base.map(Money::cents))
    .bind(&r.state_tax_account_id)
    .bind(&r.suta_account_id)
    .bind(&now)
    .bind(r.employees)
    .bind(r.leave_options.to_string())
    .bind(&r.leave_account_id)
    .bind(Value::Array(r.suta_surcharges.iter().map(|(n, rate)| json!({"name": n, "rate": rate})).collect()).to_string())
    .execute(&mut *tx)
    .await?;
    let saved = registration(&mut tx, &r.state).await?.ok_or_else(|| AppError::internal("registration missing"))?;
    tx.commit().await?;
    Ok(saved.to_json())
}

pub async fn delete_registration(db: &Db, state: &str) -> AppResult<()> {
    let done = sqlx::query(r#"DELETE FROM "us_state_registrations" WHERE "state" = ?1"#)
        .bind(state)
        .execute(db.write())
        .await?;
    if done.rows_affected() == 0 {
        return Err(AppError::NotFound(format!("The company is not registered in {state}")));
    }
    Ok(())
}

// ─── Reciprocity ────────────────────────────────────────────────────────────

pub async fn reciprocity(conn: &mut SqliteConnection) -> AppResult<Vec<(String, String)>> {
    Ok(sqlx::query_as(r#"SELECT "workState", "residentState" FROM "us_state_reciprocity" ORDER BY 1, 2"#)
        .fetch_all(&mut *conn)
        .await?)
}

pub async fn add_reciprocity(db: &Db, work: &str, resident: &str) -> AppResult<()> {
    if work == resident {
        return Err(AppError::bad_request("A reciprocity agreement is between two different states"));
    }
    sqlx::query(
        r#"INSERT INTO "us_state_reciprocity" ("workState","residentState","createdAt") VALUES (?1,?2,?3)
           ON CONFLICT DO NOTHING"#,
    )
    .bind(work)
    .bind(resident)
    .bind(now_iso())
    .execute(db.write())
    .await?;
    Ok(())
}

pub async fn remove_reciprocity(db: &Db, work: &str, resident: &str) -> AppResult<()> {
    let done = sqlx::query(r#"DELETE FROM "us_state_reciprocity" WHERE "workState" = ?1 AND "residentState" = ?2"#)
        .bind(work)
        .bind(resident)
        .execute(db.write())
        .await?;
    if done.rows_affected() == 0 {
        return Err(AppError::NotFound(format!("No reciprocity for {resident} residents working in {work}")));
    }
    Ok(())
}

/// `GET /payroll/us/states`: registrations, reciprocity pairs and the states employees live or
/// work in.
pub async fn overview(db: &Db) -> AppResult<Value> {
    let mut c = db.read().acquire().await?;
    let regs = registrations(&mut c).await?;
    let pairs = reciprocity(&mut c).await?;
    let rows: Vec<(String, Option<String>, String)> =
        sqlx::query_as(r#"SELECT "workState", "residentState", "otherWorkStates" FROM "employee_us_tax""#)
            .fetch_all(&mut *c)
            .await?;
    let mut used = std::collections::BTreeSet::new();
    for (work, home, others) in rows {
        used.insert(work);
        if let Some(h) = home {
            used.insert(h);
        }
        for (s, _) in parse_others(&others) {
            used.insert(s);
        }
    }
    used.remove("");
    Ok(json!({
        "registrations": regs.iter().map(Registration::to_json).collect::<Vec<_>>(),
        "reciprocity": pairs.iter().map(|(w, r)| json!({"workState": w, "residentState": r})).collect::<Vec<_>>(),
        "statesInUse": used.into_iter().collect::<Vec<_>>(),
    }))
}

/// `[["CA", 2500], …]` as stored.
pub fn parse_others(text: &str) -> Vec<(String, i64)> {
    serde_json::from_str::<Value>(text)
        .ok()
        .and_then(|v| {
            v.as_array().map(|a| {
                a.iter().filter_map(|p| Some((p.get(0)?.as_str()?.to_string(), p.get(1)?.as_i64()?))).collect()
            })
        })
        .unwrap_or_default()
}

pub fn others_json(others: &[(String, i64)]) -> String {
    Value::Array(others.iter().map(|(s, share)| json!([s, share])).collect()).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn usd(s: &str) -> Money {
        Money::parse_decimal(s).unwrap_or_else(|| panic!("{s}"))
    }

    fn flat(rate_ppm: i64) -> StateRule {
        StateRule { method: "FLAT".into(), flat_rate: rate_ppm, ..StateRule::none() }
    }

    fn pay(gross: &str) -> Paycheck {
        Paycheck {
            gross: usd(gross),
            pretax_401k: Money::ZERO,
            pretax_125: Money::ZERO,
            periods: 26,
            context: us_state::PayContext::default(),
        }
    }

    fn rules(list: &[(&str, StateRule)]) -> BTreeMap<String, StateRule> {
        list.iter().map(|(s, r)| (s.to_string(), r.clone())).collect()
    }

    const NOT: &dyn Fn(&str, &str) -> bool = &|_, _| false;

    #[test]
    fn one_state_is_the_single_state_result() -> AppResult<()> {
        // 4% of 2,000 = 80.00, + 10.00 extra.
        let r = rules(&[("CO", flat(40_000))]);
        let cert = StateEmployee { extra: usd("10"), ..Default::default() };
        let got = split(&pay("2000"), &allocations("CO", &[])?, "CO", &r, NOT, &cert)?;
        let single = us_state::withholding(usd("2000"), 26, &r["CO"], &cert)?;
        assert_eq!(got.len(), 1);
        assert_eq!((got[0].state.as_str(), got[0].tax, got[0].wages), ("CO", single, usd("2000")));
        assert_eq!(single, usd("90"));
        Ok(())
    }

    #[test]
    fn shares_are_taxed_by_each_work_state() -> AppResult<()> {
        // 75% Colorado 4%, 25% Utah 5%: 0.75 × 80 = 60.00 and 0.25 × 100 = 25.00.
        let r = rules(&[("CO", flat(40_000)), ("UT", flat(50_000))]);
        let a = allocations("CO", &[("UT".into(), 2_500)])?;
        assert_eq!(a, vec![("CO".to_string(), 7_500), ("UT".to_string(), 2_500)]);
        let got = split(&pay("2000"), &a, "CO", &r, NOT, &StateEmployee::default())?;
        let tax = |s: &str| got.iter().find(|p| p.state == s).map(|p| (p.tax, p.wages, p.kind.clone()));
        assert_eq!(tax("CO"), Some((usd("60"), usd("1500"), "WORK".into())));
        assert_eq!(tax("UT"), Some((usd("25"), usd("500"), "WORK".into())));
        Ok(())
    }

    #[test]
    fn reciprocity_sends_the_share_to_the_home_state() -> AppResult<()> {
        // Lives in PA (3.07%), works in NJ with reciprocity: PA taxes everything, NJ nothing.
        let r = rules(&[("NJ", flat(50_000)), ("PA", StateRule { excludes_401k: false, ..flat(30_700) })]);
        let recip = |w: &str, h: &str| w == "NJ" && h == "PA";
        let got = split(&pay("2000"), &allocations("NJ", &[])?, "PA", &r, &recip, &StateEmployee::default())?;
        assert_eq!(got.len(), 1);
        assert_eq!((got[0].state.as_str(), got[0].kind.as_str(), got[0].tax), ("PA", "RESIDENT", usd("61.40")));
        Ok(())
    }

    #[test]
    fn home_state_takes_the_difference_when_its_rule_says_so() -> AppResult<()> {
        // Lives in a 6% state with DIFFERENCE, works in a 4% state: 80.00 to the work state,
        // 120.00 − 80.00 = 40.00 to the home state. Without DIFFERENCE the home state gets nothing.
        let home = StateRule { resident_credit: true, ..flat(60_000) };
        let r = rules(&[("AA", flat(40_000)), ("BB", home.clone())]);
        let got = split(&pay("2000"), &allocations("AA", &[])?, "BB", &r, NOT, &StateEmployee::default())?;
        let tax = |s: &str| got.iter().find(|p| p.state == s).map(|p| p.tax);
        assert_eq!((tax("AA"), tax("BB")), (Some(usd("80")), Some(usd("40"))));
        // A work state that taxes more leaves no difference.
        let r = rules(&[("AA", flat(70_000)), ("BB", home)]);
        let got = split(&pay("2000"), &allocations("AA", &[])?, "BB", &r, NOT, &StateEmployee::default())?;
        assert_eq!(got.len(), 1);
        let r = rules(&[("AA", flat(40_000)), ("BB", flat(60_000))]);
        let got = split(&pay("2000"), &allocations("AA", &[])?, "BB", &r, NOT, &StateEmployee::default())?;
        assert_eq!(got.len(), 1, "NONE: the work state only");
        Ok(())
    }

    #[test]
    fn shares_are_checked() {
        assert!(allocations("CO", &[("UT".into(), 10_000)]).is_err());
        assert!(allocations("CO", &[("UT".into(), 6_000), ("WY".into(), 4_000)]).is_err());
        assert!(allocations("CO", &[("CO".into(), 1_000)]).is_err());
        assert!(allocations("CO", &[("UT".into(), 0)]).is_err());
        assert_eq!(parse_others(&others_json(&[("UT".into(), 2_500)])), vec![("UT".to_string(), 2_500)]);
    }
}
