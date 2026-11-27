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
use payroll::us_states::Registration;
use serde_json::{Map, Value};

const AMOUNT: &[Rule] = &[Rule::IsMoneyNumber, Rule::MinZero];

/// D67: leave programs the employee is exempt from, e.g. `["WA_CARES"]`.
fn leave_exempt(v: Option<&Value>) -> Result<Vec<String>, AppError> {
    let Some(v) = v.filter(|v| !v.is_null()) else { return Ok(Vec::new()) };
    let bad = || AppError::bad_request("leaveExempt must be a list of leave programs, like [\"WA_CARES\"]");
    let mut out: Vec<String> = Vec::new();
    for x in v.as_array().ok_or_else(bad)? {
        let id = x.as_str().map(|s| s.trim().to_ascii_uppercase()).ok_or_else(bad)?;
        if !payroll::us_leave::is_program(&id) {
            return Err(bad());
        }
        if !out.contains(&id) {
            out.push(id);
        }
    }
    Ok(out)
}

/// D69: `[{code, resident, allowances, secondAllowances, extra, exempt}]`, at most 6.
fn local_taxes(v: Option<&Value>) -> Result<Vec<payroll::us_local::Assignment>, AppError> {
    let Some(v) = v.filter(|v| !v.is_null()) else { return Ok(Vec::new()) };
    let bad = |m: &str| AppError::bad_request(m.to_string());
    let items = v.as_array().ok_or_else(|| bad("localTaxes must be a list"))?;
    if items.len() > 6 {
        return Err(bad("localTaxes can have at most 6 localities"));
    }
    let mut out: Vec<payroll::us_local::Assignment> = Vec::new();
    for x in items {
        let code = x["code"]
            .as_str()
            .map(|s| s.trim().to_ascii_uppercase())
            .filter(|c| is_locality_code(c))
            .ok_or_else(|| bad("Each local tax needs a locality code (letters, digits and dashes)"))?;
        if out.iter().any(|a| a.code == code) {
            return Err(bad(&format!("{code} is listed twice")));
        }
        let count = |k: &str| -> Result<i64, AppError> {
            match &x[k] {
                Value::Null => Ok(0),
                v => v
                    .as_i64()
                    .filter(|n| (0..=99).contains(n))
                    .ok_or_else(|| bad(&format!("{k} must be a whole number from 0 to 99"))),
            }
        };
        let extra = match &x["extra"] {
            Value::Null => 0,
            e => Money::parse_decimal(&js_string(
                e.as_f64().filter(|f| *f >= 0.0).ok_or_else(|| bad("extra must be an amount"))?,
            ))
            .ok_or_else(|| bad("extra must have at most 2 decimals"))?
            .cents(),
        };
        out.push(payroll::us_local::Assignment {
            code,
            resident: x["resident"].as_bool().unwrap_or(false),
            allowances: count("allowances")?,
            second_allowances: count("secondAllowances")?,
            extra,
            exempt: x["exempt"].as_bool().unwrap_or(false),
        });
    }
    Ok(out)
}

fn is_locality_code(s: &str) -> bool {
    (1..=20).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'-')
}

/// A state form's status code or table letter (D66).
fn is_state_code(s: &str) -> bool {
    (1..=8).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_alphanumeric())
}

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

