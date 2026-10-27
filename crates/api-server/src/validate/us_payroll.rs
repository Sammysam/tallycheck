//! D42 US payroll request bodies: company US settings, an employee's pay and Form W-4, and
//! the paycheck preview. Same rule words as the other DTOs.

use super::hr::{money, to_number, trim_or_null};
use super::{opt_string, Rule, Validator};
use app_core::jsnum::js_string;
use app_core::money::Money;
use app_core::AppError;
use payroll::us::YearToDate;
use payroll::us_setup::{
    percent_to_ppm, PreviewRequest, UsSettingsPatch, UsTax, COUNTRIES, FILING_STATUSES, PAY_FREQUENCIES, PAY_TYPES,
    STATES, W4_VERSIONS,
};
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

pub fn settings(body: Map<String, Value>) -> Result<UsSettingsPatch, AppError> {
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
    })
}

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
    to_number(&mut v, &["allowances"]);
    if let Some(Value::String(s)) = v.input.get_mut("workState") {
        *s = s.trim().to_ascii_uppercase();
    }
    v.field("workState", false, &[Rule::IsString, WORK_STATE])
        .field("payType", false, &[Rule::IsEnum(PAY_TYPES)])
        .field("w4Version", true, &[Rule::IsEnum(W4_VERSIONS)])
        .field("filingStatus", true, &[Rule::IsEnum(FILING_STATUSES)])
        .field("multipleJobs", true, &[Rule::IsBoolean])
        .field("exemptFromWithholding", true, &[Rule::IsBoolean])
        .field("allowances", true, &[Rule::IsInt, Rule::MinZero, Rule::MaxNum(99.0, "99")]);
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
