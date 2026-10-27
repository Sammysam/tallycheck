//! D42/D43 US payroll request bodies: company US settings, an employee's pay and Form W-4, and
//! the paycheck preview. Same rule words as the other DTOs.

use super::hr::{money, to_number, trim_or_null};
use super::{opt_string, Rule, Validator};
use app_core::ids::is_uuid;
use app_core::jsnum::js_string;
use app_core::money::Money;
use app_core::AppError;
use payroll::us::YearToDate;
use payroll::us_runs::{LinePatch, NewRun, Remit, REMIT_TYPES};
use payroll::us_setup::{
    percent_to_ppm, PreviewRequest, UsSettingsPatch, UsTax, COUNTRIES, FILING_STATUSES, PAY_FREQUENCIES, PAY_TYPES,
    STATES, W4_VERSIONS,
};
use payroll::us_state::{Bracket, StateRule, METHODS};
use serde_json::{Map, Value};

const AMOUNT: &[Rule] = &[Rule::IsMoneyNumber, Rule::MinZero];

fn is_state(s: &str) -> bool {
    STATES.contains(&s)
}

fn is_ein(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 10 && b[2] == b'-' && b.iter().enumerate().all(|(i, c)| i == 2 || c.is_ascii_digit())
}

const STATE: Rule = Rule::Matches(is_state, "state must be a two-letter US state code, like TX");
const WORK_STATE: Rule = Rule::Matches(is_state, "workState must be a two-letter US state code, like TX");

/// A percentage field (already a number) as ppm; at most 4 decimal places.
fn percent(m: &Map<String, Value>, key: &str) -> Result<Option<i64>, AppError> {
    match m.get(key).and_then(Value::as_f64) {
        None => Ok(None),
        Some(x) => percent_to_ppm(&js_string(x))
            .map(Some)
            .ok_or_else(|| AppError::validation(vec![format!("{key} must have at most 4 decimal places")])),
    }
}

/// Sent and null → `Some(None)`; sent text → `Some(Some(text))`; not sent → `None`.
fn nullable_text(m: &Map<String, Value>, key: &str) -> Option<Option<String>> {
    m.get(key).map(|v| v.as_str().map(str::to_string))
}

/// D43: `accounts: {role: accountId | null}` — null goes back to the default code.
fn accounts(v: Option<&Value>) -> Result<Vec<(String, Option<String>)>, AppError> {
    let bad = || AppError::validation(vec!["accounts must map each US payroll role to an account id or null".into()]);
    let Some(v) = v.filter(|v| !v.is_null()) else { return Ok(Vec::new()) };
    let map = v.as_object().ok_or_else(bad)?;
    map.iter()
        .map(|(role, id)| match id {
            Value::Null => Ok((role.clone(), None)),
            Value::String(s) if is_uuid(s) => Ok((role.clone(), Some(s.clone()))),
            _ => Err(bad()),
        })
        .collect()
}

pub fn settings(body: Map<String, Value>) -> Result<UsSettingsPatch, AppError> {
    let accounts = accounts(body.get("accounts"))?;
    let mut v = Validator::new(body);
    to_number(&mut v, &["sutaRate", "sutaWageBase", "futaCreditReduction"]);
    trim_or_null(&mut v, &["state", "ein", "stateAccountNo"]);
    if let Some(Value::String(s)) = v.input.get_mut("state") {
        *s = s.to_ascii_uppercase();
    }
    v.field("country", true, &[Rule::IsEnum(COUNTRIES)])
        .field("payFrequency", true, &[Rule::IsEnum(PAY_FREQUENCIES)])
        .field("state", true, &[Rule::IsString, STATE])
        .field("sutaRate", true, &[Rule::IsFiniteNumber, Rule::MinZero, Rule::MaxNum(100.0, "100")])
        .field("sutaWageBase", true, AMOUNT)
        .field("futaCreditReduction", true, &[Rule::IsFiniteNumber, Rule::MinZero, Rule::MaxNum(5.4, "5.4")])
        .field("ein", true, &[Rule::IsString, Rule::Matches(is_ein, "ein must look like 12-3456789")])
        .field("stateAccountNo", true, &[Rule::IsString, Rule::MaxLength(40)]);
    let m = v.finish()?;
    Ok(UsSettingsPatch {
        country: opt_string(&m, "country"),
        pay_frequency: opt_string(&m, "payFrequency"),
        state: nullable_text(&m, "state"),
        suta_rate: percent(&m, "sutaRate")?,
        suta_wage_base: match m.get("sutaWageBase") {
            None => None,
            Some(Value::Null) => Some(None),
            Some(_) => Some(money(&m, "sutaWageBase")?),
        },
        futa_credit_reduction: percent(&m, "futaCreditReduction")?,
        ein: nullable_text(&m, "ein"),
        state_account_no: nullable_text(&m, "stateAccountNo"),
        accounts,
    })
}

