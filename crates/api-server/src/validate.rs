//! Request validation with class-validator semantics, as configured in the reference
//! (`ValidationPipe({ whitelist: true, transform: true })`, class-validator 0.15.1,
//! validator 13.15).
//!
//! Behaviour reproduced (verified against `tests/fixtures/reference_validation.json`, which
//! was produced by running the reference DTOs through Nest's real `ValidationPipe`):
//! * properties are checked in declaration order; within one property the failing rules'
//!   messages appear in **reverse** declaration order (decorators register bottom-up);
//! * `@IsOptional()` skips every rule when the value is `null` or absent, and a present `null`
//!   is passed through;
//! * properties without decorators are stripped (whitelist);
//! * query values are converted with JavaScript `Number()` (`@Type(() => Number)`).

pub mod email;
pub mod hr;
pub mod ops;
pub mod payroll;
pub mod us_payroll;

use app_core::ids::is_uuid;
use app_core::permissions;
use app_core::AppError;
use serde_json::{Map, Value};

pub enum Rule {
    IsString,
    IsNotEmpty,
    MinLength(usize),
    MaxLength(usize),
    /// Custom-message pattern check (`@Matches(regex, { message })`).
    Matches(fn(&str) -> bool, &'static str),
    IsUuid,
    IsBoolean,
    IsArray,
    ArrayUnique,
    /// `@IsIn(list, { each: true })`.
    IsInEach(fn() -> Vec<&'static str>),
    /// `@IsEnum(E)`: one of the enum's string values.
    IsEnum(&'static [&'static str]),
    /// The reference's custom `@IsDateFormat()`: a real calendar date as `YYYY-MM-DD`.
    IsDateFormat,
    /// `@IsNumber({ maxDecimalPlaces: 2 })`: a finite JSON number with at most two decimals —
    /// and, for a currency without decimals, a whole number (D29).
    IsMoneyNumber,
    /// A quantity: like `IsMoneyNumber` but always allowed two decimals (2.5 hours of yen work).
    IsQuantity,
    /// `@Min(0)` on a number.
    MinZero,
    /// `@Min(x)` on a number; the text is how JavaScript prints x (e.g. "0.001").
    MinNum(f64, &'static str),
    /// `@IsEmail()` (validator.js defaults).
    IsEmail,
    /// `@IsString({ each: true })`.
    IsStringEach,
    /// `@MaxLength(n, { each: true })`.
    MaxLengthEach(usize),
    /// `@IsNumber()` without options: any finite JSON number.
    IsFiniteNumber,
    /// `@Max(x)` on a number; the text is how JavaScript prints x.
    MaxNum(f64, &'static str),
    /// `@IsInt()`.
    IsInt,
    /// Any rule with a custom `{ message }`.
    Msg(&'static Rule, &'static str),
}

/// validator.js `isLength`: surrogate pairs count once, variation selectors not at all.
fn js_length(s: &str) -> usize {
    s.chars().filter(|c| *c != '\u{FE0E}' && *c != '\u{FE0F}').count()
}

impl Rule {
    fn passes(&self, v: &Value) -> bool {
        match self {
            Rule::Msg(inner, _) => inner.passes(v),
            Rule::IsFiniteNumber => v.as_f64().is_some_and(f64::is_finite),
            Rule::MaxNum(max, _) => v.as_f64().is_some_and(|x| x <= *max),
            Rule::IsInt => v.as_f64().is_some_and(|x| x.is_finite() && x.fract() == 0.0),
            Rule::IsString => v.is_string(),
            Rule::IsNotEmpty => !matches!(v, Value::Null) && v.as_str() != Some(""),
            Rule::MinLength(n) => v.as_str().is_some_and(|s| js_length(s) >= *n),
            Rule::MaxLength(n) => v.as_str().is_some_and(|s| js_length(s) <= *n),
            Rule::Matches(f, _) => v.as_str().is_some_and(f),
            Rule::IsUuid => v.as_str().is_some_and(is_uuid),
            Rule::IsBoolean => v.is_boolean(),
            Rule::IsArray => v.is_array(),
            Rule::ArrayUnique => v.as_array().is_some_and(|a| a.iter().enumerate().all(|(i, x)| !a[..i].contains(x))),
            Rule::IsEnum(values) => v.as_str().is_some_and(|s| values.contains(&s)),
            Rule::IsDateFormat => v.as_str().is_some_and(app_core::time::is_date_format),
            Rule::IsMoneyNumber => app_core::money::Money::from_json(v).is_some_and(|m| m.fits_currency()),
            Rule::IsQuantity => app_core::money::Money::from_json(v).is_some(),
            Rule::MinZero => v.as_number().is_some_and(|n| n.as_f64().is_some_and(|x| x >= 0.0)),
            Rule::MinNum(min, _) => v.as_number().is_some_and(|n| n.as_f64().is_some_and(|x| x >= *min)),
            Rule::IsEmail => v.as_str().is_some_and(email::is_email),
            Rule::IsStringEach => match v {
                Value::Array(items) => items.iter().all(Value::is_string),
                other => other.is_string(),
            },
            Rule::MaxLengthEach(n) => {
                let ok = |x: &Value| x.as_str().is_some_and(|s| js_length(s) <= *n);
                match v {
                    Value::Array(items) => items.iter().all(ok),
                    other => ok(other),
                }
            }
            Rule::IsInEach(list) => {
                let allowed = list();
                let ok = |x: &Value| x.as_str().is_some_and(|s| allowed.contains(&s));
                match v {
                    Value::Array(items) => items.iter().all(ok),
                    other => ok(other),
                }
            }
        }
    }

    pub(crate) fn message(&self, prop: &str) -> String {
        match self {
            Rule::Msg(_, msg) => (*msg).to_string(),
            Rule::IsFiniteNumber => format!("{prop} must be a number conforming to the specified constraints"),
            Rule::MaxNum(_, text) => format!("{prop} must not be greater than {text}"),
            Rule::IsInt => format!("{prop} must be an integer number"),
            Rule::IsString => format!("{prop} must be a string"),
            Rule::IsNotEmpty => format!("{prop} should not be empty"),
            Rule::MinLength(n) => format!("{prop} must be longer than or equal to {n} characters"),
            Rule::MaxLength(n) => format!("{prop} must be shorter than or equal to {n} characters"),
            Rule::Matches(_, msg) => (*msg).to_string(),
            Rule::IsUuid => format!("{prop} must be a UUID"),
            Rule::IsBoolean => format!("{prop} must be a boolean value"),
            Rule::IsArray => format!("{prop} must be an array"),
            Rule::ArrayUnique => format!("All {prop}'s elements must be unique"),
            Rule::IsEnum(values) => format!("{prop} must be one of the following values: {}", values.join(", ")),
            Rule::IsDateFormat => format!("{prop} must be a valid date in YYYY-MM-DD format"),
            Rule::IsMoneyNumber if app_core::money::whole_units() => {
                format!("{prop} must be a whole amount — the company's currency has no decimal places")
            }
            Rule::IsMoneyNumber | Rule::IsQuantity => {
                format!("{prop} must be a number conforming to the specified constraints")
            }
            Rule::MinZero => format!("{prop} must not be less than 0"),
            Rule::MinNum(_, text) => format!("{prop} must not be less than {text}"),
            Rule::IsEmail => format!("{prop} must be an email"),
            Rule::IsStringEach => format!("each value in {prop} must be a string"),
            Rule::MaxLengthEach(n) => format!("each value in {prop} must be shorter than or equal to {n} characters"),
            Rule::IsInEach(list) => {
                format!("each value in {prop} must be one of the following values: {}", list().join(", "))
            }
        }
    }
}

pub struct Validator {
    pub(crate) input: Map<String, Value>,
    output: Map<String, Value>,
    errors: Vec<String>,
    /// Nest prefixes nested errors with their path, e.g. `lines.0.`.
    prefix: String,
}

impl Validator {
    pub fn new(input: Map<String, Value>) -> Self {
        Validator { input, output: Map::new(), errors: Vec::new(), prefix: String::new() }
    }

    pub fn nested(input: Map<String, Value>, prefix: String) -> Self {
        Validator { input, output: Map::new(), errors: Vec::new(), prefix }
    }

    /// Validates one property. Rules are given in declaration order (top to bottom).
    pub fn field(&mut self, prop: &str, optional: bool, rules: &[Rule]) -> &mut Self {
        let mut present = self.input.get(prop).cloned();
        // D11/D12: the frontend sends "" for an optional email or date the user left empty.
        if optional
            && present.as_ref().and_then(Value::as_str) == Some("")
            && rules.iter().any(|r| matches!(r, Rule::IsEmail | Rule::IsDateFormat))
        {
            present = Some(Value::Null);
        }
        let value = present.clone().unwrap_or(Value::Null);
        if optional && value.is_null() {
            if let Some(v) = present {
                self.output.insert(prop.to_string(), v);
            }
            return self;
        }
        let mut failed: Vec<String> =
            rules.iter().filter(|r| !r.passes(&value)).map(|r| format!("{}{}", self.prefix, r.message(prop))).collect();
        failed.reverse();
        self.errors.extend(failed);
        if let Some(v) = present {
            self.output.insert(prop.to_string(), v);
        }
        self
    }

    /// Errors collected so far plus the whitelisted object (for nested validation).
    pub fn into_parts(self) -> (Vec<String>, Map<String, Value>) {
        (self.errors, self.output)
    }

    pub fn push_errors(&mut self, errors: Vec<String>) {
        self.errors.extend(errors);
    }

    pub fn put(&mut self, prop: &str, value: Value) {
        self.output.insert(prop.to_string(), value);
    }

    /// The whitelisted object, or a 400 with every message.
    pub fn finish(self) -> Result<Map<String, Value>, AppError> {
        if self.errors.is_empty() {
            Ok(self.output)
        } else {
            Err(AppError::validation(self.errors))
        }
    }
}

// ─── JS Number() and query parameters ───────────────────────────────────────

pub use app_core::jsnum::js_number;

fn is_integer(n: Option<f64>) -> bool {
    n.is_some_and(|x| x.is_finite() && x.fract() == 0.0)
}

/// Query parameters as Express' `qs` delivers them: a repeated key becomes an array.
pub fn query_map(pairs: Vec<(String, String)>) -> Map<String, Value> {
    let mut map = Map::new();
    for (k, v) in pairs {
        match map.get_mut(&k) {
            Some(Value::Array(items)) => items.push(Value::String(v)),
            Some(existing) => {
                let first = existing.take();
                *existing = Value::Array(vec![first, Value::String(v)]);
            }
            None => {
                map.insert(k, Value::String(v));
            }
        }
    }
    map
}

/// The value `@Type(() => Number)` produces: strings through `Number()`, arrays become NaN.
pub(crate) fn as_js_number(v: &Value) -> Option<f64> {
    match v {
        Value::String(s) => js_number(s),
        Value::Number(n) => n.as_f64(),
        _ => None,
    }
}

/// Reference `PaginationDto`: page ≥ 1 (default 1), 1 ≤ limit ≤ 100 (default 20).
pub fn pagination(query: &Map<String, Value>) -> Result<(i64, i64), AppError> {
    let mut errors = Vec::new();
    let mut read = |prop: &str, default: i64, max: Option<f64>| -> i64 {
        let Some(raw) = query.get(prop) else { return default };
        let n = as_js_number(raw);
        // Declaration order: IsInt, Min(1), Max → messages in reverse.
        let mut failed = Vec::new();
        if !is_integer(n) {
            failed.push(format!("{prop} must be an integer number"));
        }
        if !n.is_some_and(|x| x >= 1.0) {
            failed.push(format!("{prop} must not be less than 1"));
        }
        if let Some(m) = max {
            if !n.is_some_and(|x| x <= m) {
                failed.push(format!("{prop} must not be greater than {m}"));
            }
        }
        failed.reverse();
        errors.extend(failed);
        n.map(|x| x as i64).unwrap_or(default)
    };
    let page = read("page", 1, None);
    let limit = read("limit", 20, Some(100.0));
    if errors.is_empty() {
        Ok((page, limit))
    } else {
        Err(AppError::validation(errors))
    }
}

// ─── DTOs (W0) ──────────────────────────────────────────────────────────────

fn username_pattern(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

const USERNAME_MESSAGE: &str = "username may only contain letters, numbers, dots, underscores and hyphens";

fn permission_keys() -> Vec<&'static str> {
    permissions::keys().collect()
}

pub(crate) fn opt_string(map: &Map<String, Value>, key: &str) -> Option<String> {
    map.get(key).and_then(Value::as_str).map(str::to_string)
}

fn string_list(map: &Map<String, Value>, key: &str) -> Option<Vec<String>> {
    map.get(key).and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect())
}

pub struct CreateUserDto {
    pub username: String,
    pub password: String,
    pub role_id: Option<String>,
}

pub fn create_user(body: Map<String, Value>) -> Result<CreateUserDto, AppError> {
    let mut v = Validator::new(body);
    v.field(
        "username",
        false,
        &[Rule::IsString, Rule::MinLength(3), Rule::Matches(username_pattern, USERNAME_MESSAGE)],
    )
    .field("password", false, &[Rule::IsString, Rule::MinLength(8)])
    .field("roleId", true, &[Rule::IsUuid]);
    let m = v.finish()?;
    Ok(CreateUserDto {
        username: opt_string(&m, "username").unwrap_or_default(),
        password: opt_string(&m, "password").unwrap_or_default(),
        role_id: opt_string(&m, "roleId"),
    })
}

/// `null` values pass validation (IsOptional) and are treated as "not sent".
pub struct UpdateUserDto {
    pub username: Option<String>,
    pub role_id: Option<String>,
    pub is_active: Option<bool>,
}

pub fn update_user(body: Map<String, Value>) -> Result<UpdateUserDto, AppError> {
    let mut v = Validator::new(body);
    v.field("username", true, &[Rule::IsString, Rule::MinLength(3), Rule::Matches(username_pattern, USERNAME_MESSAGE)])
        .field("roleId", true, &[Rule::IsUuid])
        .field("isActive", true, &[Rule::IsBoolean]);
    let m = v.finish()?;
    Ok(UpdateUserDto {
        username: opt_string(&m, "username"),
        role_id: opt_string(&m, "roleId"),
        is_active: m.get("isActive").and_then(Value::as_bool),
    })
}

pub fn reset_password(body: Map<String, Value>) -> Result<String, AppError> {
    let mut v = Validator::new(body);
    v.field("password", false, &[Rule::IsString, Rule::MinLength(8)]);
    let m = v.finish()?;
    Ok(opt_string(&m, "password").unwrap_or_default())
}

pub struct RoleDto {
    pub name: Option<String>,
    pub permissions: Option<Vec<String>>,
}

fn role_rules(v: &mut Validator, optional: bool) {
    v.field("name", optional, &[Rule::IsString, Rule::IsNotEmpty]).field(
        "permissions",
        optional,
        &[Rule::IsArray, Rule::ArrayUnique, Rule::IsInEach(permission_keys)],
    );
}

pub fn create_role(body: Map<String, Value>) -> Result<(String, Vec<String>), AppError> {
    let mut v = Validator::new(body);
    role_rules(&mut v, false);
    let m = v.finish()?;
    Ok((opt_string(&m, "name").unwrap_or_default(), string_list(&m, "permissions").unwrap_or_default()))
}

pub fn update_role(body: Map<String, Value>) -> Result<RoleDto, AppError> {
    let mut v = Validator::new(body);
    role_rules(&mut v, true);
    let m = v.finish()?;
    Ok(RoleDto { name: opt_string(&m, "name"), permissions: string_list(&m, "permissions") })
}

/// D25: company details and region, kept as settings. Each key with its maximum length.
const COMPANY_SETTINGS: &[(&str, usize)] = &[
    ("company.name", 120),
    ("company.address", 500),
    ("company.phone", 40),
    ("company.fax", 40),
    ("company.email", 120),
    ("company.website", 120),
    ("company.taxNumber", 60),
    ("company.logo", 400_000),
    ("company.paymentInstructions", 2000),
    ("company.footerNote", 300),
    ("region.country", 2),
    ("region.currency", 3),
    ("region.currencySymbol", 8),
    ("region.currencyWord", 30),
    ("region.currencyMinorWord", 30),
    ("region.locale", 20),
    ("region.fiscalYearStart", 2),
    // D26: the tax codes new sales / purchase lines start with ("" for none).
    ("tax.defaultSales", 36),
    ("tax.defaultPurchases", 36),
];

/// Returns the provided (non-null) settings in declaration order.
pub fn update_settings(body: Map<String, Value>) -> Result<Vec<(String, String)>, AppError> {
    let mut v = Validator::new(body);
    v.field("signatoryName", true, &[Rule::IsString, Rule::MaxLength(120)]).field(
        "signatoryTitle",
        true,
        &[Rule::IsString, Rule::MaxLength(120)],
    );
    for (key, max) in COMPANY_SETTINGS {
        v.field(key, true, &[Rule::IsString, Rule::MaxLength(*max)]);
    }
    let m = v.finish()?;
    let out: Vec<(String, String)> = ["signatoryName", "signatoryTitle"]
        .iter()
        .copied()
        .chain(COMPANY_SETTINGS.iter().map(|(k, _)| *k))
        .filter_map(|k| opt_string(&m, k).map(|s| (k.to_string(), s)))
        .collect();

    let mut errors = Vec::new();
    for (key, value) in &out {
        let value = value.trim();
        let ok = match key.as_str() {
            "region.country" => value.len() == 2 && value.bytes().all(|b| b.is_ascii_uppercase()),
            "region.currency" => {
                value.len() == 3
                    && value.bytes().all(|b| b.is_ascii_uppercase())
                    && !app_core::money::THREE_DECIMAL.contains(&value)
            }
            "region.locale" => {
                let mut parts = value.split('-');
                let lang = parts.next().unwrap_or("");
                (2..=3).contains(&lang.len())
                    && lang.bytes().all(|b| b.is_ascii_lowercase())
                    && parts.all(|p| (2..=4).contains(&p.len()) && p.bytes().all(|b| b.is_ascii_alphanumeric()))
            }
            "region.fiscalYearStart" => value.parse::<u8>().is_ok_and(|m| (1..=12).contains(&m)),
            "company.name" => !value.is_empty(),
            // A picture uploaded on the Settings page, or none.
            "company.logo" => {
                value.is_empty()
                    || ["data:image/png;base64,", "data:image/jpeg;base64,", "data:image/webp;base64,"]
                        .iter()
                        .any(|p| value.starts_with(p))
            }
            _ => true,
        };
        if !ok {
            errors.push(match key.as_str() {
                "region.country" => "region.country must be a two-letter country code (e.g. LK, GB, US)".to_string(),
                "region.currency" if app_core::money::THREE_DECIMAL.contains(&value.trim()) => format!(
                    "{} has three decimal places, which the app cannot keep yet — choose another currency",
                    value.trim()
                ),
                "region.currency" => {
                    "region.currency must be a three-letter currency code (e.g. LKR, USD, EUR)".to_string()
                }
                "region.locale" => "region.locale must be a language tag such as en-GB".to_string(),
                "region.fiscalYearStart" => "region.fiscalYearStart must be a month number from 1 to 12".to_string(),
                "company.name" => "company.name should not be empty".to_string(),
                _ => "company.logo must be a PNG, JPEG or WebP picture".to_string(),
            });
        }
    }
    if !errors.is_empty() {
        return Err(AppError::validation(errors));
    }
    Ok(out)
}

// ─── DTOs (W1: accounts and ledger) ─────────────────────────────────────────

pub const ACCOUNT_TYPES: &[&str] = &["ASSET", "LIABILITY", "EQUITY", "REVENUE", "EXPENSE"];

fn account_rules(v: &mut Validator, optional: bool) {
    v.field("code", optional, &[Rule::IsString, Rule::IsNotEmpty])
        .field("name", optional, &[Rule::IsString, Rule::IsNotEmpty])
        .field("type", optional, &[Rule::IsEnum(ACCOUNT_TYPES)])
        .field("subtype", true, &[Rule::IsString])
        .field("description", true, &[Rule::IsString])
        .field("parentId", true, &[Rule::IsUuid])
        .field("currency", true, &[Rule::IsString, Rule::MaxLength(3)]);
}

/// Validated account fields. For the nullable ones, `Some(None)` means "sent as null".
pub struct AccountDto {
    pub code: Option<String>,
    pub name: Option<String>,
    pub account_type: Option<String>,
    pub subtype: Option<Option<String>>,
    pub description: Option<Option<String>>,
    pub parent_id: Option<Option<String>>,
    /// D34: `Some(None)` / "" = home currency.
    pub currency: Option<Option<String>>,
}

pub(crate) fn nullable(m: &Map<String, Value>, key: &str) -> Option<Option<String>> {
    m.get(key).map(|v| v.as_str().map(str::to_string))
}

fn account_dto(m: &Map<String, Value>) -> AccountDto {
    AccountDto {
        code: opt_string(m, "code"),
        name: opt_string(m, "name"),
        account_type: opt_string(m, "type"),
        subtype: nullable(m, "subtype"),
        description: nullable(m, "description"),
        parent_id: nullable(m, "parentId"),
        currency: nullable(m, "currency"),
    }
}

pub fn create_account(body: Map<String, Value>) -> Result<AccountDto, AppError> {
    let mut v = Validator::new(body);
    account_rules(&mut v, false);
    Ok(account_dto(&v.finish()?))
}

pub fn update_account(body: Map<String, Value>) -> Result<AccountDto, AppError> {
    let mut v = Validator::new(body);
    account_rules(&mut v, true);
    Ok(account_dto(&v.finish()?))
}

pub struct JournalLineDto {
    pub account_id: String,
    pub debit: app_core::money::Money,
    pub credit: app_core::money::Money,
    pub description: Option<String>,
    /// D34: on an account kept in a foreign currency, the amount in that currency (positive).
    pub foreign_amount: Option<app_core::money::Money>,
}

pub struct JournalEntryDto {
    pub date: String,
    pub description: String,
    pub reference: Option<String>,
    pub source_type: Option<String>,
    pub source_id: Option<String>,
    pub lines: Vec<JournalLineDto>,
}

/// Validates one element of a nested array; returns its errors and whitelisted fields.
pub(crate) type ElementRules = fn(&Value, String) -> (Vec<String>, Map<String, Value>);

/// `@ValidateNested({ each: true }) @ArrayMinSize(min) @Type(...)` on `prop`, with Nest's
/// flattening: when any element has errors only the element errors are reported, prefixed
/// with their path (`lines.0.`); otherwise the property's own errors are. The whitelisted
/// elements are stored on the validator's output.
pub(crate) fn nested_array(v: &mut Validator, prop: &str, optional: bool, min: usize, element: ElementRules) {
    let value = v.input.get(prop).cloned();
    if optional && value.as_ref().is_none_or(Value::is_null) {
        if let Some(Value::Null) = value {
            v.put(prop, Value::Null);
        }
        return;
    }
    let nested_message = format!("each value in nested property {prop} must be either object or array");
    let mut own = Vec::new();
    if !value.as_ref().and_then(Value::as_array).is_some_and(|a| a.len() >= min) {
        own.push(format!("{prop} must contain at least {min} elements"));
    }
    let mut children = Vec::new();
    let mut clean = Vec::new();
    match &value {
        None => {}
        Some(Value::Array(items)) => {
            for (i, item) in items.iter().enumerate() {
                if item.is_object() || item.is_array() {
                    let (errs, c) = element(item, format!("{prop}.{i}."));
                    children.extend(errs);
                    clean.push(Value::Object(c));
                } else {
                    children.push(format!("{prop}.{nested_message}"));
                }
            }
        }
        Some(obj @ Value::Object(_)) => {
            let (errs, _) = element(obj, format!("{prop}."));
            children.extend(errs);
        }
        Some(_) => own.push(nested_message),
    }
    if children.is_empty() {
        v.push_errors(own);
    } else {
        v.push_errors(children);
    }
    if value.is_some() {
        v.put(prop, Value::Array(clean));
    }
}

fn line_rules(item: &Value, prefix: String) -> (Vec<String>, Map<String, Value>) {
    let mut v = Validator::nested(item.as_object().cloned().unwrap_or_default(), prefix);
    v.field("accountId", false, &[Rule::IsUuid])
        .field("debit", false, &[Rule::IsMoneyNumber, Rule::MinZero])
        .field("credit", false, &[Rule::IsMoneyNumber, Rule::MinZero])
        .field("description", true, &[Rule::IsString])
        .field("foreignAmount", true, &[Rule::IsMoneyNumber, Rule::MinZero]);
    v.into_parts()
}

/// `CreateJournalEntryDto` with `@ValidateNested({ each: true }) @ArrayMinSize(2)` lines, and
/// Nest's flattening: when any nested line has errors only the nested errors are reported.
pub fn create_journal_entry(body: Map<String, Value>) -> Result<JournalEntryDto, AppError> {
    let mut v = Validator::new(body);
    v.field("date", false, &[Rule::IsDateFormat])
        .field("description", false, &[Rule::IsString, Rule::IsNotEmpty])
        .field("reference", true, &[Rule::IsString])
        .field("sourceType", true, &[Rule::IsString])
        .field("sourceId", true, &[Rule::IsString]);

    nested_array(&mut v, "lines", false, 2, line_rules);
    let m = v.finish()?;

    let lines = m
        .get("lines")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .map(|l| JournalLineDto {
                    account_id: l.get("accountId").and_then(Value::as_str).unwrap_or_default().to_string(),
                    debit: l.get("debit").and_then(app_core::money::Money::from_json).unwrap_or_default(),
                    credit: l.get("credit").and_then(app_core::money::Money::from_json).unwrap_or_default(),
                    description: l.get("description").and_then(Value::as_str).map(str::to_string),
                    foreign_amount: l.get("foreignAmount").and_then(app_core::money::Money::from_json),
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(JournalEntryDto {
        date: opt_string(&m, "date").unwrap_or_default(),
        description: opt_string(&m, "description").unwrap_or_default(),
        reference: opt_string(&m, "reference"),
        source_type: opt_string(&m, "sourceType"),
        source_id: opt_string(&m, "sourceId"),
        lines,
    })
}

/// `LedgerFilterDto` (query string).
pub struct LedgerFilterDto {
    pub date_from: Option<String>,
    pub date_to: Option<String>,
    pub source_type: Option<String>,
}

pub fn ledger_filter(query: Map<String, Value>) -> Result<LedgerFilterDto, AppError> {
    let mut v = Validator::new(query);
    v.field("dateFrom", true, &[Rule::IsDateFormat]).field("dateTo", true, &[Rule::IsDateFormat]).field(
        "sourceType",
        true,
        &[Rule::IsString],
    );
    let m = v.finish()?;
    Ok(LedgerFilterDto {
        date_from: opt_string(&m, "dateFrom"),
        date_to: opt_string(&m, "dateTo"),
        source_type: opt_string(&m, "sourceType"),
    })
}

/// Raw `@Query('x')` dates the reference passed straight to SQL (a bad value was a 500 there).
/// An empty value means "not given", as the reference's truthiness checks treated it.
pub fn optional_query_date(query: &Map<String, Value>, prop: &str) -> Result<Option<String>, AppError> {
    let raw = match query.get(prop) {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(items)) => items.first().and_then(Value::as_str).unwrap_or_default().to_string(),
        _ => return Ok(None),
    };
    if raw.is_empty() {
        return Ok(None);
    }
    if app_core::time::is_date_format(&raw) {
        Ok(Some(raw))
    } else {
        Err(AppError::validation(vec![Rule::IsDateFormat.message(prop)]))
    }
}

#[cfg(test)]
mod tests;
