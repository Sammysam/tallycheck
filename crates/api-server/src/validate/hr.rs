//! W4 request DTOs: employees, employee documents, bank details and salaries. Each mirrors the
//! reference decorators in declaration order, including the class-transformer `@Transform`s
//! (trim / trim-or-null) and `@Type(() => Number)` that run before validation. Checked against
//! `tests/fixtures/reference_validation.json`.

use super::{as_js_number, nullable, opt_string, Rule, Validator};
use app_core::money::{Money, MAX_NUMERIC_15_2_CENTS};
use app_core::AppError;
use employees::docs::{DocFields, CATEGORIES};
use employees::records::{EmployeeFields, EMPLOYMENT_TYPES, STATUSES};
use employees::salaries::{BankFields, SalaryFields};
use serde_json::{Map, Number, Value};

// ─── Transforms (run before validation, like class-transformer) ─────────────

/// `trim`: strings are trimmed, anything else passes unchanged.
fn trim(v: &mut Validator, props: &[&str]) {
    for p in props {
        if let Some(Value::String(s)) = v.input.get_mut(*p) {
            *s = s.trim().to_string();
        }
    }
}

/// `trimOrNull`: strings are trimmed and a blank one becomes null.
fn trim_or_null(v: &mut Validator, props: &[&str]) {
    for p in props {
        if let Some(slot) = v.input.get_mut(*p) {
            if let Value::String(s) = slot {
                let t = s.trim();
                *slot = if t.is_empty() { Value::Null } else { Value::String(t.to_string()) };
            }
        }
    }
}

/// A JS number as JSON: integral values as integers (what `JSON.stringify` prints).
fn js_number_value(x: f64) -> Value {
    if x.fract() == 0.0 && x.abs() < 9_007_199_254_740_992.0 {
        Value::from(x as i64)
    } else {
        Number::from_f64(x).map(Value::Number).unwrap_or(Value::Null)
    }
}

/// `@Type(() => Number)`: strings go through `Number()`, booleans become 1/0, null stays null.
/// NaN and ±Infinity are not JSON; they are kept as a marker string that fails `IsNumber` and
/// `Min` alike.
fn to_number(v: &mut Validator, props: &[&str]) {
    for p in props {
        if let Some(slot) = v.input.get_mut(*p) {
            let n = match &*slot {
                Value::Null => continue,
                Value::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
                other => as_js_number(other),
            };
            *slot = match n {
                Some(x) if x.is_finite() => js_number_value(x),
                _ => Value::String("NaN".into()),
            };
        }
    }
}

const AMOUNT: &[Rule] = &[Rule::IsMoneyNumber, Rule::MinZero];

fn money(m: &Map<String, Value>, key: &str) -> Result<Option<Money>, AppError> {
    match m.get(key).and_then(Money::from_json) {
        Some(x) if x.cents() > MAX_NUMERIC_15_2_CENTS => Err(AppError::bad_request("Amount is too large")),
        other => Ok(other),
    }
}

/// A NOT NULL text column: null (allowed through by `@IsOptional`) means "not sent".
fn required_text(m: &Map<String, Value>, key: &str) -> Option<String> {
    opt_string(m, key)
}

// ─── Employees ──────────────────────────────────────────────────────────────

const EMPLOYEE_ID_REQUIRED: Rule = Rule::Msg(&Rule::IsNotEmpty, "Employee ID is required");
const NAME_REQUIRED: Rule = Rule::Msg(&Rule::IsNotEmpty, "Name is required");
const EMAIL_INVALID: Rule = Rule::Msg(&Rule::IsEmail, "Email is not valid");

