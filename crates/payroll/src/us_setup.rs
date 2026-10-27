//! US payroll (D42, ported from the web edition's wave U2): the company's US payroll settings, each employee's pay and Form W-4
//! (`employee_us_tax`), and a paycheck preview worked out by [`crate::us`].
//!
//! Settings rows: `payroll.country` ('LK' default, or 'US'), and under `payroll.us.`:
//! `payFrequency`, `state`, `sutaRate` and `futaCreditReduction` (percent text, up to 4
//! decimals), `sutaWageBase` (cents, empty = no limit), `ein`, `stateAccountNo`.

use crate::us::{self, FilingStatus, UsInputs, YearToDate, W4};
use app_core::money::Money;
use app_core::time::{now_iso, today_local};
use app_core::{AppError, AppResult};
use database::Db;
use serde_json::{json, Value};
use sqlx::sqlite::SqliteRow;
use sqlx::{Row, SqliteConnection};
use std::collections::BTreeMap;

pub const PAY_FREQUENCIES: &[&str] = &["WEEKLY", "BIWEEKLY", "SEMIMONTHLY", "MONTHLY"];
pub const PAY_TYPES: &[&str] = &["SALARY", "HOURLY"];
pub const W4_VERSIONS: &[&str] = &["2020", "LEGACY"];
pub const FILING_STATUSES: &[&str] = &["SINGLE", "MARRIED_JOINTLY", "HEAD_OF_HOUSEHOLD"];
pub const COUNTRIES: &[&str] = &["LK", "US"];

/// The US states, DC and the territories that run their own unemployment programs.
pub const STATES: &[&str] = &[
    "AL", "AK", "AZ", "AR", "CA", "CO", "CT", "DE", "DC", "FL", "GA", "HI", "ID", "IL", "IN", "IA", "KS", "KY", "LA",
    "ME", "MD", "MA", "MI", "MN", "MS", "MO", "MT", "NE", "NV", "NH", "NJ", "NM", "NY", "NC", "ND", "OH", "OK", "OR",
    "PA", "RI", "SC", "SD", "TN", "TX", "UT", "VT", "VA", "WA", "WV", "WI", "WY", "PR", "VI",
];

pub fn periods_per_year(frequency: &str) -> i64 {
    match frequency {
        "WEEKLY" => 52,
        "BIWEEKLY" => 26,
        "SEMIMONTHLY" => 24,
        _ => 12,
    }
}

// ─── Percentages ────────────────────────────────────────────────────────────

/// "2.7" (percent, up to 4 decimals, 0–100) → 27_000 ppm.
pub fn percent_to_ppm(text: &str) -> Option<i64> {
    let (int, frac) = text.trim().split_once('.').unwrap_or((text.trim(), ""));
    if int.is_empty() || frac.len() > 4 || !int.bytes().chain(frac.bytes()).all(|b| b.is_ascii_digit()) {
        return None;
    }
    let ppm = int.parse::<i64>().ok()?.checked_mul(10_000)? + format!("{frac:0<4}").parse::<i64>().ok()?;
    (ppm <= us::PPM).then_some(ppm)
}

/// 27_000 ppm → 2.7 as a JSON number.
pub fn ppm_to_percent(ppm: i64) -> Value {
    let text = format!("{}.{:04}", ppm / 10_000, ppm % 10_000);
    let text = text.trim_end_matches('0').trim_end_matches('.');
    serde_json::from_str(text).unwrap_or(Value::Null)
}

// ─── Company settings ───────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsSettings {
    pub country: String,
    pub pay_frequency: String,
    pub state: Option<String>,
    pub suta_rate: i64,
    pub suta_wage_base: Option<Money>,
    pub futa_credit_reduction: i64,
    pub ein: Option<String>,
    pub state_account_no: Option<String>,
}

/// The fields of a settings update; only sent ones change (`None` = not sent).
#[derive(Debug, Default)]
pub struct UsSettingsPatch {
    pub country: Option<String>,
    pub pay_frequency: Option<String>,
    pub state: Option<Option<String>>,
    pub suta_rate: Option<i64>,
    pub suta_wage_base: Option<Option<Money>>,
    pub futa_credit_reduction: Option<i64>,
    pub ein: Option<Option<String>>,
    pub state_account_no: Option<Option<String>>,
}

