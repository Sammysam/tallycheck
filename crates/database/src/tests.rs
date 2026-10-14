//! Schema and integrity tests against a real on-disk SQLite database.

use super::*;
use std::collections::BTreeSet;

async fn fresh() -> AppResult<(tempfile::TempDir, Db)> {
    let dir = tempfile::tempdir().map_err(AppError::internal)?;
    let db = Db::open(&dir.path().join("t.db")).await?;
    Ok((dir, db))
}

/// Final column set of every reference table after migrations 1717400000000–1717400021000.
const REFERENCE_SCHEMA: &[(&str, &[&str])] = &[
    (
        "accounts",
        &["id", "code", "name", "type", "subtype", "description", "isActive", "parentId", "createdAt", "updatedAt"],
    ),
    (
        "journal_entries",
        &[
            "id",
            "entryNumber",
            "date",
            "description",
            "reference",
            "sourceType",
            "sourceId",
            "status",
            "reversalOfId",
            "createdAt",
            "updatedAt",
        ],
    ),
    ("journal_lines", &["id", "journalEntryId", "accountId", "description", "debit", "credit"]),
    ("customers", &["id", "name", "email", "phone", "address", "taxId", "isActive", "createdAt", "updatedAt"]),
    ("vendors", &["id", "name", "email", "phone", "address", "taxId", "isActive", "createdAt", "updatedAt"]),
    (
        "invoices",
        &[
            "id",
            "invoiceNumber",
            "customerId",
            "date",
            "dueDate",
            "status",
            "notes",
            "totalAmount",
            "journalEntryId",
            "createdAt",
            "updatedAt",
            "poNumber",
            "billToName",
            "billToAddress",
            "paymentOptions",
            "signatoryName",
            "signatoryTitle",
            "paymentTerms",
            "amountPaid",
            "createdById",
        ],
    ),
    ("invoice_lines", &["id", "invoiceId", "description", "quantity", "unitPrice", "amount", "accountId"]),
    (
        "bills",
        &[
            "id",
            "billNumber",
            "vendorId",
            "date",
            "dueDate",
            "status",
            "notes",
            "totalAmount",
            "journalEntryId",
            "createdAt",
            "updatedAt",
        ],
    ),
    ("bill_lines", &["id", "billId", "description", "quantity", "unitPrice", "amount", "accountId"]),
    (
        "bank_accounts",
        &[
            "id",
            "name",
            "bankName",
            "accountNumber",
            "accountId",
            "currentBalance",
            "isActive",
            "createdAt",
            "updatedAt",
        ],
    ),
    (
        "expenses",
        &[
            "id",
            "expenseNumber",
            "date",
            "description",
            "amount",
            "expenseAccountId",
            "paymentAccountId",
            "receiptUrl",
            "status",
            "journalEntryId",
            "createdAt",
            "updatedAt",
        ],
    ),
    (
        "payments",
        &[
            "id",
            "paymentNumber",
            "type",
            "invoiceId",
            "billId",
            "customerId",
            "vendorId",
            "date",
            "amount",
            "paymentMethod",
            "paymentAccountId",
            "notes",
            "journalEntryId",
            "createdAt",
            "updatedAt",
        ],
    ),
    ("counters", &["key", "value"]),
    (
        "users",
        &[
            "id",
            "username",
            "passwordHash",
            "twoFactorSecret",
            "isTwoFactorEnabled",
            "createdAt",
            "updatedAt",
            "roleId",
            "isActive",
        ],
    ),
    ("roles", &["id", "name", "permissions", "isSystem", "createdAt", "updatedAt"]),
    ("audit_logs", &["id", "userId", "username", "action", "method", "path", "statusCode", "detail", "createdAt"]),
    ("settings", &["key", "value", "updatedAt"]),
    (
        "employees",
        &[
            "id",
            "employeeId",
            "name",
            "jobTitle",
            "department",
            "email",
            "phone",
            "joinDate",
            "status",
            "notes",
            "photoKey",
            "photoMime",
            "userId",
            "createdAt",
            "updatedAt",
            "employmentType",
            "epfNumber",
            "nationalId",
            "resignedDate",
            "bankName",
            "bankBranch",
            "bankAccountNo",
            "staffLoanBalance",
        ],
    ),
    (
        "employee_documents",
        &[
            "id",
            "employeeId",
            "category",
            "title",
            "fileKey",
            "originalName",
            "mimeType",
            "size",
            "expiryDate",
            "notes",
            "isCurrent",
            "replacesId",
            "uploadedById",
            "uploadedByName",
            "createdAt",
        ],
    ),
    (
        "employee_salaries",
        &[
            "id",
            "employeeId",
            "effectiveFrom",
            "basic",
            "bra2005",
            "bra2016",
            "vehicleAllowance",
            "otherAllowance",
            "epfApplicable",
            "welfareDeduction",
            "loanInstallment",
            "reason",
            "notes",
            "createdById",
            "createdByName",
            "createdAt",
            "updatedAt",
        ],
    ),
    (
        "payroll_runs",
        &[
            "id",
            "runNumber",
            "period",
            "periodStart",
            "periodEnd",
            "status",
            "notes",
            "ratesSnapshot",
            "employeeCount",
            "earningsTotal",
            "noPayTotal",
            "grossTotal",
            "epfEmployeeTotal",
            "payeTotal",
            "stampDutyTotal",
            "welfareTotal",
            "loanRecoveryTotal",
            "salaryAdvanceTotal",
            "otherDeductionsTotal",
            "deductionsTotal",
            "netTotal",
            "epfEmployerTotal",
            "etfTotal",
            "employerWelfareTotal",
            "costToCompanyTotal",
            "journalEntryId",
            "paymentJournalEntryId",
            "paymentAccountId",
            "payDate",
            "postedAt",
            "paidAt",
            "createdById",
            "createdByName",
            "createdAt",
            "updatedAt",
            "slipsFiledAt",
        ],
    ),
    (
        "payroll_lines",
        &[
            "id",
            "runId",
            "employeeId",
            "salaryId",
            "slipNumber",
            "employeeCode",
            "employeeName",
            "jobTitle",
            "department",
            "employmentType",
            "epfNumber",
            "nationalId",
            "bankName",
            "bankBranch",
            "bankAccountNo",
            "epfApplicable",
            "daysInMonth",
            "daysWorked",
            "basic",
            "basicForMonth",
            "bra2005",
            "bra2016",
            "vehicleAllowance",
            "otherAllowance",
            "projectBonus",
            "arrears",
            "medical",
            "otherEarnings",
            "earningsTotal",
            "noPayAmount",
            "grossSalary",
            "epfBase",
            "epfEmployee",
            "payeOverride",
            "paye",
            "stampDuty",
            "welfare",
            "loanRecovery",
            "salaryAdvance",
            "phoneBill",
            "otherDeductions",
            "deductionsTotal",
            "netSalary",
            "epfEmployer",
            "etf",
            "employerWelfare",
            "costToCompany",
            "notes",
            "createdAt",
            "updatedAt",
        ],
    ),
    (
        "payroll_remittances",
        &[
            "id",
            "runId",
            "type",
            "amount",
            "date",
            "paymentAccountId",
            "reference",
            "journalEntryId",
            "createdByName",
            "createdAt",
        ],
    ),
];