/// `CreateEmployeeDto`; with `optional` it is `UpdateEmployeeDto` (`PartialType`).
pub(crate) fn employee_rules(v: &mut Validator, optional: bool) {
    trim(v, &["employeeId", "name"]);
    trim_or_null(
        v,
        &[
            "jobTitle",
            "department",
            "email",
            "phone",
            "joinDate",
            "notes",
            "epfNumber",
            "nationalId",
            "resignedDate",
            "userId",
        ],
    );
    v.field("employeeId", optional, &[Rule::IsString, EMPLOYEE_ID_REQUIRED, Rule::MaxLength(50)])
        .field("name", optional, &[Rule::IsString, NAME_REQUIRED, Rule::MaxLength(200)])
        .field("jobTitle", true, &[Rule::IsString, Rule::MaxLength(200)])
        .field("department", true, &[Rule::IsString, Rule::MaxLength(200)])
        .field("email", true, &[EMAIL_INVALID])
        .field("phone", true, &[Rule::IsString, Rule::MaxLength(50)])
        .field("joinDate", true, &[Rule::IsDateFormat])
        .field("status", true, &[Rule::IsEnum(STATUSES)])
        .field("notes", true, &[Rule::IsString])
        .field("employmentType", true, &[Rule::IsEnum(EMPLOYMENT_TYPES)])
        .field("epfNumber", true, &[Rule::IsString, Rule::MaxLength(50)])
        .field("nationalId", true, &[Rule::IsString, Rule::MaxLength(50)])
        .field("resignedDate", true, &[Rule::IsDateFormat])
        .field("userId", true, &[Rule::IsUuid]);
}

pub fn employee(body: Map<String, Value>, optional: bool) -> Result<EmployeeFields, AppError> {
    let mut v = Validator::new(body);
    employee_rules(&mut v, optional);
    let m = v.finish()?;
    Ok(EmployeeFields {
        employee_id: required_text(&m, "employeeId"),
        name: required_text(&m, "name"),
        job_title: nullable(&m, "jobTitle"),
        department: nullable(&m, "department"),
        email: nullable(&m, "email"),
        phone: nullable(&m, "phone"),
        join_date: nullable(&m, "joinDate"),
        status: required_text(&m, "status"),
        notes: nullable(&m, "notes"),
        employment_type: required_text(&m, "employmentType"),
        epf_number: nullable(&m, "epfNumber"),
        national_id: nullable(&m, "nationalId"),
        resigned_date: nullable(&m, "resignedDate"),
        user_id: nullable(&m, "userId"),
    })
}

pub(crate) fn employee_query_rules(v: &mut Validator) {
    v.field("q", true, &[Rule::IsString]).field("status", true, &[Rule::IsEnum(STATUSES)]);
}

/// `QueryEmployeesDto`: `(q, status)`.
pub fn employee_query(query: Map<String, Value>) -> Result<(Option<String>, Option<String>), AppError> {
    let mut v = Validator::new(query);
    employee_query_rules(&mut v);
    let m = v.finish()?;
    Ok((opt_string(&m, "q"), opt_string(&m, "status")))
}

// ─── Documents ──────────────────────────────────────────────────────────────

const TITLE_REQUIRED: Rule = Rule::Msg(&Rule::IsNotEmpty, "Give the document a title");

/// `UploadDocumentDto`. With `optional` (replace and PATCH, whose bodies the reference left
/// unvalidated) every field may be left out but what is sent must still be valid.
pub(crate) fn document_rules(v: &mut Validator, optional: bool) {
    if optional {
        // NOT NULL columns: a null title or category means "not changing it".
        for k in ["title", "category"] {
            if v.input.get(k).is_some_and(Value::is_null) {
                v.input.remove(k);
            }
        }
    }
    trim(v, &["title"]);
    trim_or_null(v, &["expiryDate", "notes"]);
    v.field("title", optional, &[Rule::IsString, TITLE_REQUIRED, Rule::MaxLength(200)])
        .field("category", true, &[Rule::IsEnum(CATEGORIES)])
        .field("expiryDate", true, &[Rule::IsDateFormat])
        .field("notes", true, &[Rule::IsString]);
}

fn doc_fields(m: &Map<String, Value>) -> DocFields {
    DocFields {
        title: required_text(m, "title"),
        category: required_text(m, "category"),
        expiry_date: nullable(m, "expiryDate"),
        notes: nullable(m, "notes"),
    }
}

/// Text fields of a new upload.
pub fn document(body: Map<String, Value>) -> Result<DocFields, AppError> {
    let mut v = Validator::new(body);
    document_rules(&mut v, false);
    Ok(doc_fields(&v.finish()?))
}

