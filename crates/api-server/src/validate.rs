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
}

/// validator.js `isLength`: surrogate pairs count once, variation selectors not at all.
fn js_length(s: &str) -> usize {
    s.chars().filter(|c| *c != '\u{FE0E}' && *c != '\u{FE0F}').count()
}

impl Rule {
    fn passes(&self, v: &Value) -> bool {
        match self {
            Rule::IsString => v.is_string(),
            Rule::IsNotEmpty => !matches!(v, Value::Null) && v.as_str() != Some(""),
            Rule::MinLength(n) => v.as_str().is_some_and(|s| js_length(s) >= *n),
            Rule::MaxLength(n) => v.as_str().is_some_and(|s| js_length(s) <= *n),
            Rule::Matches(f, _) => v.as_str().is_some_and(f),
            Rule::IsUuid => v.as_str().is_some_and(is_uuid),
            Rule::IsBoolean => v.is_boolean(),
            Rule::IsArray => v.is_array(),
            Rule::ArrayUnique => v.as_array().is_some_and(|a| a.iter().enumerate().all(|(i, x)| !a[..i].contains(x))),
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

    fn message(&self, prop: &str) -> String {
        match self {
            Rule::IsString => format!("{prop} must be a string"),
            Rule::IsNotEmpty => format!("{prop} should not be empty"),
            Rule::MinLength(n) => format!("{prop} must be longer than or equal to {n} characters"),
            Rule::MaxLength(n) => format!("{prop} must be shorter than or equal to {n} characters"),
            Rule::Matches(_, msg) => (*msg).to_string(),
            Rule::IsUuid => format!("{prop} must be a UUID"),
            Rule::IsBoolean => format!("{prop} must be a boolean value"),
            Rule::IsArray => format!("{prop} must be an array"),
            Rule::ArrayUnique => format!("All {prop}'s elements must be unique"),
            Rule::IsInEach(list) => {
                format!("each value in {prop} must be one of the following values: {}", list().join(", "))
            }
        }
    }
}

pub struct Validator {
    input: Map<String, Value>,
    output: Map<String, Value>,
    errors: Vec<String>,
}

impl Validator {
    pub fn new(input: Map<String, Value>) -> Self {
        Validator { input, output: Map::new(), errors: Vec::new() }
    }

    /// Validates one property. Rules are given in declaration order (top to bottom).
    pub fn field(&mut self, prop: &str, optional: bool, rules: &[Rule]) -> &mut Self {
        let present = self.input.get(prop).cloned();
        let value = present.clone().unwrap_or(Value::Null);
        if optional && value.is_null() {
            if let Some(v) = present {
                self.output.insert(prop.to_string(), v);
            }
            return self;
        }
        let mut failed: Vec<String> = rules.iter().filter(|r| !r.passes(&value)).map(|r| r.message(prop)).collect();
        failed.reverse();
        self.errors.extend(failed);
        if let Some(v) = present {
            self.output.insert(prop.to_string(), v);
        }
        self
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

/// JavaScript `Number(string)`. `None` stands for NaN.
pub fn js_number(raw: &str) -> Option<f64> {
    let s = raw.trim_matches(|c: char| c.is_whitespace() || c == '\u{FEFF}');
    if s.is_empty() {
        return Some(0.0);
    }
    match s {
        "Infinity" | "+Infinity" => return Some(f64::INFINITY),
        "-Infinity" => return Some(f64::NEG_INFINITY),
        _ => {}
    }
    for (prefix, radix) in [("0x", 16), ("0X", 16), ("0o", 8), ("0O", 8), ("0b", 2), ("0B", 2)] {
        if let Some(digits) = s.strip_prefix(prefix) {
            return u64::from_str_radix(digits, radix).ok().map(|n| n as f64);
        }
    }
    let unsigned = s.strip_prefix(['+', '-']).unwrap_or(s);
    let (mantissa, exponent) = match unsigned.find(['e', 'E']) {
        Some(i) => (&unsigned[..i], Some(&unsigned[i + 1..])),
        None => (unsigned, None),
    };
    let mantissa_ok = {
        let (int, frac) = mantissa.split_once('.').unwrap_or((mantissa, ""));
        let digits = |x: &str| x.bytes().all(|b| b.is_ascii_digit());
        digits(int) && digits(frac) && !(int.is_empty() && frac.is_empty())
    };
    let exponent_ok = exponent.is_none_or(|e| {
        let e = e.strip_prefix(['+', '-']).unwrap_or(e);
        !e.is_empty() && e.bytes().all(|b| b.is_ascii_digit())
    });
    if !(mantissa_ok && exponent_ok) {
        return None;
    }
    s.parse::<f64>().ok()
}

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
fn as_js_number(v: &Value) -> Option<f64> {
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

fn opt_string(map: &Map<String, Value>, key: &str) -> Option<String> {
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

/// Returns the provided (non-null) settings in declaration order.
pub fn update_settings(body: Map<String, Value>) -> Result<Vec<(String, String)>, AppError> {
    let mut v = Validator::new(body);
    v.field("signatoryName", true, &[Rule::IsString, Rule::MaxLength(120)]).field(
        "signatoryTitle",
        true,
        &[Rule::IsString, Rule::MaxLength(120)],
    );
    let m = v.finish()?;
    Ok(["signatoryName", "signatoryTitle"]
        .iter()
        .filter_map(|k| opt_string(&m, k).map(|s| (k.to_string(), s)))
        .collect())
}

#[cfg(test)]
mod tests;