const STATE_FILING: &[&str] = &["SINGLE", "MARRIED", "HEAD_OF_HOUSEHOLD"];

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
    to_number(
        &mut v,
        &[
            "allowances",
            "stateAllowances",
            "stateExtraWithholding",
            "stateSecondAllowances",
            "stateDeductionOverride",
            "stateAnnualCredit",
            "stateElectedRate",
            "stateAnnualDeduction",
        ],
    );
    for k in ["workState", "residentState", "stateCode"] {
        if let Some(Value::String(s)) = v.input.get_mut(k) {
            *s = s.trim().to_ascii_uppercase();
        }
    }
    for k in ["residentState", "stateCode"] {
        if v.input.get(k).and_then(Value::as_str) == Some("") {
            v.input.insert(k.into(), Value::Null);
        }
    }
    let others = other_work_states(v.input.get("otherWorkStates"))?;
    let leave_exempt = leave_exempt(v.input.get("leaveExempt"))?;
    let local_taxes = local_taxes(v.input.get("localTaxes"))?;
    v.field("workState", false, &[Rule::IsString, WORK_STATE])
        .field(
            "residentState",
            true,
            &[Rule::IsString, Rule::Matches(is_state, "residentState must be a two-letter US state code, like PA")],
        )
        .field("payType", false, &[Rule::IsEnum(PAY_TYPES)])
        .field("w4Version", true, &[Rule::IsEnum(W4_VERSIONS)])
        .field("filingStatus", true, &[Rule::IsEnum(FILING_STATUSES)])
        .field("multipleJobs", true, &[Rule::IsBoolean])
        .field("exemptFromWithholding", true, &[Rule::IsBoolean])
        .field("allowances", true, &[Rule::IsInt, Rule::MinZero, Rule::MaxNum(99.0, "99")])
        .field("stateFilingStatus", true, &[Rule::IsEnum(STATE_FILING)])
        .field("stateAllowances", true, &[Rule::IsInt, Rule::MinZero, Rule::MaxNum(99.0, "99")])
        .field("stateExtraWithholding", true, AMOUNT)
        .field("stateExempt", true, &[Rule::IsBoolean])
        .field("stateSecondAllowances", true, &[Rule::IsInt, Rule::MinZero, Rule::MaxNum(99.0, "99")])
        .field("stateDeductionOverride", true, AMOUNT)
        .field("stateAnnualCredit", true, AMOUNT)
        .field("stateAnnualDeduction", true, AMOUNT)
        .field(
            "stateCode",
            true,
            &[
                Rule::IsString,
                Rule::Matches(is_state_code, "stateCode must be 1 to 8 letters or digits, as on the state form"),
            ],
        )
        .field("stateElectedRate", true, &[Rule::IsFiniteNumber, Rule::MinZero, Rule::MaxNum(100.0, "100")]);
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
        // Head of household is kept as SINGLE plus a flag (the stored status is SINGLE / MARRIED).
        state_filing_status: match opt_string(&m, "stateFilingStatus").as_deref() {
            Some("MARRIED") => "MARRIED".into(),
            _ => "SINGLE".into(),
        },
        state_head_of_household: opt_string(&m, "stateFilingStatus").as_deref() == Some("HEAD_OF_HOUSEHOLD"),
        state_second_allowances: m.get("stateSecondAllowances").and_then(Value::as_i64).unwrap_or(0),
        state_deduction_override: money(&m, "stateDeductionOverride")?,
        state_annual_credit: amount("stateAnnualCredit")?,
        state_elected_rate: percent(&m, "stateElectedRate")?,
        state_annual_deduction: amount("stateAnnualDeduction")?,
        state_code: opt_string(&m, "stateCode"),
        leave_exempt,
        local_taxes,
        state_allowances: m.get("stateAllowances").and_then(Value::as_i64).unwrap_or(0),
        state_extra_withholding: amount("stateExtraWithholding")?,
        state_exempt: flag("stateExempt"),
        resident_state: opt_string(&m, "residentState"),
        other_work_states: others,
    };
    if t.pay_type == "SALARY" && t.annual_salary == Money::ZERO {
        return Err(AppError::bad_request("annualSalary is required for a salaried employee"));
    }
    if t.pay_type == "HOURLY" && t.hourly_rate == Money::ZERO {
        return Err(AppError::bad_request("hourlyRate is required for an hourly employee"));
    }
    Ok(t)
}

