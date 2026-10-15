//! W5 request DTOs (reference `payroll-runs.dto.ts`): runs, line edits, payment, remittances,
//! company details and rates. Checked against `tests/fixtures/reference_validation.json`.

use super::hr::{money, to_number, trim_or_null};
use super::{opt_string, Rule, Validator};
use app_core::jsnum::js_string;
use app_core::money::Money;
use app_core::AppError;
use payroll::rates::slabs_from_json;
use payroll::runs::{LinePatch, Remit, COMPANY_KEYS, REMITTANCE_TYPES};
use serde_json::{Map, Value};

fn period_pattern(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 7
        && b[..4].iter().all(u8::is_ascii_digit)
        && b[4] == b'-'
        && matches!((b[5], b[6]), (b'0', b'1'..=b'9') | (b'1', b'0'..=b'2'))
}

pub(crate) fn run_rules(v: &mut Validator) {
    trim_or_null(v, &["notes"]);
    v.field("period", false, &[Rule::IsString, Rule::Matches(period_pattern, "period must be YYYY-MM")]).field(
        "notes",
        true,
        &[Rule::IsString],
    );
}

/// `CreateRunDto`: `(period, notes)`.
pub fn create_run(body: Map<String, Value>) -> Result<(String, Option<String>), AppError> {
    let mut v = Validator::new(body);
    run_rules(&mut v);
    let m = v.finish()?;
    Ok((opt_string(&m, "period").unwrap_or_default(), opt_string(&m, "notes")))
}

const LINE_AMOUNTS: &[&str] = &[
    "daysWorked",
    "projectBonus",
    "arrears",
    "medical",
    "otherEarnings",
    "noPayAmount",
    "welfare",
    "loanRecovery",
    "salaryAdvance",
    "phoneBill",
    "otherDeductions",
];
const AMOUNT: &[Rule] = &[Rule::IsMoneyNumber, Rule::MinZero];

pub(crate) fn line_rules(v: &mut Validator) {
    to_number(v, LINE_AMOUNTS);
    to_number(v, &["payeOverride"]);
    trim_or_null(v, &["notes"]);
    for k in LINE_AMOUNTS {
        v.field(k, true, AMOUNT);
    }
    v.field("payeOverride", true, AMOUNT).field("notes", true, &[Rule::IsString]);
}

/// `UpdateLineDto`. A null amount means "not changing it" (the reference wrote NULL into a NOT
/// NULL column); a null `payeOverride` clears the override.
pub fn line_patch(body: Map<String, Value>) -> Result<LinePatch, AppError> {
    let mut v = Validator::new(body);
    line_rules(&mut v);
    let m = v.finish()?;
    Ok(LinePatch {
        days_worked: money(&m, "daysWorked")?.map(Money::cents),
        project_bonus: money(&m, "projectBonus")?,
        arrears: money(&m, "arrears")?,
        medical: money(&m, "medical")?,
        other_earnings: money(&m, "otherEarnings")?,
        no_pay_amount: money(&m, "noPayAmount")?,
        welfare: money(&m, "welfare")?,
        loan_recovery: money(&m, "loanRecovery")?,
        salary_advance: money(&m, "salaryAdvance")?,
        phone_bill: money(&m, "phoneBill")?,
        other_deductions: money(&m, "otherDeductions")?,
        paye_override: match m.get("payeOverride") {
            None => None,
            Some(Value::Null) => Some(None),
            Some(_) => Some(money(&m, "payeOverride")?),
        },
        notes: super::nullable(&m, "notes"),
    })
}

pub(crate) fn add_line_rules(v: &mut Validator) {
    v.field("employeeId", false, &[Rule::IsUuid]);
}

pub fn add_line(body: Map<String, Value>) -> Result<String, AppError> {
    let mut v = Validator::new(body);
    add_line_rules(&mut v);
    Ok(opt_string(&v.finish()?, "employeeId").unwrap_or_default())
}

pub(crate) fn pay_rules(v: &mut Validator) {
    v.field("paymentAccountId", false, &[Rule::IsUuid]).field(
        "payDate",
        false,
        &[Rule::IsDateFormat, Rule::IsNotEmpty],
    );
}

/// `PayRunDto`: `(paymentAccountId, payDate)`.
pub fn pay(body: Map<String, Value>) -> Result<(String, String), AppError> {
    let mut v = Validator::new(body);
    pay_rules(&mut v);
    let m = v.finish()?;
    Ok((opt_string(&m, "paymentAccountId").unwrap_or_default(), opt_string(&m, "payDate").unwrap_or_default()))
}

