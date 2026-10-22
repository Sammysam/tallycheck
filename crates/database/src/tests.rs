//! Schema and integrity tests against a real on-disk SQLite database.

use super::*;
use std::collections::BTreeSet;

async fn fresh() -> AppResult<(tempfile::TempDir, Db)> {
    let dir = tempfile::tempdir().map_err(AppError::internal)?;
    let db = Db::open(&dir.path().join("t.db")).await?;
    Ok((dir, db))
}

// The fixtures were exported from a PostgreSQL database built by running all 22 reference
// migrations (tests/contract/run-all.sh starts that database):
//   reference_pg_columns.txt      table|column|type|nullable|default  (ordinal order)
//   reference_pg_constraints.txt  table|name|definition               (FK, UNIQUE, CHECK)
//   reference_pg_indexes.txt      CREATE INDEX statements
const PG_COLUMNS: &str = include_str!("../tests/fixtures/reference_pg_columns.txt");
const PG_CONSTRAINTS: &str = include_str!("../tests/fixtures/reference_pg_constraints.txt");
const PG_INDEXES: &str = include_str!("../tests/fixtures/reference_pg_indexes.txt");

/// Columns this project adds on purpose (approved deviations), appended after the reference's.
const APPROVED_EXTRA_COLUMNS: &[(&str, &str, bool)] = &[
    ("bills", "amountPaid", true),         // D13
    ("payments", "status", true),          // D18
    ("invoices", "amountCredited", true),  // D21
    ("bills", "amountCredited", true),     // D21
    ("invoice_lines", "taxCodeId", false), // D26
    ("invoice_lines", "taxAmount", true),
    ("invoices", "taxAmount", true),
    ("invoices", "pricesIncludeTax", true),
    ("bill_lines", "taxCodeId", false),
    ("bill_lines", "taxAmount", true),
    ("bills", "taxAmount", true),
    ("bills", "pricesIncludeTax", true),
    ("expenses", "taxCodeId", false),
    ("expenses", "taxAmount", true),
    ("payments", "withholdingTaxCodeId", false), // D27
    ("payments", "withholdingAmount", true),
    ("invoices", "currency", false), // D30
    ("invoices", "exchangeRate", true),
    ("invoices", "baseTotal", true),
    ("invoices", "baseAmountPaid", true),
    ("invoices", "baseAmountCredited", true),
    ("bills", "currency", false),
    ("bills", "exchangeRate", true),
    ("bills", "baseTotal", true),
    ("bills", "baseAmountPaid", true),
    ("bills", "baseAmountCredited", true),
    ("payments", "exchangeRate", true),
    ("payments", "baseAmount", true),
    ("payments", "baseSettled", true),
    ("payments", "exchangeDifference", true),
    ("bills", "payableAccountId", false), // D33
    ("payments", "payableAccountId", false),
    ("accounts", "currency", false), // D34
    ("journal_lines", "foreignAmount", false),
];

/// Tables this project adds on purpose (approved deviations); the reference has no such table.
const APPROVED_EXTRA_TABLES: &[&str] = &[
    "credit_notes",
    "bank_reconciliations",
    "reconciled_lines",
    "tax_codes",
    "tax_code_components",
    "document_taxes",
    "imports",
    "import_items",
    "bank_lines",
    "import_layouts",
]; // D21, D22, D26, D27, D37

async fn sqlite_columns(db: &Db, table: &str) -> AppResult<Vec<(String, bool)>> {
    let rows: Vec<(String, i64, i64)> =
        sqlx::query_as(&format!("SELECT name, \"notnull\", pk FROM pragma_table_info('{table}')"))
            .fetch_all(db.read())
            .await?;
    Ok(rows.into_iter().map(|(n, notnull, pk)| (n, notnull == 1 || pk > 0)).collect())
}