const STATE_FILING: &[&str] = &["SINGLE", "MARRIED"];

const TAX_AMOUNTS: &[&str] = &[
    "annualSalary",
    "hourlyRate",
    "dependentsCredit",
    "otherIncome",
    "deductions",
    "extraWithholding",
    "pretax401k",
    "pretaxSection125",
];

/// The whole US pay and Form W-4 record; anything not sent takes its default.
pub fn tax(body: Map<String, Value>) -> Result<UsTax, AppError> {
    let mut v = Validator::new(body);
    to_number(&mut v, TAX_AMOUNTS);
    to_number(&mut v, &["allowances", "stateAllowances", "stateExtraWithholding"]);
    if let Some(Value::String(s)) = v.input.get_mut("workState") {
        *s = s.trim().to_ascii_uppercase();
    }
    v.field("workState", false, &[Rule::IsString, WORK_STATE])
        .field("payType", false, &[Rule::IsEnum(PAY_TYPES)])
        .field("w4Version", true, &[Rule::IsEnum(W4_VERSIONS)])
        .field("filingStatus", true, &[Rule::IsEnum(FILING_STATUSES)])
        .field("multipleJobs", true, &[Rule::IsBoolean])
        .field("exemptFromWithholding", true, &[Rule::IsBoolean])
        .field("allowances", true, &[Rule::IsInt, Rule::MinZero, Rule::MaxNum(99.0, "99")])
        .field("stateFilingStatus", true, &[Rule::IsEnum(STATE_FILING)])
        .field("stateAllowances", true, &[Rule::IsInt, Rule::MinZero, Rule::MaxNum(99.0, "99")])
        .field("stateExtraWithholding", true, AMOUNT)
        .field("stateExempt", true, &[Rule::IsBoolean]);
    for k in TAX_AMOUNTS {
        v.field(k, true, AMOUNT);
    }
    let m = v.finish()?;
    let amount = |k: &str| -> Result<Money, AppError> { Ok(money(&m, k)?.unwrap_or(Money::ZERO)) };
    let flag = |k: &str| m.get(k).and_then(Value::as_bool).unwrap_or(false);
    let t = UsTax {
        work_state: opt_string(&m, "workState").unwrap_or_default(),
        pay_type: opt_string(&m, "payType").unwrap_or_default(),
        annual_salary: amount("annualSalary")?,
        hourly_rate: amount("hourlyRate")?,
        w4_version: opt_string(&m, "w4Version").unwrap_or_else(|| "2020".into()),
        filing_status: opt_string(&m, "filingStatus").unwrap_or_else(|| "SINGLE".into()),
        multiple_jobs: flag("multipleJobs"),
        dependents_credit: amount("dependentsCredit")?,
        other_income: amount("otherIncome")?,
        deductions: amount("deductions")?,
        extra_withholding: amount("extraWithholding")?,
        allowances: m.get("allowances").and_then(Value::as_i64).unwrap_or(0),
        exempt_from_withholding: flag("exemptFromWithholding"),
        pretax_401k: amount("pretax401k")?,
        pretax_section125: amount("pretaxSection125")?,
        state_filing_status: opt_string(&m, "stateFilingStatus").unwrap_or_else(|| "SINGLE".into()),
        state_allowances: m.get("stateAllowances").and_then(Value::as_i64).unwrap_or(0),
        state_extra_withholding: amount("stateExtraWithholding")?,
        state_exempt: flag("stateExempt"),
    };
    if t.pay_type == "SALARY" && t.annual_salary == Money::ZERO {
        return Err(AppError::bad_request("annualSalary is required for a salaried employee"));
    }
    if t.pay_type == "HOURLY" && t.hourly_rate == Money::ZERO {
        return Err(AppError::bad_request("hourlyRate is required for an hourly employee"));
    }
    Ok(t)
}

const YTD: &[&str] = &["ytdSocialSecurityWages", "ytdMedicareWages", "ytdFutaWages", "ytdSutaWages"];