pub(crate) fn remit_rules(v: &mut Validator) {
    to_number(v, &["amount"]);
    trim_or_null(v, &["reference"]);
    v.field("type", false, &[Rule::IsEnum(REMITTANCE_TYPES)])
        .field("paymentAccountId", false, &[Rule::IsUuid])
        .field("date", false, &[Rule::IsDateFormat, Rule::IsNotEmpty])
        .field("amount", true, &[Rule::IsMoneyNumber, Rule::MinNum(0.01, "0.01")])
        .field("reference", true, &[Rule::IsString, Rule::MaxLength(100)]);
}

pub fn remit(body: Map<String, Value>) -> Result<Remit, AppError> {
    let mut v = Validator::new(body);
    remit_rules(&mut v);
    let m = v.finish()?;
    Ok(Remit {
        kind: opt_string(&m, "type").unwrap_or_default(),
        payment_account_id: opt_string(&m, "paymentAccountId").unwrap_or_default(),
        date: opt_string(&m, "date").unwrap_or_default(),
        amount: money(&m, "amount")?,
        reference: opt_string(&m, "reference"),
    })
}

const COMPANY_LIMITS: &[usize] = &[120, 40, 40, 120, 200, 200, 60, 120];

pub(crate) fn company_rules(v: &mut Validator) {
    trim_or_null(v, COMPANY_KEYS);
    for (k, max) in COMPANY_KEYS.iter().zip(COMPANY_LIMITS) {
        v.field(k, true, &[Rule::IsString, Rule::MaxLength(*max)]);
    }
}

/// `CompanyDetailsDto`: the sent fields; `None` clears one.
pub fn company(body: Map<String, Value>) -> Result<Vec<(String, Option<String>)>, AppError> {
    let mut v = Validator::new(body);
    company_rules(&mut v);
    let m = v.finish()?;
    Ok(COMPANY_KEYS
        .iter()
        .filter_map(|k| m.get(*k).map(|val| ((*k).to_string(), val.as_str().map(str::to_string))))
        .collect())
}

const RATE: &[Rule] = &[Rule::IsFiniteNumber, Rule::MinZero, Rule::MaxNum(1.0, "1")];

pub(crate) fn rates_rules(v: &mut Validator) {
    to_number(v, &["epfEmployeeRate", "epfEmployerRate", "etfRate", "stampDuty", "employerWelfare", "daysInMonth"]);
    v.field("epfEmployeeRate", true, RATE)
        .field("epfEmployerRate", true, RATE)
        .field("etfRate", true, RATE)
        .field("stampDuty", true, AMOUNT)
        .field("employerWelfare", true, AMOUNT)
        .field("daysInMonth", true, &[Rule::IsInt, Rule::MinNum(28.0, "28"), Rule::MaxNum(31.0, "31")])
        .field("payeSlabs", true, &[Rule::IsArray]);
}

const SLABS_MESSAGE: &str = "PAYE slabs must be [[upperBound, rate], …] with ascending bounds, rates between 0 and 1, and only the last bound may be null";

/// `UpdateRatesDto` plus the service's slab check: the settings to write, as the reference
/// stores them (`String(value)`; slabs as JSON; a sent null is stored as "null", which reads
/// back as the default).
pub fn rates(body: Map<String, Value>) -> Result<Vec<(&'static str, String)>, AppError> {
    let mut v = Validator::new(body);
    rates_rules(&mut v);
    let m = v.finish()?;
    if let Some(slabs) = m.get("payeSlabs").filter(|s| !s.is_null()) {
        if slabs_from_json(slabs).is_none() {
            return Err(AppError::bad_request(SLABS_MESSAGE));
        }
    }
    let mut out = Vec::new();
    for (field, key) in payroll::rates::RATE_KEYS {
        let Some(value) = m.get(*field) else { continue };
        let text = match (*field, value) {
            (_, Value::Null) => "null".to_string(),
            ("payeSlabs", slabs) => js_json(slabs),
            (_, n) => n.as_f64().map(js_string).unwrap_or_else(|| n.to_string()),
        };
        out.push((*key, text));
    }
    Ok(out)
}

/// `JSON.stringify` of parsed JSON: numbers printed the JavaScript way.
fn js_json(v: &Value) -> String {
    match v {
        Value::Array(items) => format!("[{}]", items.iter().map(js_json).collect::<Vec<_>>().join(",")),
        Value::Number(n) => n.as_f64().map(js_string).unwrap_or_else(|| n.to_string()),
        other => other.to_string(),
    }
}
