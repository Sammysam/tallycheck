use crate::runs::{self, LinePatch, Remit};
use app_core::money::Money;
use app_core::{AppError, AppResult};
use database::Db;
use employees::records::{self, EmployeeFields};
use employees::salaries::{self, BankFields, SalaryFields};
use employees::Actor;
use serde_json::Value;

async fn db() -> AppResult<(tempfile::TempDir, Db)> {
    let dir = tempfile::tempdir().map_err(AppError::internal)?;
    let db = Db::open(&dir.path().join("t.db")).await?;
    ledger::accounts::seed(&db).await?;
    Ok((dir, db))
}

fn rs(rupees: i64) -> Option<Money> {
    Some(Money::from_cents(rupees * 100))
}

fn admin() -> Actor {
    Actor { id: None, username: Some("admin".into()) }
}

async fn employee(db: &Db, code: &str, join: Option<&str>, basic: i64) -> AppResult<String> {
    let e = records::create(
        db,
        EmployeeFields {
            employee_id: Some(code.into()),
            name: Some(format!("Employee {code}")),
            join_date: Some(join.map(str::to_string)),
            ..Default::default()
        },
    )
    .await?;
    salaries::add(
        db,
        &e.id,
        SalaryFields {
            effective_from: Some("2026-01-01".into()),
            basic: rs(basic),
            bra2005: rs(1000),
            bra2016: rs(2500),
            vehicle_allowance: rs(15_000),
            other_allowance: rs(24_850),
            ..Default::default()
        },
        &admin(),
    )
    .await?;
    Ok(e.id)
}

fn s(v: &Value, k: &str) -> String {
    v[k].as_str().unwrap_or_default().to_string()
}

async fn loan(db: &Db, id: &str) -> AppResult<Value> {
    Ok(salaries::employee_payroll(db, id, None).await?["staffLoanBalance"].clone())
}

/// Sum of debits minus credits per account code over every journal line.
async fn net_by_code(db: &Db, code: &str) -> AppResult<i64> {
    let v: i64 = sqlx::query_scalar(
        r#"SELECT COALESCE(SUM(l."debit" - l."credit"), 0) FROM "journal_lines" l JOIN "accounts" a ON a."id" = l."accountId" WHERE a."code" = ?1"#,
    )
    .bind(code)
    .fetch_one(db.read())
    .await?;
    Ok(v)
}