/// `(employeeId, request)` for the paycheck preview.
pub fn preview(body: Map<String, Value>) -> Result<(String, PreviewRequest), AppError> {
    let mut v = Validator::new(body);
    to_number(&mut v, &["hours", "gross"]);
    to_number(&mut v, YTD);
    v.field("employeeId", false, &[Rule::IsUuid])
        .field("payDate", true, &[Rule::IsDateFormat])
        .field("hours", true, &[Rule::IsQuantity, Rule::MinZero])
        .field("gross", true, AMOUNT);
    for k in YTD {
        v.field(k, true, AMOUNT);
    }
    let m = v.finish()?;
    let ytd = |k: &str| -> Result<Money, AppError> { Ok(money(&m, k)?.unwrap_or(Money::ZERO)) };
    Ok((
        opt_string(&m, "employeeId").unwrap_or_default(),
        PreviewRequest {
            pay_date: opt_string(&m, "payDate"),
            hours: m.get("hours").and_then(Money::from_json),
            gross: money(&m, "gross")?,
            ytd: YearToDate {
                social_security_wages: ytd("ytdSocialSecurityWages")?,
                medicare_wages: ytd("ytdMedicareWages")?,
                futa_wages: ytd("ytdFutaWages")?,
                suta_wages: ytd("ytdSutaWages")?,
            },
        },
    ))
}

// ─── Runs ───────────────────────────────────────────────────────────────────

pub fn create_run(body: Map<String, Value>) -> Result<NewRun, AppError> {
    let mut v = Validator::new(body);
    trim_or_null(&mut v, &["notes"]);
    v.field("periodStart", false, &[Rule::IsDateFormat])
        .field("periodEnd", false, &[Rule::IsDateFormat])
        .field("payDate", false, &[Rule::IsDateFormat])
        .field("notes", true, &[Rule::IsString, Rule::MaxLength(500)]);
    let m = v.finish()?;
    Ok(NewRun {
        period_start: opt_string(&m, "periodStart").unwrap_or_default(),
        period_end: opt_string(&m, "periodEnd").unwrap_or_default(),
        pay_date: opt_string(&m, "payDate").unwrap_or_default(),
        notes: opt_string(&m, "notes"),
    })
}

pub fn add_line(body: Map<String, Value>) -> Result<String, AppError> {
    let mut v = Validator::new(body);
    v.field("employeeId", false, &[Rule::IsUuid]);
    Ok(opt_string(&v.finish()?, "employeeId").unwrap_or_default())
}

const HOURS: &[Rule] = &[Rule::IsQuantity, Rule::MinZero, Rule::MaxNum(744.0, "744")];

pub fn line_patch(body: Map<String, Value>) -> Result<LinePatch, AppError> {
    let mut v = Validator::new(body);
    to_number(&mut v, &["hours", "overtimeHours", "regularPay", "bonus", "otherEarnings"]);
    trim_or_null(&mut v, &["notes"]);
    v.field("hours", true, HOURS)
        .field("overtimeHours", true, HOURS)
        .field("regularPay", true, AMOUNT)
        .field("bonus", true, AMOUNT)
        .field("otherEarnings", true, AMOUNT)
        .field("notes", true, &[Rule::IsString, Rule::MaxLength(500)])
        .field("refresh", true, &[Rule::IsBoolean]);
    let m = v.finish()?;
    let hours = |k: &str| m.get(k).and_then(Money::from_json);
    Ok(LinePatch {
        hours: hours("hours"),
        overtime_hours: hours("overtimeHours"),
        regular_pay: money(&m, "regularPay")?,
        bonus: money(&m, "bonus")?,
        other_earnings: money(&m, "otherEarnings")?,
        notes: nullable_text(&m, "notes"),
        refresh: m.get("refresh").and_then(Value::as_bool).unwrap_or(false),
    })
}

/// `(paymentAccountId, date)`; the date defaults to the run's pay date.
pub fn pay(body: Map<String, Value>) -> Result<(String, Option<String>), AppError> {
    let mut v = Validator::new(body);
    v.field("paymentAccountId", false, &[Rule::IsUuid]).field("date", true, &[Rule::IsDateFormat]);
    let m = v.finish()?;
    Ok((opt_string(&m, "paymentAccountId").unwrap_or_default(), opt_string(&m, "date")))
}

pub fn remit(body: Map<String, Value>) -> Result<Remit, AppError> {
    let mut v = Validator::new(body);
    to_number(&mut v, &["amount"]);
    trim_or_null(&mut v, &["reference"]);
    v.field("type", false, &[Rule::IsEnum(REMIT_TYPES)])
        .field("paymentAccountId", false, &[Rule::IsUuid])
        .field("date", false, &[Rule::IsDateFormat])
        .field("amount", true, &[Rule::IsMoneyNumber, Rule::MinNum(0.01, "0.01")])
        .field("reference", true, &[Rule::IsString, Rule::MaxLength(100)]);
    let m = v.finish()?;
    Ok(Remit {
        kind: opt_string(&m, "type").unwrap_or_default(),
        payment_account_id: opt_string(&m, "paymentAccountId").unwrap_or_default(),
        date: opt_string(&m, "date").unwrap_or_default(),
        amount: money(&m, "amount")?,
        reference: opt_string(&m, "reference"),
    })
}