/// D63: `otherWorkStates: [{state, percent}]` → shares in hundredths of a percent.
fn other_work_states(v: Option<&Value>) -> Result<Vec<(String, i64)>, AppError> {
    let bad = || {
        AppError::validation(vec![
            "otherWorkStates must be a list of {state, percent} with a two-letter state and a percent like 25 or 12.5"
                .into(),
        ])
    };
    let Some(v) = v.filter(|v| !v.is_null()) else { return Ok(Vec::new()) };
    let items = v.as_array().ok_or_else(bad)?;
    items
        .iter()
        .map(|item| {
            let state =
                item.get("state").and_then(Value::as_str).map(|s| s.trim().to_ascii_uppercase()).ok_or_else(bad)?;
            if !is_state(&state) {
                return Err(bad());
            }
            let share = item.get("percent").and_then(Money::from_json).map(Money::cents).ok_or_else(bad)?;
            Ok((state, share))
        })
        .collect()
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
        .field("reference", true, &[Rule::IsString, Rule::MaxLength(100)])
        .field("state", true, &[Rule::IsString, STATE])
        .field(
            "program",
            true,
            &[
                Rule::IsString,
                Rule::Matches(
                    |s| payroll::us_leave::is_program(&s.to_ascii_uppercase()),
                    "program must be a leave program, like CA_SDI",
                ),
            ],
        )
        .field("locality", true, &[Rule::IsString, Rule::MaxLength(20)]);
    let m = v.finish()?;
    let kind = opt_string(&m, "type").unwrap_or_default();
    // D67: a leave payment names its program, D69 a local tax payment its locality (kept where
    // per-state payments keep the state).
    let program = opt_string(&m, "program").map(|s| s.to_ascii_uppercase());
    if program.is_some() && kind != "STATE_LEAVE" {
        return Err(AppError::bad_request("program only applies to a STATE_LEAVE payment"));
    }
    let locality = opt_string(&m, "locality").map(|s| s.trim().to_ascii_uppercase());
    if locality.is_some() && kind != "LOCAL_TAX" {
        return Err(AppError::bad_request("locality only applies to a LOCAL_TAX payment"));
    }
    if locality.as_deref().is_some_and(|c| !is_locality_code(c)) {
        return Err(AppError::bad_request("locality must be a locality code, like NYC"));
    }
    Ok(Remit {
        state: match kind.as_str() {
            "STATE_LEAVE" => program,
            "LOCAL_TAX" => locality,
            _ => opt_string(&m, "state").map(|s| s.to_ascii_uppercase()),
        },
        kind,
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

// ─── States the company is registered in (D63) ──────────────────────────────

/// `PUT /payroll/us/states/:state`: the company's details in that state.
pub fn registration(state: String, body: Map<String, Value>) -> Result<Registration, AppError> {
    let mut v = Validator::new(body);
    to_number(&mut v, &["sutaRate", "sutaWageBase", "employees"]);
    trim_or_null(&mut v, &["withholdingAccountNo", "unemploymentAccountNo"]);
    let leave_options = leave_options(v.input.get("leaveOptions"))?;
    let suta_surcharges = suta_surcharges(v.input.get("sutaSurcharges"))?;
    v.field("withholdingAccountNo", true, &[Rule::IsString, Rule::MaxLength(40)])
        .field("unemploymentAccountNo", true, &[Rule::IsString, Rule::MaxLength(40)])
        .field("sutaRate", true, &[Rule::IsFiniteNumber, Rule::MinZero, Rule::MaxNum(100.0, "100")])
        .field("sutaWageBase", true, AMOUNT)
        .field("stateTaxAccountId", true, &[Rule::IsUuid])
        .field("sutaAccountId", true, &[Rule::IsUuid])
        .field("employees", true, &[Rule::IsInt, Rule::MinZero, Rule::MaxNum(10_000_000.0, "10000000")])
        .field("leaveAccountId", true, &[Rule::IsUuid]);
    let m = v.finish()?;
    Ok(Registration {
        state,
        withholding_account_no: opt_string(&m, "withholdingAccountNo"),
        unemployment_account_no: opt_string(&m, "unemploymentAccountNo"),
        suta_rate: percent(&m, "sutaRate")?.unwrap_or(0),
        suta_wage_base: money(&m, "sutaWageBase")?,
        state_tax_account_id: opt_string(&m, "stateTaxAccountId"),
        suta_account_id: opt_string(&m, "sutaAccountId"),
        employees: m.get("employees").and_then(Value::as_i64),
        leave_options,
        leave_account_id: opt_string(&m, "leaveAccountId"),
        suta_surcharges,
    })
}

/// D68: `[{"name": "Employment Training Tax (ETT)", "rate": 0.1}, …]` (rate in percent).
fn suta_surcharges(v: Option<&Value>) -> Result<Vec<(String, i64)>, AppError> {
    let bad = |m: &str| AppError::bad_request(m.to_string());
    let Some(v) = v.filter(|v| !v.is_null()) else { return Ok(Vec::new()) };
    let items = v.as_array().ok_or_else(|| bad("sutaSurcharges must be a list"))?;
    if items.len() > 10 {
        return Err(bad("sutaSurcharges can have at most 10 charges"));
    }
    let mut out = Vec::new();
    for x in items {
        let name = x["name"]
            .as_str()
            .map(str::trim)
            .filter(|n| !n.is_empty() && n.chars().count() <= 100)
            .ok_or_else(|| bad("Each unemployment surcharge needs a name (up to 100 characters)"))?;
        let pct = x["rate"]
            .as_f64()
            .filter(|p| p.is_finite() && (0.0..=20.0).contains(p))
            .ok_or_else(|| bad("Each unemployment surcharge rate must be a percentage from 0 to 20"))?;
        let ppm =
            percent_to_ppm(&js_string(pct)).ok_or_else(|| bad("Surcharge rates have at most 4 decimal places"))?;
        out.push((name.to_string(), ppm));
    }
    Ok(out)
}

/// D69: `PUT /payroll/us/localities/{code}`: a locality (rates in percent, amounts in dollars).
pub fn locality(code: String, body: Map<String, Value>) -> Result<payroll::us_local::Locality, AppError> {
    let bad = |m: &str| AppError::bad_request(m.to_string());
    let code = code.trim().to_ascii_uppercase();
    if !is_locality_code(&code) {
        return Err(bad("The locality code must be 1 to 20 letters, digits or dashes, like OH-COLUMBUS"));
    }
    let mut v = Validator::new(body);
    to_number(
        &mut v,
        &[
            "residentRate",
            "nonresidentRate",
            "employerRate",
            "exemption",
            "secondExemption",
            "flatEmployee",
            "flatEmployer",
            "monthlyThreshold",
            "employeeYearCap",
        ],
    );
    if let Some(Value::String(s)) = v.input.get_mut("state") {
        *s = s.trim().to_ascii_uppercase();
    }
    const RATE: &[Rule] = &[Rule::IsFiniteNumber, Rule::MinZero, Rule::MaxNum(20.0, "20")];
    v.field("name", false, &[Rule::IsString, Rule::MaxLength(120)])
        .field("state", false, &[Rule::IsString, STATE])
        .field("method", false, &[Rule::IsEnum(&["PERCENT", "MONTHLY_FLAT", "WEEKLY_FLAT", "PERIOD_FLAT", "FORMULA"])])
        .field("formula", true, &[Rule::IsString, Rule::MaxLength(40)])
        .field("residentRate", true, RATE)
        .field("nonresidentRate", true, RATE)
        .field("employerRate", true, RATE)
        .field("exemption", true, AMOUNT)
        .field("secondExemption", true, AMOUNT)
        .field("flatEmployee", true, AMOUNT)
        .field("flatEmployer", true, AMOUNT)
        .field("monthlyThreshold", true, AMOUNT)
        .field("employeeYearCap", true, AMOUNT)
        .field("includes401k", true, &[Rule::IsBoolean])
        .field("includes125", true, &[Rule::IsBoolean])
        .field("group", true, &[Rule::IsString, Rule::MaxLength(20)])
        .field("accountId", true, &[Rule::IsUuid])
        .field("source", true, &[Rule::IsString, Rule::MaxLength(400)])
        .field("verified", true, &[Rule::IsBoolean]);
    let m = v.finish()?;
    let rate = |k: &str| -> Result<i64, AppError> { Ok(percent(&m, k)?.unwrap_or(0)) };
    let cents = |k: &str| -> Result<i64, AppError> { Ok(money(&m, k)?.map_or(0, Money::cents)) };
    let name = opt_string(&m, "name").unwrap_or_default();
    if name.trim().is_empty() {
        return Err(bad("name should not be empty"));
    }
    Ok(payroll::us_local::Locality {
        code,
        name: name.trim().to_string(),
        state: opt_string(&m, "state").unwrap_or_default(),
        method: opt_string(&m, "method").unwrap_or_default(),
        formula: opt_string(&m, "formula"),
        resident_rate: rate("residentRate")?,
        nonresident_rate: rate("nonresidentRate")?,
        employer_rate: rate("employerRate")?,
        exemption: cents("exemption")?,
        second_exemption: cents("secondExemption")?,
        flat_employee: cents("flatEmployee")?,
        flat_employer: cents("flatEmployer")?,
        monthly_threshold: cents("monthlyThreshold")?,
        employee_year_cap: money(&m, "employeeYearCap")?.map(Money::cents),
        includes_401k: m.get("includes401k").and_then(Value::as_bool).unwrap_or(true),
        includes_125: m.get("includes125").and_then(Value::as_bool).unwrap_or(false),
        group: opt_string(&m, "group").map(|g| g.trim().to_ascii_uppercase()).filter(|g| !g.is_empty()),
        account_id: opt_string(&m, "accountId"),
        source: opt_string(&m, "source").unwrap_or_default(),
        verified: m.get("verified").and_then(Value::as_bool).unwrap_or(true),
    })
}

/// D67: `{"NJ_TDI": {"employerRate": 0.5}, "NY_DBL": {"off": true}, "MN_PL": {"small": true}}`
/// (rates in percent, kept in ppm).
fn leave_options(v: Option<&Value>) -> Result<Value, AppError> {
    let bad = |m: &str| AppError::bad_request(m.to_string());
    let mut out = serde_json::Map::new();
    let Some(v) = v.filter(|v| !v.is_null()) else { return Ok(Value::Object(out)) };
    let o = v.as_object().ok_or_else(|| bad("leaveOptions must be an object"))?;
    for (id, x) in o {
        if !payroll::us_leave::is_program(id) {
            return Err(bad(&format!("leaveOptions: {id} is not a leave program")));
        }
        let x = x.as_object().ok_or_else(|| bad("leaveOptions entries must be objects"))?;
        let mut item = serde_json::Map::new();
        for flag in ["off", "small"] {
            match x.get(flag) {
                None | Some(Value::Null) => {}
                Some(Value::Bool(b)) => {
                    item.insert(flag.into(), Value::Bool(*b));
                }
                _ => return Err(bad(&format!("leaveOptions.{id}.{flag} must be true or false"))),
            }
        }
        match x.get("employerRate") {
            None | Some(Value::Null) => {}
            Some(r) => {
                let pct = r.as_f64().filter(|p| p.is_finite() && (0.0..=100.0).contains(p)).ok_or_else(|| {
                    bad(&format!("leaveOptions.{id}.employerRate must be a percentage from 0 to 100"))
                })?;
                let ppm = percent_to_ppm(&js_string(pct))
                    .ok_or_else(|| bad(&format!("leaveOptions.{id}.employerRate must have at most 4 decimals")))?;
                item.insert("employerRate".into(), Value::from(ppm));
            }
        }
        out.insert(id.clone(), Value::Object(item));
    }
    Ok(Value::Object(out))
}

/// `POST /payroll/us/reciprocity`: `{workState, residentState}`.
pub fn reciprocity(body: Map<String, Value>) -> Result<(String, String), AppError> {
    let mut v = Validator::new(body);
    for k in ["workState", "residentState"] {
        if let Some(Value::String(s)) = v.input.get_mut(k) {
            *s = s.trim().to_ascii_uppercase();
        }
    }
    v.field("workState", false, &[Rule::IsString, WORK_STATE]).field(
        "residentState",
        false,
        &[Rule::IsString, Rule::Matches(is_state, "residentState must be a two-letter US state code, like PA")],
    );
    let m = v.finish()?;
    Ok((opt_string(&m, "workState").unwrap_or_default(), opt_string(&m, "residentState").unwrap_or_default()))
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
            n => Some(
                Money::from_json(n)
                    .filter(|m| m.cents().unsigned_abs() <= app_core::money::MAX_NUMERIC_15_2_CENTS as u64)
                    .ok_or_else(bad)?
                    .cents(),
            ),
        };
        let rate = pair[1].as_f64().and_then(|x| percent_to_ppm(&js_string(x))).ok_or_else(bad)?;
        out.push((upper, rate));
    }
    Ok(out)
}

const RULE_AMOUNTS: &[&str] = &[
    "standardDeductionSingle",
    "standardDeductionMarried",
    "allowanceDeduction",
    "allowanceCredit",
    "secondAllowanceDeduction",
    "secondAllowanceCredit",
];

/// The whole rule for one state; anything not sent takes its default.
pub fn state_rule(body: Map<String, Value>) -> Result<(StateRule, Option<String>), AppError> {
    let mut v = Validator::new(body);
    to_number(&mut v, &["flatRate"]);
    to_number(&mut v, RULE_AMOUNTS);
    to_number(&mut v, &["standardDeductionHeadOfHousehold"]);
    trim_or_null(&mut v, &["notes"]);
    v.field("method", false, &[Rule::IsEnum(METHODS)])
        .field("flatRate", true, &[Rule::IsFiniteNumber, Rule::MinZero, Rule::MaxNum(100.0, "100")])
        .field("brackets", true, &[Rule::IsArray])
        .field("bracketsMarried", true, &[Rule::IsArray])
        .field("excludes401k", true, &[Rule::IsBoolean])
        .field("residentCredit", true, &[Rule::IsEnum(&["NONE", "DIFFERENCE"])])
        .field("rounding", true, &[Rule::IsEnum(&["CENT", "DOLLAR"])])
        .field("standardDeductionHeadOfHousehold", true, AMOUNT)
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
        resident_credit: opt_string(&m, "residentCredit").as_deref() == Some("DIFFERENCE"),
        round_to_dollar: opt_string(&m, "rounding").as_deref() == Some("DOLLAR"),
        standard_deduction_hoh: money(&m, "standardDeductionHeadOfHousehold")?.map(Money::cents),
        second_allowance_deduction: amount("secondAllowanceDeduction")?,
        second_allowance_credit: amount("secondAllowanceCredit")?,
        brackets_hoh: None,
        allowance_wage_limit: None,
        round_annual_to_dollar: false,
        formula: None,
        method,
    };
    if rule.method == "FLAT" && m.get("flatRate").is_none_or(Value::is_null) {
        return Err(AppError::bad_request("flatRate is required for a flat-rate state"));
    }
    Ok((rule, opt_string(&m, "notes")))
}