#[tokio::test]
async fn tables_columns_and_nullability_match_reference_postgres() -> AppResult<()> {
    let (_dir, db) = fresh().await?;
    let mut expected: std::collections::BTreeMap<String, Vec<(String, bool)>> = Default::default();
    for line in PG_COLUMNS.lines().filter(|l| !l.trim().is_empty()) {
        let f: Vec<&str> = line.split('|').collect();
        expected.entry(f[0].to_string()).or_default().push((f[1].to_string(), f[3] == "NO"));
    }
    let tables: BTreeSet<String> = sqlx::query_scalar(
        "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' AND name <> '_sqlx_migrations'",
    )
    .fetch_all(db.read())
    .await?
    .into_iter()
    .collect();
    let mut reference_tables: BTreeSet<String> = expected.keys().cloned().collect();
    reference_tables.extend(APPROVED_EXTRA_TABLES.iter().map(|t| t.to_string()));
    assert_eq!(tables, reference_tables);
    for (table, cols) in &expected {
        let mut cols = cols.clone();
        for (t, c, not_null) in APPROVED_EXTRA_COLUMNS {
            if t == table {
                cols.push((c.to_string(), *not_null));
            }
        }
        assert_eq!(sqlite_columns(&db, table).await?, cols, "columns / NOT NULL of {table}");
    }
    Ok(())
}