async fn settings_rows(conn: &mut SqliteConnection) -> AppResult<BTreeMap<String, String>> {
    let rows: Vec<(String, String)> = sqlx::query_as(
        r#"SELECT "key", "value" FROM "settings" WHERE "key" = 'payroll.country' OR "key" LIKE 'payroll.us.%'"#,
    )
    .fetch_all(conn)
    .await?;
    Ok(rows.into_iter().collect())
}

fn settings_from(rows: &BTreeMap<String, String>) -> UsSettings {
    let text = |k: &str| rows.get(k).map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    UsSettings {
        country: text("payroll.country").filter(|c| COUNTRIES.contains(&c.as_str())).unwrap_or_else(|| "LK".into()),
        pay_frequency: text("payroll.us.payFrequency")
            .filter(|f| PAY_FREQUENCIES.contains(&f.as_str()))
            .unwrap_or_else(|| "BIWEEKLY".into()),
        state: text("payroll.us.state").filter(|s| STATES.contains(&s.as_str())),
        suta_rate: text("payroll.us.sutaRate").and_then(|t| percent_to_ppm(&t)).unwrap_or(0),
        suta_wage_base: text("payroll.us.sutaWageBase").and_then(|t| t.parse().ok()).map(Money::from_cents),
        futa_credit_reduction: text("payroll.us.futaCreditReduction").and_then(|t| percent_to_ppm(&t)).unwrap_or(0),
        ein: text("payroll.us.ein"),
        state_account_no: text("payroll.us.stateAccountNo"),
    }
}

impl UsSettings {
    pub fn to_json(&self) -> Value {
        json!({
            "country": self.country,
            "payFrequency": self.pay_frequency,
            "periodsPerYear": periods_per_year(&self.pay_frequency),
            "state": self.state,
            "sutaRate": ppm_to_percent(self.suta_rate),
            "sutaWageBase": self.suta_wage_base.map(Money::to_json_number),
            "futaCreditReduction": ppm_to_percent(self.futa_credit_reduction),
            "ein": self.ein,
            "stateAccountNo": self.state_account_no,
            "taxYears": us::years(),
        })
    }
}

pub async fn load_settings(conn: &mut SqliteConnection) -> AppResult<UsSettings> {
    Ok(settings_from(&settings_rows(conn).await?))
}

pub async fn get_settings(db: &Db) -> AppResult<Value> {
    let mut c = db.read().acquire().await?;
    Ok(load_settings(&mut c).await?.to_json())
}

pub async fn update_settings(db: &Db, p: UsSettingsPatch) -> AppResult<Value> {
    let mut tx = db.begin().await?;
    let mut put: Vec<(&str, String)> = Vec::new();
    let opt = |v: Option<String>| v.unwrap_or_default();
    if let Some(v) = p.country {
        put.push(("payroll.country", v));
    }
    if let Some(v) = p.pay_frequency {
        put.push(("payroll.us.payFrequency", v));
    }
    if let Some(v) = p.state {
        put.push(("payroll.us.state", opt(v)));
    }
    if let Some(v) = p.suta_rate {
        put.push(("payroll.us.sutaRate", ppm_to_percent(v).to_string()));
    }
    if let Some(v) = p.suta_wage_base {
        put.push(("payroll.us.sutaWageBase", v.map(|m| m.cents().to_string()).unwrap_or_default()));
    }
    if let Some(v) = p.futa_credit_reduction {
        put.push(("payroll.us.futaCreditReduction", ppm_to_percent(v).to_string()));
    }
    if let Some(v) = p.ein {
        put.push(("payroll.us.ein", opt(v)));
    }
    if let Some(v) = p.state_account_no {
        put.push(("payroll.us.stateAccountNo", opt(v)));
    }
    for (k, v) in &put {
        database::settings::upsert(&mut tx, k, v).await?;
    }
    let out = load_settings(&mut tx).await?.to_json();
    tx.commit().await?;
    Ok(out)
}

// ─── Employee pay and Form W-4 ──────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsTax {
    pub work_state: String,
    pub pay_type: String,
    pub annual_salary: Money,
    pub hourly_rate: Money,
    pub w4_version: String,
    pub filing_status: String,
    pub multiple_jobs: bool,
    pub dependents_credit: Money,
    pub other_income: Money,
    pub deductions: Money,
    pub extra_withholding: Money,
    pub allowances: i64,
    pub exempt_from_withholding: bool,
    pub pretax_401k: Money,
    pub pretax_section125: Money,
}

