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
        if let Some(intended) = approved_deviation(dto, &case["input"]) {
            assert_eq!(got.ok(), Some(intended), "{dto} {}: approved deviation", case["input"]);
            continue;
        }
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

/// Fixture cases where the reference result is deliberately not reproduced, with the intended
/// whitelisted output. D11: an empty optional email means "no email" (the reference rejected it).
fn approved_deviation(dto: &str, input: &Value) -> Option<Value> {
    match (dto, input) {
        ("CreateCustomerDto", i) if i == &serde_json::json!({"name": "Acme", "email": ""}) => {
            Some(serde_json::json!({"name": "Acme", "email": null}))
        }
        _ => None,
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

#[test]
fn accounts_match_reference() {
    check("CreateAccountDto", |b| {
        let mut v = Validator::new(b);
        account_rules(&mut v, false);
        whitelisted(v)
    });
    check("UpdateAccountDto", |b| {
        let mut v = Validator::new(b);
        account_rules(&mut v, true);
        whitelisted(v)
    });
}

#[test]
fn journal_entries_match_reference() {
    check("CreateJournalEntryDto", |b| {
        create_journal_entry(b).map(|d| {
            let mut out = serde_json::json!({ "date": d.date, "description": d.description });
            for (k, val) in [("reference", d.reference), ("sourceType", d.source_type), ("sourceId", d.source_id)] {
                if let Some(x) = val {
                    out[k] = Value::String(x);
                }
            }
            out["lines"] = Value::Array(
                d.lines
                    .iter()
                    .map(|l| {
                        let mut o = serde_json::json!({
                            "accountId": l.account_id,
                            "debit": l.debit.to_json_number(),
                            "credit": l.credit.to_json_number(),
                        });
                        if let Some(desc) = &l.description {
                            o["description"] = Value::String(desc.clone());
                        }
                        o
                    })
                    .collect(),
            );
            out
        })
    });
}

#[test]
fn ledger_filter_matches_reference() {
    check("LedgerFilterDto", |q| {
        ledger_filter(q).map(|f| {
            let mut out = serde_json::json!({});
            for (k, val) in [("dateFrom", f.date_from), ("dateTo", f.date_to), ("sourceType", f.source_type)] {
                if let Some(x) = val {
                    out[k] = Value::String(x);
                }
            }
            out
        })
    });
}

// ── W2 ───────────────────────────────────────────────────────────────────────

fn rules_check(dto: &str, f: fn(&mut Validator)) {
    check(dto, |b| {
        let mut v = Validator::new(b);
        f(&mut v);
        whitelisted(v)
    });
}

#[test]
fn parties_and_banking_match_reference() {
    rules_check("CreateCustomerDto", |v| ops::party_rules(v, false));
    rules_check("UpdateCustomerDto", |v| ops::party_rules(v, true));
    rules_check("CreateVendorDto", |v| ops::party_rules(v, false));
    rules_check("CreateBankAccountDto", |v| ops::bank_rules(v, false));
    rules_check("UpdateBankAccountDto", |v| ops::bank_rules(v, true));
}

#[test]
fn invoices_match_reference() {
    rules_check("CreateInvoiceDto", |v| ops::invoice_rules(v, false));
    rules_check("UpdateInvoiceDto", |v| ops::invoice_rules(v, true));
    rules_check("ChangeInvoiceStatusDto", ops::status_rules);
    check("QueryInvoicesDto", |q| {
        ops::invoice_query(q).map(|iq| {
            let mut out = serde_json::json!({ "page": iq.page, "limit": iq.limit });
            for (k, val) in [
                ("q", iq.q),
                ("status", iq.status),
                ("customerId", iq.customer_id),
                ("dateFrom", iq.date_from),
                ("dateTo", iq.date_to),
            ] {
                if let Some(x) = val {
                    out[k] = Value::String(x);
                }
            }
            out
        })
    });
}

#[test]
fn bills_expenses_payments_match_reference() {
    rules_check("CreateBillDto", ops::bill_rules);
    rules_check("CreateExpenseDto", ops::expense_rules);
    rules_check("CreatePaymentDto", ops::payment_rules);
}

// ── W4 ───────────────────────────────────────────────────────────────────────

#[test]
fn employees_match_reference() {
    rules_check("CreateEmployeeDto", |v| hr::employee_rules(v, false));
    rules_check("UpdateEmployeeDto", |v| hr::employee_rules(v, true));
    rules_check("QueryEmployeesDto", hr::employee_query_rules);
    rules_check("UploadDocumentDto", |v| hr::document_rules(v, false));
}

#[test]
fn bank_and_salaries_match_reference() {
    rules_check("UpdateBankDto", hr::bank_rules);
    rules_check("CreateSalaryDto", |v| hr::salary_rules(v, false));
    rules_check("UpdateSalaryDto", |v| hr::salary_rules(v, true));
}
