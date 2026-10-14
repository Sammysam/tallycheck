//! Differential tests: every case in the fixture was run through the reference DTO classes and
//! Nest's real ValidationPipe (tools/gen_validation_fixture.js).

use super::*;

fn fixture() -> Value {
    serde_json::from_str(include_str!("../../tests/fixtures/reference_validation.json")).unwrap_or(Value::Null)
}

/// Runs `f` on each case of `dto` and compares the error list or the whitelisted output.
fn check(dto: &str, f: impl Fn(Map<String, Value>) -> Result<Value, AppError>) {
    let fx = fixture();
    let cases = fx[dto].as_array().cloned().unwrap_or_default();
    assert!(!cases.is_empty(), "no fixture cases for {dto}");
    for case in cases {
        let input = case["input"].as_object().cloned().unwrap_or_default();
        let got = f(input);
        match (&case["message"], got) {
            (Value::Array(expected), Err(AppError::BadRequest(app_core::ErrorMessage::List(actual)))) => {
                let expected: Vec<String> = expected.iter().filter_map(Value::as_str).map(str::to_string).collect();
                assert_eq!(actual, expected, "{dto} {}", case["input"]);
            }
            (Value::Null, Ok(out)) => assert_eq!(out, case["ok"], "{dto} {}", case["input"]),
            (expected, got) => panic!("{dto} {}: expected {expected}, got {got:?}", case["input"]),
        }
    }
}

/// The raw whitelisted object, for comparing with the pipe's transformed output.
fn whitelisted(v: Validator) -> Result<Value, AppError> {
    v.finish().map(Value::Object)
}

#[test]
fn create_user_matches_reference() {
    check("CreateUserDto", |b| {
        let mut v = Validator::new(b);
        v.field(
            "username",
            false,
            &[Rule::IsString, Rule::MinLength(3), Rule::Matches(username_pattern, USERNAME_MESSAGE)],
        )
        .field("password", false, &[Rule::IsString, Rule::MinLength(8)])
        .field("roleId", true, &[Rule::IsUuid]);
        whitelisted(v)
    });
}

#[test]
fn update_user_matches_reference() {
    check("UpdateUserDto", |b| {
        let mut v = Validator::new(b);
        v.field(
            "username",
            true,
            &[Rule::IsString, Rule::MinLength(3), Rule::Matches(username_pattern, USERNAME_MESSAGE)],
        )
        .field("roleId", true, &[Rule::IsUuid])
        .field("isActive", true, &[Rule::IsBoolean]);
        whitelisted(v)
    });
}

#[test]
fn reset_password_matches_reference() {
    check("ResetPasswordDto", |b| {
        let mut v = Validator::new(b);
        v.field("password", false, &[Rule::IsString, Rule::MinLength(8)]);
        whitelisted(v)
    });
}

#[test]
fn roles_match_reference() {
    check("CreateRoleDto", |b| {
        let mut v = Validator::new(b);
        role_rules(&mut v, false);
        whitelisted(v)
    });
    check("UpdateRoleDto", |b| {
        let mut v = Validator::new(b);
        role_rules(&mut v, true);
        whitelisted(v)
    });
}

#[test]
fn settings_match_reference() {
    check("UpdateSettingsDto", |b| {
        let mut v = Validator::new(b);
        v.field("signatoryName", true, &[Rule::IsString, Rule::MaxLength(120)]).field(
            "signatoryTitle",
            true,
            &[Rule::IsString, Rule::MaxLength(120)],
        );
        whitelisted(v)
    });
}

#[test]
fn pagination_matches_reference() {
    check("PaginationDto", |q| pagination(&q).map(|(page, limit)| serde_json::json!({ "page": page, "limit": limit })));
}

#[test]
fn typed_dtos_use_the_same_rules() -> Result<(), AppError> {
    let body = |v: Value| v.as_object().cloned().unwrap_or_default();
    let u = create_user(body(serde_json::json!({"username": "alice", "password": "longenough", "roleId": null})))?;
    assert_eq!((u.username.as_str(), u.role_id), ("alice", None));
    let u = update_user(body(serde_json::json!({"isActive": null, "username": "bob"})))?;
    assert_eq!((u.username.as_deref(), u.is_active), (Some("bob"), None));
    let s = update_settings(body(serde_json::json!({"signatoryTitle": "CEO", "signatoryName": null})))?;
    assert_eq!(s, vec![("signatoryTitle".to_string(), "CEO".to_string())]);
    assert!(create_role(body(serde_json::json!({"name": "x", "permissions": ["users"]}))).is_ok());
    Ok(())
}

#[test]
fn js_number_semantics() {
    assert_eq!(js_number(""), Some(0.0));
    assert_eq!(js_number(" 3 "), Some(3.0));
    assert_eq!(js_number("1e1"), Some(10.0));
    assert_eq!(js_number("0x10"), Some(16.0));
    assert_eq!(js_number("-1"), Some(-1.0));
    assert_eq!(js_number(".5"), Some(0.5));
    assert_eq!(js_number("1."), Some(1.0));
    assert_eq!(js_number("abc"), None);
    assert_eq!(js_number("inf"), None);
    assert_eq!(js_number("NaN"), None);
    assert_eq!(js_number("1e"), None);
    assert_eq!(js_number("-0x10"), None);
}

#[test]
fn repeated_query_keys_become_arrays() {
    let m = query_map(vec![("page".into(), "1".into()), ("page".into(), "2".into()), ("limit".into(), "5".into())]);
    assert_eq!(m["page"], serde_json::json!(["1", "2"]));
    assert_eq!(m["limit"], serde_json::json!("5"));
}