impl UsTax {
    /// The Form W-4 as the tax engine reads it.
    pub fn w4(&self) -> W4 {
        let married = self.filing_status == "MARRIED_JOINTLY";
        if self.w4_version == "LEGACY" {
            return W4::Before2020 { married, allowances: self.allowances, extra: self.extra_withholding };
        }
        W4::Since2020 {
            status: match self.filing_status.as_str() {
                "MARRIED_JOINTLY" => FilingStatus::MarriedJointly,
                "HEAD_OF_HOUSEHOLD" => FilingStatus::HeadOfHousehold,
                _ => FilingStatus::Single,
            },
            step2_checkbox: self.multiple_jobs,
            credits: self.dependents_credit,
            other_income: self.other_income,
            deductions: self.deductions,
            extra: self.extra_withholding,
        }
    }

    fn defaults(state: Option<&str>) -> UsTax {
        UsTax {
            work_state: state.unwrap_or("").to_string(),
            pay_type: "SALARY".into(),
            annual_salary: Money::ZERO,
            hourly_rate: Money::ZERO,
            w4_version: "2020".into(),
            filing_status: "SINGLE".into(),
            multiple_jobs: false,
            dependents_credit: Money::ZERO,
            other_income: Money::ZERO,
            deductions: Money::ZERO,
            extra_withholding: Money::ZERO,
            allowances: 0,
            exempt_from_withholding: false,
            pretax_401k: Money::ZERO,
            pretax_section125: Money::ZERO,
        }
    }
}

fn from_row(r: &SqliteRow) -> AppResult<UsTax> {
    let m = |k: &str| -> AppResult<Money> { Ok(Money::from_cents(r.try_get::<i64, _>(k)?)) };
    Ok(UsTax {
        work_state: r.try_get("workState")?,
        pay_type: r.try_get("payType")?,
        annual_salary: m("annualSalary")?,
        hourly_rate: m("hourlyRate")?,
        w4_version: r.try_get("w4Version")?,
        filing_status: r.try_get("filingStatus")?,
        multiple_jobs: r.try_get("multipleJobs")?,
        dependents_credit: m("dependentsCredit")?,
        other_income: m("otherIncome")?,
        deductions: m("deductions")?,
        extra_withholding: m("extraWithholding")?,
        allowances: r.try_get("allowances")?,
        exempt_from_withholding: r.try_get("exemptFromWithholding")?,
        pretax_401k: m("pretax401k")?,
        pretax_section125: m("pretaxSection125")?,
    })
}

