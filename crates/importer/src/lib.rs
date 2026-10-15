//! One-time import from the reference PostgreSQL database (W6).
//!
//! * Reads the old database over a normal PostgreSQL connection (read-only queries; works
//!   against the live server or a dump restored into any PostgreSQL).
//! * Writes into a **new, empty** data folder — never into a live installation. The whole
//!   import is one SQLite transaction: it either completes or leaves nothing. Primary keys are
//!   kept, so every reference between records (employees, payroll, invoices, payments,
//!   documents, journal entries) survives unchanged, and running it twice into the same folder
//!   is refused instead of duplicating data.
//! * Historical transactions are copied, never re-posted: journal entries and lines arrive
//!   exactly as they were, including reversed ones.
//! * Afterwards it reconciles source and target and writes `import-report.md` / `.json`. The
//!   import only counts as successful when the report has no unexplained discrepancy.

pub mod convert;

use app_core::money::Money;
use chrono::FixedOffset;
use convert::{convert, Value};
use database::Db;
use serde::Serialize;
use sqlx::postgres::PgConnection;
use sqlx::{Connection, Row};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};

pub struct Options {
    /// `postgres://user@host:port/db` (the password may come from `PGPASSWORD`).
    pub source: String,
    /// The old server's time zone, as an offset: `timestamp without time zone` values are
    /// wall-clock times there.
    pub source_offset: FixedOffset,
    pub target_dir: PathBuf,
    /// Folder holding the old system's stored files under their storage keys (the local
    /// storage folder, or an S3 bucket synced to disk).
    pub files_dir: Option<PathBuf>,
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TableCount {
    pub table: String,
    pub source: i64,
    pub target: i64,
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PayrollCheck {
    pub lines: usize,
    pub identical: usize,
    /// Lines that differ only by one-cent exact-tie rounding (approved deviation D4).
    pub exact_ties: Vec<String>,
    pub mismatches: Vec<String>,
    pub run_total_mismatches: Vec<String>,
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileCheck {
    pub referenced: usize,
    pub copied: usize,
    pub verified: usize,
    pub stored_unencrypted: usize,
    pub missing: Vec<String>,
    pub unreadable: Vec<String>,
    pub size_mismatch: Vec<String>,
}

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub started_at: String,
    pub finished_at: String,
    pub source_offset: String,
    pub target: String,
    pub tables: Vec<TableCount>,
    pub source_tables_not_imported: Vec<String>,
    pub source_columns_not_imported: Vec<String>,
    pub money_columns_checked: usize,
    pub money_mismatches: Vec<String>,
    pub ledger_accounts_checked: usize,
    pub ledger_mismatches: Vec<String>,
    pub unbalanced_entries: Vec<String>,
    pub integrity: String,
    pub foreign_key_problems: usize,
    pub bills_amount_paid_backfilled: i64,
    pub payroll: PayrollCheck,
    pub files: FileCheck,
    pub discrepancies: Vec<String>,
    pub success: bool,
}

/// TypeORM bookkeeping that has no counterpart here.
const SOURCE_ONLY_OK: &[&str] = &["migrations", "typeorm_metadata"];

fn q(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

struct SourceColumn {
    name: String,
    data_type: String,
}

async fn source_inventory(pg: &mut PgConnection) -> anyhow::Result<BTreeMap<String, Vec<SourceColumn>>> {
    let rows = sqlx::query(
        "SELECT table_name::text, column_name::text, data_type::text FROM information_schema.columns
         WHERE table_schema = 'public' ORDER BY table_name, ordinal_position",
    )
    .fetch_all(&mut *pg)
    .await?;
    let mut out: BTreeMap<String, Vec<SourceColumn>> = BTreeMap::new();
    for r in rows {
        out.entry(r.try_get(0)?).or_default().push(SourceColumn { name: r.try_get(1)?, data_type: r.try_get(2)? });
    }
    Ok(out)
}

struct TargetColumn {
    name: String,
    sqlite_type: String,
}

async fn target_tables(db: &Db) -> anyhow::Result<Vec<(String, Vec<TargetColumn>)>> {
    let names: Vec<String> = sqlx::query_scalar(
        "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' AND name <> '_sqlx_migrations' ORDER BY rowid",
    )
    .fetch_all(db.read())
    .await?;
    let mut out = Vec::new();
    for n in names {
        let cols: Vec<(String, String)> = sqlx::query_as("SELECT name, type FROM pragma_table_info(?1) ORDER BY cid")
            .bind(&n)
            .fetch_all(db.read())
            .await?;
        out.push((n, cols.into_iter().map(|(name, sqlite_type)| TargetColumn { name, sqlite_type }).collect()));
    }
    Ok(out)
}

/// A fresh installation holds only migration seeds: no users, entries, employees or documents.
async fn ensure_fresh(db: &Db) -> anyhow::Result<()> {
    for t in ["users", "journal_entries", "employees", "invoices", "payroll_runs", "audit_logs"] {
        let n: i64 = sqlx::query_scalar(&format!("SELECT COUNT(*) FROM {}", q(t))).fetch_one(db.read()).await?;
        if n > 0 {
            anyhow::bail!("the target already holds data ({t}: {n} rows); import into a new, empty data folder");
        }
    }
    Ok(())
}

fn safe_key(key: &str) -> bool {
    !key.is_empty() && !key.contains('\\') && Path::new(key).components().all(|c| matches!(c, Component::Normal(_)))
}

pub async fn run(opts: &Options) -> anyhow::Result<Report> {
    let mut report = Report {
        started_at: app_core::time::now_iso(),
        source_offset: opts.source_offset.to_string(),
        target: opts.target_dir.to_string_lossy().to_string(),
        ..Report::default()
    };
    let db_path = opts.target_dir.join("accounting.db");
    if db_path.exists() {
        anyhow::bail!("{} already exists; import into a new, empty data folder", db_path.display());
    }
    let key_path = opts.target_dir.join("secrets").join(app_core::secrets::STORAGE_KEY_FILE);
    if opts.files_dir.is_some() && !key_path.exists() {
        anyhow::bail!(
            "install the old system's file-encryption key first (accounting-server import-storage-key with \
             ACCOUNTING_DATA_DIR set to the target folder), so the documents can be checked after copying"
        );
    }

    let mut pg = PgConnection::connect(&opts.source).await?;
    for stmt in ["SET DateStyle = 'ISO, YMD'", "SET TimeZone = 'UTC'", "SET extra_float_digits = 3"] {
        sqlx::query(stmt).execute(&mut pg).await?;
    }
    sqlx::query("SET default_transaction_read_only = on").execute(&mut pg).await?;
    let mut tx_source = pg.begin().await?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY").execute(&mut *tx_source).await?;

    let source = source_inventory(&mut tx_source).await?;
    let db = Db::open(&db_path).await?;
    ensure_fresh(&db).await?;
    let targets = target_tables(&db).await?;
    let target_names: BTreeSet<&str> = targets.iter().map(|(n, _)| n.as_str()).collect();
    for t in source.keys() {
        if !target_names.contains(t.as_str()) && !SOURCE_ONLY_OK.contains(&t.as_str()) {
            report.source_tables_not_imported.push(t.clone());
        }
    }

    let mut tx = db.begin().await?;
    sqlx::query("PRAGMA defer_foreign_keys = ON").execute(&mut *tx).await?;
    let mut money_columns: Vec<(String, String)> = Vec::new();
    for (table, tcols) in &targets {
        let Some(scols) = source.get(table) else { continue };
        let pg_types: BTreeMap<&str, &str> = scols.iter().map(|c| (c.name.as_str(), c.data_type.as_str())).collect();
        let shared: Vec<&TargetColumn> = tcols.iter().filter(|c| pg_types.contains_key(c.name.as_str())).collect();
        for c in scols {
            if !tcols.iter().any(|t| t.name == c.name) {
                report.source_columns_not_imported.push(format!("{table}.{}", c.name));
            }
        }
        for c in &shared {
            if pg_types[c.name.as_str()] == "numeric" && c.sqlite_type.eq_ignore_ascii_case("INTEGER") {
                money_columns.push((table.clone(), c.name.clone()));
            }
        }
        let upsert = table == "settings";
        if !upsert {
            // Only migration seeds can be here (checked above); the source's own rows replace them.
            sqlx::query(&format!("DELETE FROM {}", q(table))).execute(&mut *tx).await?;
        }
        let select = format!(
            "SELECT {} FROM {}",
            shared.iter().map(|c| format!("{}::text", q(&c.name))).collect::<Vec<_>>().join(", "),
            q(table)
        );
        let insert = format!(
            "INSERT {}INTO {} ({}) VALUES ({})",
            if upsert { "OR REPLACE " } else { "" },
            q(table),
            shared.iter().map(|c| q(&c.name)).collect::<Vec<_>>().join(","),
            (1..=shared.len()).map(|i| format!("?{i}")).collect::<Vec<_>>().join(",")
        );
        let rows = sqlx::query(&select).fetch_all(&mut *tx_source).await?;
        for row in &rows {
            let mut stmt = sqlx::query(&insert);
            for (i, c) in shared.iter().enumerate() {
                let raw: Option<String> = row.try_get(i)?;
                stmt = match convert(
                    table,
                    &c.name,
                    pg_types[c.name.as_str()],
                    &c.sqlite_type,
                    raw.as_deref(),
                    opts.source_offset,
                )? {
                    Value::Null => stmt.bind(None::<String>),
                    Value::Int(n) => stmt.bind(n),
                    Value::Text(t) => stmt.bind(t),
                };
            }
            stmt.execute(&mut *tx).await.map_err(|e| anyhow::anyhow!("inserting into {table}: {e}"))?;
        }
        report.tables.push(TableCount { table: table.clone(), source: rows.len() as i64, target: 0 });
    }
    // D13: bills accumulate their AP payments in a column the reference did not have.
    let backfilled = sqlx::query(
        r#"UPDATE "bills" SET "amountPaid" = (SELECT COALESCE(SUM(p."amount"), 0) FROM "payments" p
           WHERE p."billId" = "bills"."id" AND p."type" = 'AP')"#,
    )
    .execute(&mut *tx)
    .await?;
    report.bills_amount_paid_backfilled = i64::try_from(backfilled.rows_affected()).unwrap_or(0);
    tx.commit()
        .await
        .map_err(|e| anyhow::anyhow!("committing the import (a reference between records is broken?): {e}"))?;

    copy_files(opts, &db, &mut report).await?;
    reconcile(&mut tx_source, &db, &money_columns, &mut report).await?;
    tx_source.rollback().await?;
    db.close().await;

    report.finished_at = app_core::time::now_iso();
    report.success = report.discrepancies.is_empty();
    write_report(&opts.target_dir, &report)?;
    Ok(report)
}

async fn copy_files(opts: &Options, db: &Db, report: &mut Report) -> anyhow::Result<()> {
    let referenced: Vec<(String, Option<String>, Option<i64>)> = sqlx::query_as(
        r#"SELECT "photoKey", "photoMime", NULL FROM "employees" WHERE "photoKey" IS NOT NULL
           UNION ALL SELECT "fileKey", "mimeType", "size" FROM "employee_documents""#,
    )
    .fetch_all(db.read())
    .await?;
    report.files.referenced = referenced.len();
    let Some(src_root) = &opts.files_dir else {
        if !referenced.is_empty() {
            report.discrepancies.push(format!(
                "{} stored files are referenced but no --files folder was given; documents were not imported",
                referenced.len()
            ));
        }
        return Ok(());
    };
    let raw = app_core::secrets::load_or_create(
        &opts.target_dir.join("secrets").join(app_core::secrets::STORAGE_KEY_FILE),
        32,
    )?;
    let key = documents::StorageKey::from_bytes(&raw).map_err(|e| anyhow::anyhow!("{e}"))?;
    let store = documents::FileStore::new(opts.target_dir.join("files"), key);
    for (file_key, mime, size) in referenced {
        if !safe_key(&file_key) {
            report.files.missing.push(format!("{file_key} (unsafe storage key, not copied)"));
            continue;
        }
        let src = src_root.join(&file_key);
        let Ok(bytes) = std::fs::read(&src) else {
            report.files.missing.push(file_key);
            continue;
        };
        let dest = opts.target_dir.join("files").join(&file_key);
        if let Some(d) = dest.parent() {
            std::fs::create_dir_all(d)?;
        }
        std::fs::write(&dest, &bytes)?;
        let mut meta = dest.as_os_str().to_owned();
        meta.push(".meta");
        std::fs::write(PathBuf::from(meta), mime.unwrap_or_else(|| "application/octet-stream".into()))?;
        report.files.copied += 1;
        if !documents::is_encrypted(&bytes) {
            report.files.stored_unencrypted += 1;
        }
        match store.get(&file_key) {
            Ok(f) => {
                if let Some(expected) = size {
                    if i64::try_from(f.bytes.len()).ok() != Some(expected) {
                        report
                            .files
                            .size_mismatch
                            .push(format!("{file_key}: {} bytes, record says {expected}", f.bytes.len()));
                        continue;
                    }
                }
                report.files.verified += 1;
            }
            Err(_) => report.files.unreadable.push(file_key),
        }
    }
    let f = &report.files;
    if !f.missing.is_empty() {
        report.discrepancies.push(format!("{} stored files are missing from the files folder", f.missing.len()));
    }
    if !f.unreadable.is_empty() {
        report.discrepancies.push(format!("{} files cannot be decrypted with the installed key", f.unreadable.len()));
    }
    if !f.size_mismatch.is_empty() {
        report.discrepancies.push(format!("{} files differ in size from their records", f.size_mismatch.len()));
    }
    Ok(())
}

fn parse_money(text: &str) -> Option<Money> {
    Money::parse_decimal(text)
}

async fn reconcile(
    pg: &mut PgConnection,
    db: &Db,
    money_columns: &[(String, String)],
    report: &mut Report,
) -> anyhow::Result<()> {
    // Record counts.
    for t in &mut report.tables {
        t.target = sqlx::query_scalar(&format!("SELECT COUNT(*) FROM {}", q(&t.table))).fetch_one(db.read()).await?;
        let ok = if t.table == "settings" { t.target >= t.source } else { t.target == t.source };
        if !ok {
            report.discrepancies.push(format!("{}: {} rows in the source, {} imported", t.table, t.source, t.target));
        }
    }
    // Every money / quantity column, summed on both sides.
    for (table, col) in money_columns {
        let src: String = sqlx::query_scalar(&format!("SELECT COALESCE(SUM({}), 0)::text FROM {}", q(col), q(table)))
            .fetch_one(&mut *pg)
            .await?;
        let dst: i64 = sqlx::query_scalar(&format!("SELECT COALESCE(SUM({}), 0) FROM {}", q(col), q(table)))
            .fetch_one(db.read())
            .await?;
        report.money_columns_checked += 1;
        if parse_money(&src).map(Money::cents) != Some(dst) {
            report.money_mismatches.push(format!("{table}.{col}: source {src}, imported {}", Money::from_cents(dst)));
        }
    }
    if !report.money_mismatches.is_empty() {
        report
            .discrepancies
            .push(format!("{} money columns do not sum to the same totals", report.money_mismatches.len()));
    }
    // Ledger per account.
    let src_rows = sqlx::query(
        r#"SELECT "accountId"::text, SUM("debit")::text, SUM("credit")::text FROM "journal_lines" GROUP BY 1"#,
    )
    .fetch_all(&mut *pg)
    .await?;
    let dst: BTreeMap<String, (i64, i64)> = sqlx::query_as::<_, (String, i64, i64)>(
        r#"SELECT "accountId", SUM("debit"), SUM("credit") FROM "journal_lines" GROUP BY 1"#,
    )
    .fetch_all(db.read())
    .await?
    .into_iter()
    .map(|(a, d, c)| (a, (d, c)))
    .collect();
    for r in &src_rows {
        let account: String = r.try_get(0)?;
        let d: String = r.try_get(1)?;
        let c: String = r.try_get(2)?;
        report.ledger_accounts_checked += 1;
        let expected = (parse_money(&d).map(Money::cents), parse_money(&c).map(Money::cents));
        let got = dst.get(&account).map(|(d, c)| (Some(*d), Some(*c)));
        if got != Some(expected) {
            report.ledger_mismatches.push(format!("account {account}: source debit {d} credit {c}"));
        }
    }
    if dst.len() != src_rows.len() {
        report.ledger_mismatches.push(format!(
            "{} accounts have lines in the source, {} imported",
            src_rows.len(),
            dst.len()
        ));
    }
    if !report.ledger_mismatches.is_empty() {
        report.discrepancies.push(format!("{} ledger account totals differ", report.ledger_mismatches.len()));
    }
    report.unbalanced_entries = sqlx::query_scalar(
        r#"SELECT e."entryNumber" FROM "journal_entries" e JOIN "journal_lines" l ON l."journalEntryId" = e."id"
           GROUP BY e."id" HAVING SUM(l."debit") <> SUM(l."credit")"#,
    )
    .fetch_all(db.read())
    .await?;
    if !report.unbalanced_entries.is_empty() {
        report.discrepancies.push(format!("{} journal entries do not balance", report.unbalanced_entries.len()));
    }
    report.integrity = sqlx::query_scalar("PRAGMA integrity_check").fetch_one(db.read()).await?;
    if report.integrity != "ok" {
        report.discrepancies.push(format!("database integrity check: {}", report.integrity));
    }
    let fk: Vec<(String,)> =
        sqlx::query_as(r#"SELECT "table" FROM pragma_foreign_key_check"#).fetch_all(db.read()).await?;
    report.foreign_key_problems = fk.len();
    if !fk.is_empty() {
        report.discrepancies.push(format!("{} broken references between records", fk.len()));
    }
    check_payroll(db, report).await?;
    Ok(())
}

/// id, run number, status, rate snapshot, gross, net, deductions, employee count.
type RunRow = (String, String, String, String, i64, i64, i64, i64);

async fn check_payroll(db: &Db, report: &mut Report) -> anyhow::Result<()> {
    use payroll::rates::{compute_line_with, LineFigures, LineInputs, Rates, Rounding};
    let runs: Vec<RunRow> = sqlx::query_as(
        r#"SELECT "id","runNumber","status","ratesSnapshot","grossTotal","netTotal","deductionsTotal","employeeCount" FROM "payroll_runs""#,
    )
    .fetch_all(db.read())
    .await?;
    for (id, number, status, snapshot, gross, net, deductions, count) in runs {
        let rates = Rates::from_json(&serde_json::from_str(&snapshot).unwrap_or(serde_json::Value::Null));
        let rows =
            sqlx::query(r#"SELECT * FROM "payroll_lines" WHERE "runId" = ?1"#).bind(&id).fetch_all(db.read()).await?;
        let (mut g, mut n, mut d) = (0i64, 0i64, 0i64);
        for r in &rows {
            let m = |c: &str| -> anyhow::Result<Money> { Ok(Money::from_cents(r.try_get::<i64, _>(c)?)) };
            let inputs = LineInputs {
                epf_applicable: r.try_get::<i64, _>("epfApplicable")? != 0,
                days_in_month: r.try_get("daysInMonth")?,
                days_worked: r.try_get("daysWorked")?,
                basic: m("basic")?,
                bra2005: m("bra2005")?,
                bra2016: m("bra2016")?,
                vehicle_allowance: m("vehicleAllowance")?,
                other_allowance: m("otherAllowance")?,
                project_bonus: m("projectBonus")?,
                arrears: m("arrears")?,
                medical: m("medical")?,
                other_earnings: m("otherEarnings")?,
                no_pay_amount: m("noPayAmount")?,
                paye_override: r.try_get::<Option<i64>, _>("payeOverride")?.map(Money::from_cents),
                welfare: m("welfare")?,
                loan_recovery: m("loanRecovery")?,
                salary_advance: m("salaryAdvance")?,
                phone_bill: m("phoneBill")?,
                other_deductions: m("otherDeductions")?,
            };
            let stored = LineFigures {
                basic_for_month: m("basicForMonth")?,
                earnings_total: m("earningsTotal")?,
                gross_salary: m("grossSalary")?,
                epf_base: m("epfBase")?,
                epf_employee: m("epfEmployee")?,
                paye: m("paye")?,
                stamp_duty: m("stampDuty")?,
                deductions_total: m("deductionsTotal")?,
                net_salary: m("netSalary")?,
                epf_employer: m("epfEmployer")?,
                etf: m("etf")?,
                employer_welfare: m("employerWelfare")?,
                cost_to_company: m("costToCompany")?,
            };
            g += stored.gross_salary.cents();
            n += stored.net_salary.cents();
            d += stored.deductions_total.cents();
            let slip: String = r.try_get("slipNumber")?;
            report.payroll.lines += 1;
            if compute_line_with(&inputs, &rates, Rounding::HalfUp)? == stored {
                report.payroll.identical += 1;
            } else if compute_line_with(&inputs, &rates, Rounding::HalfDown)? == stored {
                report.payroll.exact_ties.push(slip);
            } else {
                report.payroll.mismatches.push(format!("{slip} ({status})"));
            }
        }
        if (g, n, d, count) != (gross, net, deductions, rows.len() as i64) {
            report.payroll.run_total_mismatches.push(number);
        }
    }
    if !report.payroll.mismatches.is_empty() {
        report.discrepancies.push(format!(
            "{} payroll lines do not recompute to their stored figures",
            report.payroll.mismatches.len()
        ));
    }
    if !report.payroll.run_total_mismatches.is_empty() {
        report.discrepancies.push(format!(
            "{} payroll runs' totals differ from the sum of their lines",
            report.payroll.run_total_mismatches.len()
        ));
    }
    Ok(())
}

fn list(items: &[String]) -> String {
    if items.is_empty() {
        "none".into()
    } else {
        items.iter().take(50).cloned().collect::<Vec<_>>().join(", ") + if items.len() > 50 { ", …" } else { "" }
    }
}

pub fn render_markdown(r: &Report) -> String {
    let mut s = String::new();
    s.push_str("# Import reconciliation report\n\n");
    s.push_str(&format!(
        "Result: **{}**\n\nStarted {} · finished {} · source time zone offset {} · target `{}`\n\n",
        if r.success { "reconciled, no unexplained discrepancies" } else { "NOT reconciled — see discrepancies" },
        r.started_at,
        r.finished_at,
        r.source_offset,
        r.target
    ));
    s.push_str("## Discrepancies\n\n");
    if r.discrepancies.is_empty() {
        s.push_str("None.\n\n");
    } else {
        for d in &r.discrepancies {
            s.push_str(&format!("- {d}\n"));
        }
        s.push('\n');
    }
    s.push_str("## Records\n\n| Table | Source | Imported |\n|---|---:|---:|\n");
    for t in &r.tables {
        s.push_str(&format!("| {} | {} | {} |\n", t.table, t.source, t.target));
    }
    s.push_str(&format!(
        "\nSource tables not imported: {}. Source columns not imported: {}.\n\n",
        list(&r.source_tables_not_imported),
        list(&r.source_columns_not_imported)
    ));
    s.push_str("## Checks\n\n");
    s.push_str(&format!(
        "- Money and quantity columns summed on both sides: {} checked, mismatches: {}\n",
        r.money_columns_checked,
        list(&r.money_mismatches)
    ));
    s.push_str(&format!(
        "- Ledger debit/credit totals per account: {} accounts, mismatches: {}\n",
        r.ledger_accounts_checked,
        list(&r.ledger_mismatches)
    ));
    s.push_str(&format!("- Unbalanced journal entries: {}\n", list(&r.unbalanced_entries)));
    s.push_str(&format!("- SQLite integrity check: {}; broken references: {}\n", r.integrity, r.foreign_key_problems));
    s.push_str(&format!("- Bills given an accumulated amount paid (D13): {}\n", r.bills_amount_paid_backfilled));
    let p = &r.payroll;
    s.push_str(&format!(
        "- Payroll lines recomputed: {}; identical {}; one-cent exact ties (D4): {}; mismatches: {}; run totals not matching their lines: {}\n",
        p.lines,
        p.identical,
        list(&p.exact_ties),
        list(&p.mismatches),
        list(&p.run_total_mismatches)
    ));
    let f = &r.files;
    s.push_str(&format!(
        "- Stored files: {} referenced, {} copied, {} decrypted and checked, {} stored unencrypted by the old system; missing: {}; unreadable: {}; wrong size: {}\n",
        f.referenced,
        f.copied,
        f.verified,
        f.stored_unencrypted,
        list(&f.missing),
        list(&f.unreadable),
        list(&f.size_mismatch)
    ));
    s
}

fn write_report(dir: &Path, r: &Report) -> anyhow::Result<()> {
    std::fs::write(dir.join("import-report.json"), serde_json::to_vec_pretty(r)?)?;
    std::fs::write(dir.join("import-report.md"), render_markdown(r))?;
    Ok(())
}