#[tokio::test]
async fn every_reference_foreign_key_and_unique_constraint_exists() -> AppResult<()> {
    let (_dir, db) = fresh().await?;
    let mut fk_checked = 0;
    let mut uq_checked = 0;
    for line in PG_CONSTRAINTS.lines().filter(|l| !l.trim().is_empty()) {
        let f: Vec<&str> = line.splitn(3, '|').collect();
        let (table, def) = (f[0], f[2]);
        let cols_in = |s: &str| -> Vec<String> {
            let inner = s.split_once('(').and_then(|(_, r)| r.split_once(')')).map(|(c, _)| c).unwrap_or("");
            inner.split(',').map(|c| c.trim().trim_matches('"').to_string()).collect()
        };
        if let Some(rest) = def.strip_prefix("FOREIGN KEY ") {
            let column = cols_in(rest)[0].clone();
            let target = rest.split("REFERENCES ").nth(1).and_then(|r| r.split('(').next()).unwrap_or("").trim();
            let on_delete = if def.contains("ON DELETE CASCADE") {
                "CASCADE"
            } else if def.contains("ON DELETE SET NULL") {
                "SET NULL"
            } else {
                "NO ACTION"
            };
            let found: Option<(String, String)> = sqlx::query_as(&format!(
                "SELECT \"table\", on_delete FROM pragma_foreign_key_list('{table}') WHERE \"from\" = ?1"
            ))
            .bind(&column)
            .fetch_optional(db.read())
            .await?;
            assert_eq!(found, Some((target.to_string(), on_delete.to_string())), "FK {table}.{column}");
            fk_checked += 1;
        } else if def.starts_with("UNIQUE") {
            let cols = cols_in(def);
            let indexes: Vec<String> = sqlx::query_scalar(&format!(
                "SELECT name FROM pragma_index_list('{table}') WHERE \"unique\" = 1 AND partial = 0"
            ))
            .fetch_all(db.read())
            .await?;
            let mut matched = false;
            for idx in indexes {
                let idx_cols: Vec<String> =
                    sqlx::query_scalar(&format!("SELECT name FROM pragma_index_info('{idx}') ORDER BY seqno"))
                        .fetch_all(db.read())
                        .await?;
                matched |= idx_cols == cols;
            }
            assert!(matched, "UNIQUE {table} {cols:?}");
            uq_checked += 1;
        }
    }
    assert_eq!((fk_checked, uq_checked), (19, 11));

    // Expression / partial unique indexes keep their reference names.
    for name in ["UQ_employees_employeeId_ci", "UQ_payroll_runs_period_live"] {
        assert!(PG_INDEXES.contains(name));
        let n: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' AND name = ?1")
            .bind(name)
            .fetch_one(db.read())
            .await?;
        assert_eq!(n, 1, "{name}");
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
    // D25: the seeded customer details are blanked on a fresh install.
    assert_eq!(settings.get("signatoryName").map(String::as_str), Some(""));
    assert_eq!(settings.get("payroll.companyName").map(String::as_str), Some(""));
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

/// Upgrade path: a database created by an earlier release keeps its data when the newer
/// migrations run. Each prefix of the migration list is applied, data is written, and then the
/// full list runs on top.
#[tokio::test]
async fn every_earlier_schema_version_upgrades_cleanly() -> AppResult<()> {
    let all: Vec<sqlx::migrate::Migration> = MIGRATOR.iter().cloned().collect();
    assert!(all.len() >= 2);
    for applied in 1..all.len() {
        let dir = tempfile::tempdir().map_err(AppError::internal)?;
        let path = dir.path().join("old.db");
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(base_options(&path).create_if_missing(true))
            .await?;
        let older = sqlx::migrate::Migrator {
            migrations: std::borrow::Cow::Owned(all[..applied].to_vec()),
            ignore_missing: false,
            locking: true,
            no_tx: false,
        };
        older.run(&pool).await?;
        sqlx::query(r#"INSERT INTO "vendors" ("id","name") VALUES ('v1','Paper Co')"#).execute(&pool).await?;
        sqlx::query(
            r#"INSERT INTO "bills" ("id","billNumber","vendorId","date","status","totalAmount") VALUES ('b1','BILL-000001','v1','2026-04-03','POSTED',5750)"#,
        )
        .execute(&pool)
        .await?;
        pool.close().await;

        let db = Db::open(&path).await?;
        let (total, paid): (i64, i64) =
            sqlx::query_as(r#"SELECT "totalAmount", "amountPaid" FROM "bills" WHERE "id" = 'b1'"#)
                .fetch_one(db.read())
                .await?;
        assert_eq!((total, paid), (5750, 0), "after upgrading from {applied} migration(s)");
        let versions: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM _sqlx_migrations").fetch_one(db.read()).await?;
        assert_eq!(versions, i64::try_from(all.len()).map_err(AppError::internal)?);
        db.close().await;
    }
    Ok(())
}

#[tokio::test]
async fn an_older_database_is_copied_before_it_is_upgraded() -> AppResult<()> {
    let all: Vec<sqlx::migrate::Migration> = MIGRATOR.iter().cloned().collect();
    let dir = tempfile::tempdir().map_err(AppError::internal)?;
    let path = dir.path().join("accounting.db");
    let copies = dir.path().join("pre-upgrade");
    assert!(Db::snapshot_before_upgrade(&path, &copies).await?.is_none(), "no database yet");

    let pool = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(base_options(&path).create_if_missing(true))
        .await?;
    let older = sqlx::migrate::Migrator {
        migrations: std::borrow::Cow::Owned(all[..1].to_vec()),
        ignore_missing: false,
        locking: true,
        no_tx: false,
    };
    older.run(&pool).await?;
    sqlx::query(r#"INSERT INTO "vendors" ("id","name") VALUES ('v1','Paper Co')"#).execute(&pool).await?;
    pool.close().await;

    let copy = Db::snapshot_before_upgrade(&path, &copies).await?.ok_or_else(|| AppError::internal("no copy"))?;
    let check = sqlx::sqlite::SqlitePoolOptions::new()
        .connect_with(sqlx::sqlite::SqliteConnectOptions::new().filename(&copy).read_only(true))
        .await?;
    let name: String = sqlx::query_scalar(r#"SELECT "name" FROM "vendors""#).fetch_one(&check).await?;
    assert_eq!(name, "Paper Co");
    check.close().await;

    let db = Db::open(&path).await?;
    db.close().await;
    assert!(Db::snapshot_before_upgrade(&path, &copies).await?.is_none(), "up to date: nothing copied");
    Ok(())
}