/// The employee's saved US pay and W-4, with when it was last changed.
pub async fn load_tax(conn: &mut SqliteConnection, employee_id: &str) -> AppResult<Option<(UsTax, String)>> {
    let row = sqlx::query(r#"SELECT * FROM "employee_us_tax" WHERE "employeeId" = ?1"#)
        .bind(employee_id)
        .fetch_optional(&mut *conn)
        .await?;
    match row {
        Some(r) => Ok(Some((from_row(&r)?, r.try_get("updatedAt")?))),
        None => Ok(None),
    }
}

fn tax_json(employee_id: &str, t: &UsTax, updated_at: Option<String>) -> Value {
    json!({
        "employeeId": employee_id,
        "configured": updated_at.is_some(),
        "workState": (!t.work_state.is_empty()).then_some(&t.work_state),
        "payType": t.pay_type,
        "annualSalary": t.annual_salary.to_json_number(),
        "hourlyRate": t.hourly_rate.to_json_number(),
        "w4Version": t.w4_version,
        "filingStatus": t.filing_status,
        "multipleJobs": t.multiple_jobs,
        "dependentsCredit": t.dependents_credit.to_json_number(),
        "otherIncome": t.other_income.to_json_number(),
        "deductions": t.deductions.to_json_number(),
        "extraWithholding": t.extra_withholding.to_json_number(),
        "allowances": t.allowances,
        "exemptFromWithholding": t.exempt_from_withholding,
        "pretax401k": t.pretax_401k.to_json_number(),
        "pretaxSection125": t.pretax_section125.to_json_number(),
        "updatedAt": updated_at,
    })
}

/// `GET /payroll/employees/:id/us-tax`: the saved details, or the defaults (`configured`: false).
pub async fn get_tax(db: &Db, employee_id: &str) -> AppResult<Value> {
    let mut c = db.read().acquire().await?;
    employees::records::find_one(&mut c, employee_id).await?;
    match load_tax(&mut c, employee_id).await? {
        Some((t, at)) => Ok(tax_json(employee_id, &t, Some(at))),
        None => {
            let s = load_settings(&mut c).await?;
            Ok(tax_json(employee_id, &UsTax::defaults(s.state.as_deref()), None))
        }
    }
}

/// `PUT /payroll/employees/:id/us-tax`: saves the whole record.
pub async fn put_tax(db: &Db, employee_id: &str, t: UsTax) -> AppResult<Value> {
    if t.w4_version == "LEGACY" && t.filing_status == "HEAD_OF_HOUSEHOLD" {
        return Err(AppError::bad_request("Head of household is only on the 2020 and later Form W-4"));
    }
    let mut tx = db.begin().await?;
    employees::records::find_one(&mut tx, employee_id).await?;
    sqlx::query(
        r#"INSERT INTO "employee_us_tax" ("employeeId","workState","payType","annualSalary","hourlyRate","w4Version",
             "filingStatus","multipleJobs","dependentsCredit","otherIncome","deductions","extraWithholding","allowances",
             "exemptFromWithholding","pretax401k","pretaxSection125","createdAt","updatedAt")
           VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?17)
           ON CONFLICT ("employeeId") DO UPDATE SET
             "workState" = excluded."workState", "payType" = excluded."payType",
             "annualSalary" = excluded."annualSalary", "hourlyRate" = excluded."hourlyRate",
             "w4Version" = excluded."w4Version", "filingStatus" = excluded."filingStatus",
             "multipleJobs" = excluded."multipleJobs", "dependentsCredit" = excluded."dependentsCredit",
             "otherIncome" = excluded."otherIncome", "deductions" = excluded."deductions",
             "extraWithholding" = excluded."extraWithholding", "allowances" = excluded."allowances",
             "exemptFromWithholding" = excluded."exemptFromWithholding", "pretax401k" = excluded."pretax401k",
             "pretaxSection125" = excluded."pretaxSection125", "updatedAt" = excluded."updatedAt""#,
    )
    .bind(employee_id)
    .bind(&t.work_state)
    .bind(&t.pay_type)
    .bind(t.annual_salary.cents())
    .bind(t.hourly_rate.cents())
    .bind(&t.w4_version)
    .bind(&t.filing_status)
    .bind(t.multiple_jobs)
    .bind(t.dependents_credit.cents())
    .bind(t.other_income.cents())
    .bind(t.deductions.cents())
    .bind(t.extra_withholding.cents())
    .bind(t.allowances)
    .bind(t.exempt_from_withholding)
    .bind(t.pretax_401k.cents())
    .bind(t.pretax_section125.cents())
    .bind(now_iso())
    .execute(&mut *tx)
    .await?;
    let (saved, at) = load_tax(&mut tx, employee_id).await?.ok_or_else(|| AppError::internal("US tax row missing"))?;
    tx.commit().await?;
    Ok(tax_json(employee_id, &saved, Some(at)))
}

// ─── Paycheck preview ───────────────────────────────────────────────────────

#[derive(Debug, Default)]
pub struct PreviewRequest {
    pub pay_date: Option<String>,
    /// Hours worked this period (hundredths), for hourly employees.
    pub hours: Option<Money>,
    /// Gross pay to use instead of the salary or hours.
    pub gross: Option<Money>,
    pub ytd: YearToDate,
}

fn figures_json(f: &us::UsFigures) -> Value {
    json!({
        "incomeTaxWages": f.income_tax_wages.to_json_number(),
        "federalIncomeTax": f.federal_income_tax.to_json_number(),
        "socialSecurityWages": f.social_security_wages.to_json_number(),
        "socialSecurityEmployee": f.social_security_employee.to_json_number(),
        "socialSecurityEmployer": f.social_security_employer.to_json_number(),
        "medicareWages": f.medicare_wages.to_json_number(),
        "medicareEmployee": f.medicare_employee.to_json_number(),
        "additionalMedicare": f.additional_medicare.to_json_number(),
        "medicareEmployer": f.medicare_employer.to_json_number(),
        "futaWages": f.futa_wages.to_json_number(),
        "futa": f.futa.to_json_number(),
        "sutaWages": f.suta_wages.to_json_number(),
        "suta": f.suta.to_json_number(),
        "employeeTaxes": f.employee_taxes.to_json_number(),
        "netPay": f.net_pay.to_json_number(),
        "employerTaxes": f.employer_taxes.to_json_number(),
        "costToEmployer": f.cost_to_employer.to_json_number(),
    })
}