#[tokio::test]
async fn schema_matches_reference_inventory() -> AppResult<()> {
    let (_dir, db) = fresh().await?;
    let tables: BTreeSet<String> = sqlx::query_scalar(
        "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' AND name <> '_sqlx_migrations'",
    )
    .fetch_all(db.read())
    .await?
    .into_iter()
    .collect();
    let expected: BTreeSet<String> = REFERENCE_SCHEMA.iter().map(|(t, _)| t.to_string()).collect();
    assert_eq!(tables, expected);

    for (table, cols) in REFERENCE_SCHEMA {
        let actual: Vec<String> = sqlx::query_scalar(&format!(r#"SELECT name FROM pragma_table_info('{table}')"#))
            .fetch_all(db.read())
            .await?;
        let expected: Vec<String> = cols.iter().map(|c| c.to_string()).collect();
        assert_eq!(actual, expected, "columns of {table}");
    }
    Ok(())
}

#[tokio::test]
async fn every_connection_enforces_foreign_keys_and_wal() -> AppResult<()> {
    let (_dir, db) = fresh().await?;
    // Check several reader connections, not just the first one handed out.
    let mut conns = Vec::new();
    for _ in 0..READ_POOL_SIZE {
        conns.push(db.read().acquire().await?);
    }
    for c in conns.iter_mut() {
        let fk: i64 = sqlx::query_scalar("PRAGMA foreign_keys").fetch_one(&mut **c).await?;
        assert_eq!(fk, 1);
    }
    drop(conns);
    db.verify_pragmas().await?;
    let sync: i64 = sqlx::query_scalar("PRAGMA synchronous").fetch_one(db.write()).await?;
    assert_eq!(sync, 2, "synchronous=FULL");
    Ok(())
}

#[tokio::test]
async fn reader_pool_cannot_write() -> AppResult<()> {
    let (_dir, db) = fresh().await?;
    let r = sqlx::query(r#"INSERT INTO "counters" ("key","value") VALUES ('X', 1)"#).execute(db.read()).await;
    assert!(r.is_err());
    Ok(())
}

#[tokio::test]
async fn reopening_is_idempotent_and_keeps_seeds_once() -> AppResult<()> {
    let dir = tempfile::tempdir().map_err(AppError::internal)?;
    let path = dir.path().join("t.db");
    Db::open(&path).await?.close().await;
    let db = Db::open(&path).await?;
    let roles: i64 = sqlx::query_scalar(r#"SELECT COUNT(*) FROM "roles""#).fetch_one(db.read()).await?;
    assert_eq!(roles, 3);
    let applied: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM _sqlx_migrations").fetch_one(db.read()).await?;
    assert_eq!(applied, i64::try_from(MIGRATOR.iter().count()).map_err(AppError::internal)?);
    Ok(())
}

#[tokio::test]
async fn seeds_match_reference_migrations() -> AppResult<()> {
    let (_dir, db) = fresh().await?;
    let roles: Vec<(String, String, i64)> =
        sqlx::query_as(r#"SELECT "name", "permissions", "isSystem" FROM "roles" ORDER BY rowid"#)
            .fetch_all(db.read())
            .await?;
    let all: Vec<&str> = app_core::permissions::keys().collect();
    let admin: Vec<String> = serde_json_array(&roles[0].1);
    assert_eq!(roles[0].0, "Admin");
    assert_eq!(admin, all);
    assert_eq!(roles[0].2, 1);
    let accountant: Vec<String> = serde_json_array(&roles[1].1);
    assert_eq!(accountant, all.iter().filter(|k| **k != "users").map(|s| s.to_string()).collect::<Vec<_>>());
    assert_eq!(serde_json_array(&roles[2].1), vec!["dashboard", "ledger", "reports"]);

    let inv: i64 =
        sqlx::query_scalar(r#"SELECT "value" FROM "counters" WHERE "key" = 'INV'"#).fetch_one(db.read()).await?;
    assert_eq!(inv, 8999);

    let settings = settings::find_all(&db).await?;
    assert_eq!(settings.get("signatoryName").map(String::as_str), Some("Samir Husain"));
    assert_eq!(
        settings.get("payroll.payeSlabs").map(String::as_str),
        Some("[[250000,0],[500000,0.06],[750000,0.12],[null,0.18]]")
    );
    assert_eq!(settings.len(), 17);

    let codes: Vec<String> =
        sqlx::query_scalar(r#"SELECT "code" FROM "accounts" ORDER BY "code""#).fetch_all(db.read()).await?;
    assert_eq!(codes, ["1300", "2200", "2210", "2220", "2230", "2240", "2250", "2260", "6100", "6110", "6120"]);
    let ids: BTreeSet<String> =
        sqlx::query_scalar(r#"SELECT "id" FROM "accounts""#).fetch_all(db.read()).await?.into_iter().collect();
    assert_eq!(ids.len(), 11);
    assert!(ids.iter().all(|id| app_core::ids::is_uuid(id)));
    Ok(())
}

fn serde_json_array(text: &str) -> Vec<String> {
    // Minimal parser for the seeded JSON arrays of strings, avoiding a serde_json dependency.
    text.trim_matches(['[', ']'])
        .split(',')
        .filter(|s| !s.is_empty())
        .map(|s| s.trim().trim_matches('"').to_string())
        .collect()
}

async fn insert_entry(tx: &mut Tx, id: &str, number: &str) -> AppResult<()> {
    sqlx::query(r#"INSERT INTO "journal_entries" ("id","entryNumber","date","description") VALUES (?1, ?2, '2026-04-30', 'test')"#)
        .bind(id)
        .bind(number)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

#[tokio::test]
async fn journal_entries_are_immutable() -> AppResult<()> {
    let (_dir, db) = fresh().await?;
    let account: String = sqlx::query_scalar(r#"SELECT "id" FROM "accounts" LIMIT 1"#).fetch_one(db.read()).await?;
    let mut tx = db.begin().await?;
    insert_entry(&mut tx, "e1", "JE-000001").await?;
    sqlx::query(r#"INSERT INTO "journal_lines" ("id","journalEntryId","accountId","debit") VALUES ('l1','e1',?1,100)"#)
        .bind(&account)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;

    let w = db.write();
    assert!(sqlx::query(r#"DELETE FROM "journal_entries" WHERE "id"='e1'"#).execute(w).await.is_err());
    assert!(sqlx::query(r#"DELETE FROM "journal_lines" WHERE "id"='l1'"#).execute(w).await.is_err());
    assert!(sqlx::query(r#"UPDATE "journal_lines" SET "debit"=200 WHERE "id"='l1'"#).execute(w).await.is_err());
    assert!(sqlx::query(r#"UPDATE "journal_entries" SET "description"='x' WHERE "id"='e1'"#).execute(w).await.is_err());
    assert!(sqlx::query(r#"UPDATE "journal_entries" SET "date"='2026-05-01' WHERE "id"='e1'"#)
        .execute(w)
        .await
        .is_err());
    // The one permitted change, and it cannot be undone.
    sqlx::query(
        r#"UPDATE "journal_entries" SET "status"='REVERSED', "updatedAt"='2026-10-14T00:00:00.000Z' WHERE "id"='e1'"#,
    )
    .execute(w)
    .await?;
    assert!(sqlx::query(r#"UPDATE "journal_entries" SET "status"='POSTED' WHERE "id"='e1'"#).execute(w).await.is_err());
    // A journal line cannot carry a negative amount.
    assert!(sqlx::query(
        r#"INSERT INTO "journal_lines" ("id","journalEntryId","accountId","credit") VALUES ('l2','e1',?1,-1)"#
    )
    .bind(&account)
    .execute(w)
    .await
    .is_err());
    Ok(())
}

#[tokio::test]
async fn an_entry_can_be_reversed_only_once() -> AppResult<()> {
    let (_dir, db) = fresh().await?;
    let mut tx = db.begin().await?;
    insert_entry(&mut tx, "e1", "JE-1").await?;
    tx.commit().await?;
    let rev = r#"INSERT INTO "journal_entries" ("id","entryNumber","date","description","reversalOfId") VALUES (?1, ?2, '2026-05-01', 'rev', 'e1')"#;
    sqlx::query(rev).bind("r1").bind("JE-2").execute(db.write()).await?;
    assert!(sqlx::query(rev).bind("r2").bind("JE-3").execute(db.write()).await.is_err());
    Ok(())
}

#[tokio::test]
async fn foreign_keys_are_enforced() -> AppResult<()> {
    let (_dir, db) = fresh().await?;
    let r = sqlx::query(
        r#"INSERT INTO "journal_lines" ("id","journalEntryId","accountId") VALUES ('l','missing','missing')"#,
    )
    .execute(db.write())
    .await;
    assert!(r.is_err());
    let r = sqlx::query(
        r#"INSERT INTO "users" ("id","username","passwordHash","roleId") VALUES ('u','a','h','no-such-role')"#,
    )
    .execute(db.write())
    .await;
    assert!(r.is_err());
    Ok(())
}

#[tokio::test]
async fn check_constraints_reject_bad_enums_dates_and_types() -> AppResult<()> {
    let (_dir, db) = fresh().await?;
    let w = db.write();
    assert!(sqlx::query(r#"INSERT INTO "accounts" ("id","code","name","type") VALUES ('a','9','x','INCOME')"#)
        .execute(w)
        .await
        .is_err());
    assert!(sqlx::query(
        r#"INSERT INTO "journal_entries" ("id","entryNumber","date","description") VALUES ('e','n','2026-02-30','x')"#
    )
    .execute(w)
    .await
    .is_err());
    assert!(
        sqlx::query(r#"INSERT INTO "counters" ("key","value") VALUES ('k','12.5')"#).execute(w).await.is_err(),
        "STRICT tables reject non-integers in INTEGER columns"
    );
    assert!(sqlx::query(r#"INSERT INTO "roles" ("id","name","permissions") VALUES ('r','R','{"a":1}')"#)
        .execute(w)
        .await
        .is_err());
    Ok(())
}

#[tokio::test]
async fn employee_ids_are_unique_ignoring_case() -> AppResult<()> {
    let (_dir, db) = fresh().await?;
    let q = r#"INSERT INTO "employees" ("id","employeeId","name") VALUES (?1, ?2, 'x')"#;
    sqlx::query(q).bind("e1").bind("CEN-001").execute(db.write()).await?;
    assert!(sqlx::query(q).bind("e2").bind("cen-001").execute(db.write()).await.is_err());
    Ok(())
}

#[tokio::test]
async fn one_live_payroll_run_per_period() -> AppResult<()> {
    let (_dir, db) = fresh().await?;
    let q = r#"INSERT INTO "payroll_runs" ("id","runNumber","period","periodStart","periodEnd","status") VALUES (?1, ?2, '2026-04', '2026-04-01', '2026-04-30', ?3)"#;
    sqlx::query(q).bind("r1").bind("PRUN-0001").bind("VOID").execute(db.write()).await?;
    sqlx::query(q).bind("r2").bind("PRUN-0002").bind("DRAFT").execute(db.write()).await?;
    assert!(sqlx::query(q).bind("r3").bind("PRUN-0003").bind("POSTED").execute(db.write()).await.is_err());
    Ok(())
}

#[tokio::test]
async fn audit_log_is_append_only() -> AppResult<()> {
    let (_dir, db) = fresh().await?;
    sqlx::query(r#"INSERT INTO "audit_logs" ("id","action") VALUES ('a','Logged in')"#).execute(db.write()).await?;
    assert!(sqlx::query(r#"UPDATE "audit_logs" SET "action"='x'"#).execute(db.write()).await.is_err());
    assert!(sqlx::query(r#"DELETE FROM "audit_logs""#).execute(db.write()).await.is_err());
    Ok(())
}

#[tokio::test]
async fn settings_upsert_overrides_defaults() -> AppResult<()> {
    let (_dir, db) = fresh().await?;
    let mut tx = db.begin().await?;
    settings::upsert(&mut tx, "signatoryName", "Someone Else").await?;
    settings::upsert(&mut tx, "newKey", "v").await?;
    tx.commit().await?;
    let s = settings::find_all(&db).await?;
    assert_eq!(s.get("signatoryName").map(String::as_str), Some("Someone Else"));
    assert_eq!(s.get("newKey").map(String::as_str), Some("v"));
    Ok(())
}