/// D83 figure names of a row of payroll before TallyCheck (`gross` is required).
const PRIOR_AMOUNTS: &[&str] = &[
    "gross",
    "pretax401k",
    "pretaxSection125",
    "incomeTaxWages",
    "federalIncomeTax",
    "socialSecurityWages",
    "socialSecurityEmployee",
    "socialSecurityEmployer",
    "medicareWages",
    "medicareEmployee",
    "medicareEmployer",
    "additionalMedicareWages",
    "additionalMedicare",
    "futaWages",
    "futa",
    "sutaWages",
    "suta",
    "stateIncomeTaxWages",
    "stateIncomeTax",
];

/// D83: one row of payroll before TallyCheck. Figures left out are worked out by the server.
pub fn prior(body: Map<String, Value>) -> Result<payroll::us_prior::PriorInput, AppError> {
    let mut v = Validator::new(body);
    to_number(&mut v, PRIOR_AMOUNTS);
    trim_or_null(&mut v, &["notes"]);
    v.field("employeeId", false, &[Rule::IsUuid])
        .field("payDate", false, &[Rule::IsDateFormat])
        .field("workState", true, &[Rule::IsString, Rule::MaxLength(2)])
        .field("sutaState", true, &[Rule::IsString, Rule::MaxLength(2)])
        .field("notes", true, &[Rule::IsString, Rule::MaxLength(500)]);
    for k in PRIOR_AMOUNTS {
        v.field(k, *k != "gross", AMOUNT);
    }
    let m = v.finish()?;
    let mo = |k: &str| money(&m, k);
    Ok(payroll::us_prior::PriorInput {
        employee_id: opt_string(&m, "employeeId").unwrap_or_default(),
        pay_date: opt_string(&m, "payDate").unwrap_or_default(),
        work_state: opt_string(&m, "workState"),
        suta_state: opt_string(&m, "sutaState"),
        gross: mo("gross")?.unwrap_or(Money::ZERO),
        pretax_401k: mo("pretax401k")?,
        pretax_125: mo("pretaxSection125")?,
        income_tax_wages: mo("incomeTaxWages")?,
        federal_income_tax: mo("federalIncomeTax")?,
        ss_wages: mo("socialSecurityWages")?,
        ss_employee: mo("socialSecurityEmployee")?,
        ss_employer: mo("socialSecurityEmployer")?,
        medicare_wages: mo("medicareWages")?,
        medicare_employee: mo("medicareEmployee")?,
        medicare_employer: mo("medicareEmployer")?,
        additional_medicare_wages: mo("additionalMedicareWages")?,
        additional_medicare: mo("additionalMedicare")?,
        futa_wages: mo("futaWages")?,
        futa: mo("futa")?,
        suta_wages: mo("sutaWages")?,
        suta: mo("suta")?,
        state_wages: mo("stateIncomeTaxWages")?,
        state_tax: mo("stateIncomeTax")?,
        notes: m.get("notes").and_then(Value::as_str).map(str::to_string),
    })
}