/// Text fields of a replacement upload: a blank title keeps the previous one (reference
/// `dto.title?.trim() || previous.title`).
pub fn document_replace(mut body: Map<String, Value>) -> Result<DocFields, AppError> {
    if body.get("title").and_then(Value::as_str).is_some_and(|t| t.trim().is_empty()) {
        body.remove("title");
    }
    let mut v = Validator::new(body);
    document_rules(&mut v, true);
    Ok(doc_fields(&v.finish()?))
}

/// PATCH of a document's label.
pub fn document_patch(body: Map<String, Value>) -> Result<DocFields, AppError> {
    let mut v = Validator::new(body);
    document_rules(&mut v, true);
    Ok(doc_fields(&v.finish()?))
}

// ─── Bank details ───────────────────────────────────────────────────────────

pub(crate) fn bank_rules(v: &mut Validator) {
    trim_or_null(v, &["bankName", "bankBranch", "bankAccountNo"]);
    to_number(v, &["staffLoanBalance"]);
    v.field("bankName", true, &[Rule::IsString, Rule::MaxLength(100)])
        .field("bankBranch", true, &[Rule::IsString, Rule::MaxLength(100)])
        .field("bankAccountNo", true, &[Rule::IsString, Rule::MaxLength(50)])
        .field("staffLoanBalance", true, AMOUNT);
}

pub fn bank(body: Map<String, Value>) -> Result<BankFields, AppError> {
    let mut v = Validator::new(body);
    bank_rules(&mut v);
    let m = v.finish()?;
    Ok(BankFields {
        bank_name: nullable(&m, "bankName"),
        bank_branch: nullable(&m, "bankBranch"),
        bank_account_no: nullable(&m, "bankAccountNo"),
        // null loan balance: "not changing it" (the reference wrote NULL into a NOT NULL column).
        staff_loan_balance: money(&m, "staffLoanBalance")?,
    })
}

// ─── Salaries ───────────────────────────────────────────────────────────────

const EFFECTIVE_REQUIRED: Rule = Rule::Msg(&Rule::IsNotEmpty, "Effective date is required");
const SALARY_AMOUNTS: &[&str] =
    &["basic", "bra2005", "bra2016", "vehicleAllowance", "otherAllowance", "welfareDeduction", "loanInstallment"];

/// `CreateSalaryDto`; with `optional` it is `UpdateSalaryDto`.
pub(crate) fn salary_rules(v: &mut Validator, optional: bool) {
    to_number(v, SALARY_AMOUNTS);
    trim_or_null(v, &["reason", "notes"]);
    v.field("effectiveFrom", optional, &[Rule::IsDateFormat, EFFECTIVE_REQUIRED])
        .field("basic", optional, AMOUNT)
        .field("bra2005", true, AMOUNT)
        .field("bra2016", true, AMOUNT)
        .field("vehicleAllowance", true, AMOUNT)
        .field("otherAllowance", true, AMOUNT)
        .field("epfApplicable", true, &[Rule::IsBoolean])
        .field("welfareDeduction", true, AMOUNT)
        .field("loanInstallment", true, AMOUNT)
        .field("reason", true, &[Rule::IsString, Rule::MaxLength(200)])
        .field("notes", true, &[Rule::IsString]);
}

pub fn salary(body: Map<String, Value>, optional: bool) -> Result<SalaryFields, AppError> {
    let mut v = Validator::new(body);
    salary_rules(&mut v, optional);
    let m = v.finish()?;
    Ok(SalaryFields {
        effective_from: required_text(&m, "effectiveFrom"),
        basic: money(&m, "basic")?,
        bra2005: money(&m, "bra2005")?,
        bra2016: money(&m, "bra2016")?,
        vehicle_allowance: money(&m, "vehicleAllowance")?,
        other_allowance: money(&m, "otherAllowance")?,
        epf_applicable: m.get("epfApplicable").and_then(Value::as_bool),
        welfare_deduction: money(&m, "welfareDeduction")?,
        loan_installment: money(&m, "loanInstallment")?,
        reason: nullable(&m, "reason"),
        notes: nullable(&m, "notes"),
    })
}