#[tokio::test]
async fn a_month_from_draft_to_void() -> AppResult<()> {
    let (_d, db) = db().await?;
    let asha = employee(&db, "CEN-001", Some("2024-01-15"), 40_500).await?;
    let joiner = employee(&db, "CEN-002", Some("2026-04-16"), 30_000).await?;
    let future = employee(&db, "CEN-003", Some("2026-05-10"), 30_000).await?;
    salaries::update_bank(&db, &asha, BankFields { staff_loan_balance: rs(10_000), ..Default::default() }).await?;

    let run = runs::create(&db, "2026-04", Some("April".into()), &admin()).await?;
    let id = s(&run, "id");
    assert_eq!(s(&run, "runNumber"), "PRUN-0001");
    assert_eq!(s(&run, "periodEnd"), "2026-04-30");
    assert_eq!(run["employeeCount"], 2);
    assert_eq!(run["skipped"][0]["employeeId"].as_str(), Some(future.as_str()));
    assert_eq!(run["skipped"][0]["reason"], "Joins on 2026-05-10");
    let lines = run["lines"].as_array().cloned().unwrap_or_default();
    assert_eq!(lines[0]["grossSalary"], "83850.00", "April 2026 regression row");
    assert_eq!(lines[0]["netSalary"], "80005.00");
    assert_eq!(lines[1]["daysWorked"], "15.00", "joined on the 16th");
    assert_eq!(lines[1]["basicForMonth"], "15000.00");
    let again = runs::create(&db, "2026-04", None, &admin()).await;
    assert!(matches!(again, Err(AppError::BadRequest(_))), "one live run per month");

    let asha_line = s(&lines[0], "id");
    let edited = runs::update_line(
        &db,
        &id,
        &asha_line,
        LinePatch { loan_recovery: rs(2_500), salary_advance: rs(500), ..Default::default() },
    )
    .await?;
    assert_eq!(edited["loanRecovery"], 2500, "sent fields come back as numbers");
    assert_eq!(edited["netSalary"], 77005);
    assert_eq!(edited["basic"], "40500.00", "untouched fields stay strings");
    let too_much =
        runs::update_line(&db, &id, &asha_line, LinePatch { loan_recovery: rs(20_000), ..Default::default() }).await;
    assert!(too_much.is_err());

    let posted = runs::post(&db, &id).await?;
    assert_eq!(posted["status"], "POSTED");
    assert_eq!(loan(&db, &asha).await?, 7000, "recovery and advance came off the loan");
    let entry = ledger::entries::get(&mut *db.read().acquire().await?, &s(&posted, "journalEntryId")).await?;
    let debits: i64 = entry.lines.iter().map(|l| l.debit.cents()).sum();
    let credits: i64 = entry.lines.iter().map(|l| l.credit.cents()).sum();
    assert_eq!(debits, credits);
    assert_eq!(entry.date, "2026-04-30");
    assert!(runs::post(&db, &id).await.is_err(), "no second posting");

    let cash = ledger::accounts::find_by_code(&mut *db.read().acquire().await?, "1000").await?.id;
    let remit = |kind: &str, amount: Option<Money>| Remit {
        kind: kind.into(),
        payment_account_id: cash.clone(),
        date: "2026-05-15".into(),
        amount,
        reference: None,
    };
    let after = runs::remit(&db, &id, remit("EPF", None), &admin()).await?;
    assert_eq!(after["owed"]["EPF"]["outstanding"], 0);
    assert!(runs::remit(&db, &id, remit("EPF", None), &admin()).await.is_err(), "nothing left to remit");
    assert!(runs::remit(&db, &id, remit("ETF", rs(1_000_000)), &admin()).await.is_err(), "more than owed");
    let paid = runs::pay(&db, &id, &cash, "2026-04-30").await?;
    assert_eq!(paid["status"], "PAID");
    assert_eq!(net_by_code(&db, "2200").await?, 0, "salaries payable cleared");

    let void = runs::void(&db, &id).await?;
    assert_eq!(void["status"], "VOID");
    assert_eq!(loan(&db, &asha).await?, 10_000, "loan restored");
    for code in ["1000", "1300", "2200", "2210", "2220", "2230", "2240", "2250", "2260", "6100", "6110", "6120"] {
        assert_eq!(net_by_code(&db, code).await?, 0, "account {code} nets to zero after the void");
    }
    assert!(runs::void(&db, &id).await.is_err());
    // The month is free again; a draft can be deleted outright.
    let redo = runs::create(&db, "2026-04", None, &admin()).await?;
    assert_eq!(s(&redo, "runNumber"), "PRUN-0002");
    runs::remove(&db, &s(&redo, "id")).await?;
    let _ = joiner;
    Ok(())
}

#[tokio::test]
async fn a_failed_posting_changes_nothing() -> AppResult<()> {
    let (_d, db) = db().await?;
    let asha = employee(&db, "CEN-001", None, 40_500).await?;
    salaries::update_bank(&db, &asha, BankFields { staff_loan_balance: rs(5_000), ..Default::default() }).await?;
    let run = runs::create(&db, "2026-04", None, &admin()).await?;
    let id = s(&run, "id");
    let line = s(&run["lines"][0], "id");
    // Deductions above gross pay make net negative: the reference's posting rules then give an
    // unbalanced entry, which the ledger refuses.
    runs::update_line(
        &db,
        &id,
        &line,
        LinePatch { loan_recovery: rs(5_000), other_deductions: rs(100_000), ..Default::default() },
    )
    .await?;
    let failed = runs::post(&db, &id).await;
    assert!(matches!(failed, Err(AppError::BadRequest(_))), "{failed:?}");
    let after = runs::find_one(&db, &id).await?;
    assert_eq!(after["status"], "DRAFT");
    assert_eq!(loan(&db, &asha).await?, 5000, "loan untouched");
    let entries: i64 = sqlx::query_scalar(r#"SELECT COUNT(*) FROM "journal_entries""#).fetch_one(db.read()).await?;
    assert_eq!(entries, 0, "no journal entry left behind");
    Ok(())
}

#[tokio::test]
async fn rates_and_company_settings_round_trip() -> AppResult<()> {
    let (_d, db) = db().await?;
    let rates =
        runs::update_rates(&db, vec![("payroll.stampDuty", "25.5".into()), ("payroll.etfRate", "null".into())]).await?;
    assert_eq!(rates["stampDuty"], serde_json::json!(25.5));
    assert_eq!(rates["etfRate"], serde_json::json!(0.03), "an unreadable value falls back to the default");
    let company =
        runs::update_company(&db, vec![("careOf".into(), None), ("telephone".into(), Some("011".into()))]).await?;
    assert_eq!(company["careOf"], Value::Null);
    assert_eq!(company["telephone"], "011");
    assert_eq!(company["companyName"], "CENANGO (PVT) LTD");
    Ok(())
}