/// Gross pay for one period: salary / periods (half-up to the cent), or rate × hours.
pub fn period_gross(t: &UsTax, periods: i64, hours: Option<Money>) -> AppResult<Money> {
    if t.pay_type == "HOURLY" {
        let hours = hours.ok_or_else(|| AppError::bad_request("hours is required for an hourly employee"))?;
        return Money::line_amount(hours, t.hourly_rate).ok_or_else(|| AppError::bad_request("Amount is too large"));
    }
    let n = i128::from(t.annual_salary.cents());
    let d = i128::from(periods);
    i64::try_from((2 * n + d) / (2 * d))
        .map(Money::from_cents)
        .map_err(|_| AppError::bad_request("Amount is too large"))
}

/// `POST /payroll/us/preview`: one paycheck for one employee, without saving anything.
pub async fn preview(db: &Db, employee_id: &str, req: PreviewRequest) -> AppResult<Value> {
    let mut c = db.read().acquire().await?;
    let e = employees::records::find_one(&mut c, employee_id).await?;
    let s = load_settings(&mut c).await?;
    let (t, _) = load_tax(&mut c, employee_id)
        .await?
        .ok_or_else(|| AppError::bad_request(format!("Add the US pay and Form W-4 details for {} first", e.name)))?;
    let pay_date = req.pay_date.unwrap_or_else(|| today_local().to_string());
    let year: i32 = pay_date
        .get(..4)
        .and_then(|y| y.parse().ok())
        .ok_or_else(|| AppError::bad_request("payDate must be YYYY-MM-DD"))?;
    let periods = periods_per_year(&s.pay_frequency);
    let gross = match req.gross {
        Some(g) => g,
        None => period_gross(&t, periods, req.hours)?,
    };
    let inputs = UsInputs {
        year,
        periods_per_year: periods,
        gross,
        pretax_income_tax_only: t.pretax_401k,
        pretax_all_taxes: t.pretax_section125,
        w4: t.w4(),
        exempt_from_withholding: t.exempt_from_withholding,
        ytd: req.ytd,
        suta_rate: s.suta_rate,
        suta_wage_base: s.suta_wage_base,
        futa_credit_reduction: s.futa_credit_reduction,
    };
    let figures = us::compute(&inputs)?;
    let mut notes = vec!["State and local income tax are not included yet.".to_string()];
    if s.state.as_deref().is_some_and(|st| st != t.work_state) {
        notes.push(format!(
            "{} works in {}, but state unemployment uses the company's {} rate.",
            e.name,
            t.work_state,
            s.state.as_deref().unwrap_or("")
        ));
    }
    Ok(json!({
        "employeeId": employee_id,
        "name": e.name,
        "payDate": pay_date,
        "payFrequency": s.pay_frequency,
        "periodsPerYear": periods,
        "gross": gross.to_json_number(),
        "pretax401k": t.pretax_401k.to_json_number(),
        "pretaxSection125": t.pretax_section125.to_json_number(),
        "taxes": figures_json(&figures),
        "notes": notes,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentages_round_trip_exactly() {
        assert_eq!(percent_to_ppm("2.7"), Some(27_000));
        assert_eq!(percent_to_ppm("0.6"), Some(6_000));
        assert_eq!(percent_to_ppm("5.4321"), Some(54_321));
        assert_eq!(percent_to_ppm("100"), Some(1_000_000));
        assert_eq!(percent_to_ppm("100.0001"), None);
        assert_eq!(percent_to_ppm("1.23456"), None);
        assert_eq!(percent_to_ppm("-1"), None);
        assert_eq!(ppm_to_percent(27_000), json!(2.7));
        assert_eq!(ppm_to_percent(0), json!(0));
        assert_eq!(ppm_to_percent(54_321), json!(5.4321));
    }

    #[test]
    fn salary_is_split_evenly_per_period() -> AppResult<()> {
        let mut t = UsTax::defaults(Some("TX"));
        t.annual_salary = Money::from_cents(5_200_000);
        assert_eq!(period_gross(&t, 26, None)?, Money::from_cents(200_000));
        t.annual_salary = Money::from_cents(5_000_000);
        assert_eq!(period_gross(&t, 26, None)?, Money::from_cents(192_308)); // 1,923.0769…
        t.pay_type = "HOURLY".into();
        t.hourly_rate = Money::from_cents(2_550);
        assert_eq!(period_gross(&t, 26, Some(Money::from_cents(8_000)))?, Money::from_cents(204_000));
        assert!(period_gross(&t, 26, None).is_err());
        Ok(())
    }
}