// ─── Form figures ───────────────────────────────────────────────────────────

/// `?year=2026[&quarter=4]` → (year, quarter); quarter only when `with_quarter`.
pub fn report_period(
    q: &std::collections::HashMap<String, String>,
    with_quarter: bool,
) -> Result<(i32, u32), AppError> {
    let mut errors = Vec::new();
    let year = q.get("year").and_then(|y| y.trim().parse::<i32>().ok()).filter(|y| (2000..=2100).contains(y));
    if year.is_none() {
        errors.push("year must be a year like 2026".to_string());
    }
    let quarter = q.get("quarter").and_then(|v| v.trim().parse::<u32>().ok()).filter(|v| (1..=4).contains(v));
    if with_quarter && quarter.is_none() {
        errors.push("quarter must be 1, 2, 3 or 4".to_string());
    }
    if !errors.is_empty() {
        return Err(AppError::validation(errors));
    }
    Ok((year.unwrap_or(0), quarter.unwrap_or(0)))
}

// ─── State income tax rules ─────────────────────────────────────────────────

/// A two-letter state from the path, upper-cased.
pub fn state_code(raw: &str) -> Result<String, AppError> {
    let s = raw.trim().to_ascii_uppercase();
    if is_state(&s) {
        Ok(s)
    } else {
        Err(AppError::bad_request("state must be a two-letter US state code, like CA"))
    }
}

/// `[[upTo dollars | null, rate percent], …]` → brackets in cents and ppm.
fn brackets(v: &Value) -> Result<Vec<Bracket>, AppError> {
    let bad = || AppError::bad_request(payroll::us_state::BRACKETS_MESSAGE);
    let items = v.as_array().ok_or_else(bad)?;
    let mut out = Vec::new();
    for p in items {
        let pair = p.as_array().filter(|a| a.len() == 2).ok_or_else(bad)?;
        let upper = match &pair[0] {
            Value::Null => None,
            n => Some(Money::from_json(n).ok_or_else(bad)?.cents()),
        };
        let rate = pair[1].as_f64().and_then(|x| percent_to_ppm(&js_string(x))).ok_or_else(bad)?;
        out.push((upper, rate));
    }
    Ok(out)
}

const RULE_AMOUNTS: &[&str] =
    &["standardDeductionSingle", "standardDeductionMarried", "allowanceDeduction", "allowanceCredit"];

/// The whole rule for one state; anything not sent takes its default.
pub fn state_rule(body: Map<String, Value>) -> Result<(StateRule, Option<String>), AppError> {
    let mut v = Validator::new(body);
    to_number(&mut v, &["flatRate"]);
    to_number(&mut v, RULE_AMOUNTS);
    trim_or_null(&mut v, &["notes"]);
    v.field("method", false, &[Rule::IsEnum(METHODS)])
        .field("flatRate", true, &[Rule::IsFiniteNumber, Rule::MinZero, Rule::MaxNum(100.0, "100")])
        .field("brackets", true, &[Rule::IsArray])
        .field("bracketsMarried", true, &[Rule::IsArray])
        .field("excludes401k", true, &[Rule::IsBoolean])
        .field("notes", true, &[Rule::IsString, Rule::MaxLength(500)]);
    for k in RULE_AMOUNTS {
        v.field(k, true, AMOUNT);
    }
    let m = v.finish()?;
    let amount = |k: &str| -> Result<i64, AppError> { Ok(money(&m, k)?.map(Money::cents).unwrap_or(0)) };
    let method = opt_string(&m, "method").unwrap_or_default();
    let rule = StateRule {
        flat_rate: percent(&m, "flatRate")?.unwrap_or(0),
        brackets: match m.get("brackets") {
            Some(b) if !b.is_null() => brackets(b)?,
            _ => Vec::new(),
        },
        brackets_married: match m.get("bracketsMarried") {
            Some(b) if !b.is_null() => Some(brackets(b)?),
            _ => None,
        },
        standard_deduction_single: amount("standardDeductionSingle")?,
        standard_deduction_married: amount("standardDeductionMarried")?,
        allowance_deduction: amount("allowanceDeduction")?,
        allowance_credit: amount("allowanceCredit")?,
        excludes_401k: m.get("excludes401k").and_then(Value::as_bool).unwrap_or(true),
        method,
    };
    if rule.method == "FLAT" && m.get("flatRate").is_none_or(Value::is_null) {
        return Err(AppError::bad_request("flatRate is required for a flat-rate state"));
    }
    Ok((rule, opt_string(&m, "notes")))
}
