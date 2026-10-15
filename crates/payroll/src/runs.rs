//! Payroll runs (reference `payroll-runs.service.ts`).
//!
//! Every step that changes money runs in one write transaction together with its journal
//! entries and loan-balance changes, so a run is never half-posted or half-voided (the
//! reference wrote these in separate statements). Status checks happen inside that
//! transaction, so a retried request is refused instead of posting twice.
//!
//! JSON follows the reference: columns read back are 2-dp strings (`"83850.00"`), figures it
//! computed in JS are numbers, and a line returned by an edit mixes both, exactly as TypeORM
//! returned the edited entity.

use crate::rates::{compute_line, rates_from_settings, LineFigures, LineInputs, Rates};
use app_core::ids::new_id;
use app_core::money::Money;
use app_core::time::{now_iso, today_local};
use app_core::{AppError, AppResult};
use database::{counters, Db, Tx};
use employees::Actor;
use ledger::accounts;
use ledger::entries::{self, NewEntry, NewLine};
use serde_json::{json, Map, Value};
use sqlx::sqlite::SqliteRow;
use sqlx::{Row, SqliteConnection};

pub const REMITTANCE_TYPES: &[&str] = &["EPF", "ETF", "PAYE", "STAMP_DUTY", "WELFARE"];

/// Chart-of-accounts codes the postings use (seeded by the payroll migration).
mod acc {
    pub const STAFF_LOANS: &str = "1300";
    pub const SALARIES_PAYABLE: &str = "2200";
    pub const EPF_PAYABLE: &str = "2210";
    pub const ETF_PAYABLE: &str = "2220";
    pub const PAYE_PAYABLE: &str = "2230";
    pub const STAMP_DUTY_PAYABLE: &str = "2240";
    pub const WELFARE_FUND: &str = "2250";
    pub const STAFF_DEDUCTIONS_PAYABLE: &str = "2260";
    pub const SALARIES_EXPENSE: &str = "6100";
    pub const EMPLOYER_CONTRIBUTIONS: &str = "6110";
    pub const WELFARE_EXPENSE: &str = "6120";
}

fn remit_payable(kind: &str) -> &'static str {
    match kind {
        "EPF" => acc::EPF_PAYABLE,
        "ETF" => acc::ETF_PAYABLE,
        "PAYE" => acc::PAYE_PAYABLE,
        "STAMP_DUTY" => acc::STAMP_DUTY_PAYABLE,
        _ => acc::WELFARE_FUND,
    }
}

fn too_large() -> AppError {
    AppError::bad_request("Amount is too large")
}

fn add(a: Money, b: Money) -> AppResult<Money> {
    a.checked_add(b).ok_or_else(too_large)
}

fn opt(v: &Option<String>) -> Value {
    v.clone().map(Value::String).unwrap_or(Value::Null)
}

// ─── Runs ───────────────────────────────────────────────────────────────────

/// The run totals, in the reference's column order.
pub const RUN_TOTALS: &[&str] = &[
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
];

#[derive(Debug, Clone)]
pub struct Run {
    pub id: String,
    pub run_number: String,
    pub period: String,
    pub period_start: String,
    pub period_end: String,
    pub status: String,
    pub notes: Option<String>,
    pub rates_snapshot: Value,
    pub employee_count: i64,
    pub totals: Vec<(&'static str, Money)>,
    pub journal_entry_id: Option<String>,
    pub payment_journal_entry_id: Option<String>,
    pub payment_account_id: Option<String>,
    pub pay_date: Option<String>,
    pub posted_at: Option<String>,
    pub paid_at: Option<String>,
    pub slips_filed_at: Option<String>,
    pub created_by_id: Option<String>,
    pub created_by_name: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl Run {
    pub fn total(&self, name: &str) -> Money {
        self.totals.iter().find(|(k, _)| *k == name).map(|(_, v)| *v).unwrap_or(Money::ZERO)
    }

    pub fn to_json(&self) -> Value {
        let mut o = Map::new();
        o.insert("id".into(), json!(self.id));
        o.insert("runNumber".into(), json!(self.run_number));
        o.insert("period".into(), json!(self.period));
        o.insert("periodStart".into(), json!(self.period_start));
        o.insert("periodEnd".into(), json!(self.period_end));
        o.insert("status".into(), json!(self.status));
        o.insert("notes".into(), opt(&self.notes));
        o.insert("ratesSnapshot".into(), self.rates_snapshot.clone());
        o.insert("employeeCount".into(), json!(self.employee_count));
        for (k, v) in &self.totals {
            o.insert((*k).into(), Value::String(v.to_db_string()));
        }
        o.insert("journalEntryId".into(), opt(&self.journal_entry_id));
        o.insert("paymentJournalEntryId".into(), opt(&self.payment_journal_entry_id));
        o.insert("paymentAccountId".into(), opt(&self.payment_account_id));
        o.insert("payDate".into(), opt(&self.pay_date));
        o.insert("postedAt".into(), opt(&self.posted_at));
        o.insert("paidAt".into(), opt(&self.paid_at));
        o.insert("slipsFiledAt".into(), opt(&self.slips_filed_at));
        o.insert("createdById".into(), opt(&self.created_by_id));
        o.insert("createdByName".into(), opt(&self.created_by_name));
        o.insert("createdAt".into(), json!(self.created_at));
        o.insert("updatedAt".into(), json!(self.updated_at));
        Value::Object(o)
    }
}

fn run_from_row(r: &SqliteRow) -> AppResult<Run> {
    let snapshot: String = r.try_get("ratesSnapshot")?;
    let mut totals = Vec::with_capacity(RUN_TOTALS.len());
    for k in RUN_TOTALS {
        totals.push((*k, Money::from_cents(r.try_get(*k)?)));
    }
    Ok(Run {
        id: r.try_get("id")?,
        run_number: r.try_get("runNumber")?,
        period: r.try_get("period")?,
        period_start: r.try_get("periodStart")?,
        period_end: r.try_get("periodEnd")?,
        status: r.try_get("status")?,
        notes: r.try_get("notes")?,
        rates_snapshot: serde_json::from_str(&snapshot).unwrap_or_else(|_| json!({})),
        employee_count: r.try_get("employeeCount")?,
        totals,
        journal_entry_id: r.try_get("journalEntryId")?,
        payment_journal_entry_id: r.try_get("paymentJournalEntryId")?,
        payment_account_id: r.try_get("paymentAccountId")?,
        pay_date: r.try_get("payDate")?,
        posted_at: r.try_get("postedAt")?,
        paid_at: r.try_get("paidAt")?,
        slips_filed_at: r.try_get("slipsFiledAt")?,
        created_by_id: r.try_get("createdById")?,
        created_by_name: r.try_get("createdByName")?,
        created_at: r.try_get("createdAt")?,
        updated_at: r.try_get("updatedAt")?,
    })
}

async fn get_run(conn: &mut SqliteConnection, id: &str) -> AppResult<Run> {
    let row = sqlx::query(r#"SELECT * FROM "payroll_runs" WHERE "id" = ?1"#).bind(id).fetch_optional(conn).await?;
    row.as_ref().map(run_from_row).transpose()?.ok_or_else(|| AppError::NotFound(format!("Payroll run {id} not found")))
}

async fn draft_only(conn: &mut SqliteConnection, id: &str) -> AppResult<Run> {
    let run = get_run(conn, id).await?;
    if run.status != "DRAFT" {
        return Err(AppError::bad_request(format!(
            "This run is {} and can no longer be changed — void it to redo the month",
            run.status
        )));
    }
    Ok(run)
}

/// All runs, newest month first.
pub async fn find_all(db: &Db) -> AppResult<Vec<Value>> {
    let rows = sqlx::query(r#"SELECT * FROM "payroll_runs" ORDER BY "period" DESC, "createdAt" DESC, rowid DESC"#)
        .fetch_all(db.read())
        .await?;
    rows.iter().map(|r| run_from_row(r).map(|run| run.to_json())).collect()
}

// ─── Lines ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct Line {
    pub id: String,
    pub run_id: String,
    pub employee_id: Option<String>,
    pub salary_id: Option<String>,
    pub slip_number: String,
    pub employee_code: String,
    pub employee_name: String,
    pub job_title: Option<String>,
    pub department: Option<String>,
    pub employment_type: String,
    pub epf_number: Option<String>,
    pub national_id: Option<String>,
    pub bank_name: Option<String>,
    pub bank_branch: Option<String>,
    pub bank_account_no: Option<String>,
    pub inputs: LineInputs,
    pub figures: LineFigures,
    pub notes: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// The money columns of a line, in the reference's column order.
fn line_money(l: &Line) -> [(&'static str, Money); 28] {
    let (i, f) = (&l.inputs, &l.figures);
    [
        ("basic", i.basic),
        ("basicForMonth", f.basic_for_month),
        ("bra2005", i.bra2005),
        ("bra2016", i.bra2016),
        ("vehicleAllowance", i.vehicle_allowance),
        ("otherAllowance", i.other_allowance),
        ("projectBonus", i.project_bonus),
        ("arrears", i.arrears),
        ("medical", i.medical),
        ("otherEarnings", i.other_earnings),
        ("earningsTotal", f.earnings_total),
        ("noPayAmount", i.no_pay_amount),
        ("grossSalary", f.gross_salary),
        ("epfBase", f.epf_base),
        ("epfEmployee", f.epf_employee),
        ("paye", f.paye),
        ("stampDuty", f.stamp_duty),
        ("welfare", i.welfare),
        ("loanRecovery", i.loan_recovery),
        ("salaryAdvance", i.salary_advance),
        ("phoneBill", i.phone_bill),
        ("otherDeductions", i.other_deductions),
        ("deductionsTotal", f.deductions_total),
        ("netSalary", f.net_salary),
        ("epfEmployer", f.epf_employer),
        ("etf", f.etf),
        ("employerWelfare", f.employer_welfare),
        ("costToCompany", f.cost_to_company),
    ]
}

/// The computed columns (`applyFigures` sets them as JS numbers).
const FIGURE_KEYS: &[&str] = &[
    "basicForMonth",
    "earningsTotal",
    "grossSalary",
    "epfBase",
    "epfEmployee",
    "paye",
    "stampDuty",
    "deductionsTotal",
    "netSalary",
    "epfEmployer",
    "etf",
    "employerWelfare",
    "costToCompany",
];

impl Line {
    fn col(&self, name: &str) -> Money {
        line_money(self).iter().find(|(k, _)| *k == name).map(|(_, v)| *v).unwrap_or(Money::ZERO)
    }

    /// The row as read back: every numeric column a 2-dp string.
    pub fn to_json(&self) -> Value {
        let mut o = Map::new();
        o.insert("id".into(), json!(self.id));
        o.insert("runId".into(), json!(self.run_id));
        o.insert("employeeId".into(), opt(&self.employee_id));
        o.insert("salaryId".into(), opt(&self.salary_id));
        o.insert("slipNumber".into(), json!(self.slip_number));
        o.insert("employeeCode".into(), json!(self.employee_code));
        o.insert("employeeName".into(), json!(self.employee_name));
        o.insert("jobTitle".into(), opt(&self.job_title));
        o.insert("department".into(), opt(&self.department));
        o.insert("employmentType".into(), json!(self.employment_type));
        o.insert("epfNumber".into(), opt(&self.epf_number));
        o.insert("nationalId".into(), opt(&self.national_id));
        o.insert("bankName".into(), opt(&self.bank_name));
        o.insert("bankBranch".into(), opt(&self.bank_branch));
        o.insert("bankAccountNo".into(), opt(&self.bank_account_no));
        o.insert("epfApplicable".into(), json!(self.inputs.epf_applicable));
        o.insert("daysInMonth".into(), json!(self.inputs.days_in_month));
        o.insert("daysWorked".into(), json!(Money::from_cents(self.inputs.days_worked).to_db_string()));
        for (k, v) in line_money(self) {
            o.insert(k.into(), json!(v.to_db_string()));
        }
        o.insert(
            "payeOverride".into(),
            self.inputs.paye_override.map(|p| json!(p.to_db_string())).unwrap_or(Value::Null),
        );
        o.insert("notes".into(), opt(&self.notes));
        o.insert("createdAt".into(), json!(self.created_at));
        o.insert("updatedAt".into(), json!(self.updated_at));
        Value::Object(o)
    }
}

fn line_from_row(r: &SqliteRow) -> AppResult<Line> {
    let m = |c: &str| -> AppResult<Money> { Ok(Money::from_cents(r.try_get(c)?)) };
    let paye_override: Option<i64> = r.try_get("payeOverride")?;
    Ok(Line {
        id: r.try_get("id")?,
        run_id: r.try_get("runId")?,
        employee_id: r.try_get("employeeId")?,
        salary_id: r.try_get("salaryId")?,
        slip_number: r.try_get("slipNumber")?,
        employee_code: r.try_get("employeeCode")?,
        employee_name: r.try_get("employeeName")?,
        job_title: r.try_get("jobTitle")?,
        department: r.try_get("department")?,
        employment_type: r.try_get("employmentType")?,
        epf_number: r.try_get("epfNumber")?,
        national_id: r.try_get("nationalId")?,
        bank_name: r.try_get("bankName")?,
        bank_branch: r.try_get("bankBranch")?,
        bank_account_no: r.try_get("bankAccountNo")?,
        inputs: LineInputs {
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
            paye_override: paye_override.map(Money::from_cents),
            welfare: m("welfare")?,
            loan_recovery: m("loanRecovery")?,
            salary_advance: m("salaryAdvance")?,
            phone_bill: m("phoneBill")?,
            other_deductions: m("otherDeductions")?,
        },
        figures: LineFigures {
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
        },
        notes: r.try_get("notes")?,
        created_at: r.try_get("createdAt")?,
        updated_at: r.try_get("updatedAt")?,
    })
}

async fn lines_of(conn: &mut SqliteConnection, run_id: &str) -> AppResult<Vec<Line>> {
    let rows =
        sqlx::query(r#"SELECT * FROM "payroll_lines" WHERE "runId" = ?1 ORDER BY "employeeCode" ASC, rowid ASC"#)
            .bind(run_id)
            .fetch_all(conn)
            .await?;
    rows.iter().map(line_from_row).collect()
}

async fn write_line(conn: &mut SqliteConnection, l: &Line) -> AppResult<()> {
    let money = line_money(l);
    let mut cols: Vec<&str> = vec![
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
        "payeOverride",
        "notes",
        "createdAt",
        "updatedAt",
    ];
    cols.extend(money.iter().map(|(k, _)| *k));
    let names: Vec<String> = cols.iter().map(|c| format!("\"{c}\"")).collect();
    let marks: Vec<String> = (1..=cols.len()).map(|i| format!("?{i}")).collect();
    let updates: Vec<String> = names
        .iter()
        .filter(|n| !matches!(n.as_str(), "\"id\"" | "\"createdAt\""))
        .map(|n| format!("{n}=excluded.{n}"))
        .collect();
    let sql = format!(
        r#"INSERT INTO "payroll_lines" ({}) VALUES ({}) ON CONFLICT ("id") DO UPDATE SET {}"#,
        names.join(","),
        marks.join(","),
        updates.join(",")
    );
    let mut q = sqlx::query(&sql)
        .bind(&l.id)
        .bind(&l.run_id)
        .bind(&l.employee_id)
        .bind(&l.salary_id)
        .bind(&l.slip_number)
        .bind(&l.employee_code)
        .bind(&l.employee_name)
        .bind(&l.job_title)
        .bind(&l.department)
        .bind(&l.employment_type)
        .bind(&l.epf_number)
        .bind(&l.national_id)
        .bind(&l.bank_name)
        .bind(&l.bank_branch)
        .bind(&l.bank_account_no)
        .bind(i64::from(l.inputs.epf_applicable))
        .bind(l.inputs.days_in_month)
        .bind(l.inputs.days_worked)
        .bind(l.inputs.paye_override.map(Money::cents))
        .bind(&l.notes)
        .bind(&l.created_at)
        .bind(&l.updated_at);
    for (_, v) in money {
        q = q.bind(v.cents());
    }
    q.execute(conn).await?;
    Ok(())
}

async fn get_line(conn: &mut SqliteConnection, run_id: &str, id: &str) -> AppResult<Line> {
    let row = sqlx::query(r#"SELECT * FROM "payroll_lines" WHERE "id" = ?1 AND "runId" = ?2"#)
        .bind(id)
        .bind(run_id)
        .fetch_optional(conn)
        .await?;
    row.as_ref().map(line_from_row).transpose()?.ok_or_else(|| AppError::NotFound("Payroll line not found".into()))
}

// ─── Remittances and the run detail ─────────────────────────────────────────

fn remittance_json(r: &SqliteRow) -> AppResult<Value> {
    let amount: i64 = r.try_get("amount")?;
    Ok(json!({
        "id": r.try_get::<String, _>("id")?,
        "runId": r.try_get::<String, _>("runId")?,
        "type": r.try_get::<String, _>("type")?,
        "amount": Money::from_cents(amount).to_db_string(),
        "date": r.try_get::<String, _>("date")?,
        "paymentAccountId": r.try_get::<String, _>("paymentAccountId")?,
        "reference": r.try_get::<Option<String>, _>("reference")?,
        "journalEntryId": r.try_get::<Option<String>, _>("journalEntryId")?,
        "createdByName": r.try_get::<Option<String>, _>("createdByName")?,
        "createdAt": r.try_get::<String, _>("createdAt")?,
    }))
}

/// What each statutory bucket owes for this run, less what has been remitted.
async fn owed(conn: &mut SqliteConnection, run: &Run) -> AppResult<Vec<(&'static str, Money, Money)>> {
    let mut out = Vec::new();
    for kind in REMITTANCE_TYPES {
        let total = match *kind {
            "EPF" => add(run.total("epfEmployeeTotal"), run.total("epfEmployerTotal"))?,
            "ETF" => run.total("etfTotal"),
            "PAYE" => run.total("payeTotal"),
            "STAMP_DUTY" => run.total("stampDutyTotal"),
            _ => add(run.total("welfareTotal"), run.total("employerWelfareTotal"))?,
        };
        let remitted: i64 = sqlx::query_scalar(
            r#"SELECT COALESCE(SUM("amount"), 0) FROM "payroll_remittances" WHERE "runId" = ?1 AND "type" = ?2"#,
        )
        .bind(&run.id)
        .bind(kind)
        .fetch_one(&mut *conn)
        .await?;
        out.push((*kind, total, Money::from_cents(remitted)));
    }
    Ok(out)
}

/// Reference `findOne`: the run with its lines (by employee code), remittances (by date) and
/// what is still owed per statutory bucket.
pub async fn detail(conn: &mut SqliteConnection, id: &str) -> AppResult<Value> {
    let run = get_run(conn, id).await?;
    let lines: Vec<Value> = lines_of(conn, id).await?.iter().map(Line::to_json).collect();
    let rows = sqlx::query(r#"SELECT * FROM "payroll_remittances" WHERE "runId" = ?1 ORDER BY "date" ASC, rowid ASC"#)
        .bind(id)
        .fetch_all(&mut *conn)
        .await?;
    let remittances: Vec<Value> = rows.iter().map(remittance_json).collect::<AppResult<_>>()?;
    let mut owed_map = Map::new();
    for (kind, total, remitted) in owed(conn, &run).await? {
        let outstanding = total.checked_sub(remitted).ok_or_else(too_large)?;
        owed_map.insert(
            kind.into(),
            json!({
                "total": total.to_json_number(),
                "remitted": remitted.to_json_number(),
                "outstanding": outstanding.to_json_number(),
            }),
        );
    }
    let mut v = run.to_json();
    if let Some(o) = v.as_object_mut() {
        o.insert("lines".into(), Value::Array(lines));
        o.insert("remittances".into(), Value::Array(remittances));
        o.insert("owed".into(), Value::Object(owed_map));
    }
    Ok(v)
}

pub async fn find_one(db: &Db, id: &str) -> AppResult<Value> {
    let mut c = db.read().acquire().await?;
    detail(&mut c, id).await
}

async fn recalc_totals(conn: &mut SqliteConnection, run_id: &str) -> AppResult<()> {
    let lines = lines_of(&mut *conn, run_id).await?;
    let sum = |col: &str| -> AppResult<Money> { lines.iter().try_fold(Money::ZERO, |a, l| add(a, l.col(col))) };
    let values: Vec<(&str, Money)> = vec![
        ("earningsTotal", sum("earningsTotal")?),
        ("noPayTotal", sum("noPayAmount")?),
        ("grossTotal", sum("grossSalary")?),
        ("epfEmployeeTotal", sum("epfEmployee")?),
        ("payeTotal", sum("paye")?),
        ("stampDutyTotal", sum("stampDuty")?),
        ("welfareTotal", sum("welfare")?),
        ("loanRecoveryTotal", sum("loanRecovery")?),
        ("salaryAdvanceTotal", sum("salaryAdvance")?),
        ("otherDeductionsTotal", add(sum("phoneBill")?, sum("otherDeductions")?)?),
        ("deductionsTotal", sum("deductionsTotal")?),
        ("netTotal", sum("netSalary")?),
        ("epfEmployerTotal", sum("epfEmployer")?),
        ("etfTotal", sum("etf")?),
        ("employerWelfareTotal", sum("employerWelfare")?),
        ("costToCompanyTotal", sum("costToCompany")?),
    ];
    let sets: Vec<String> = values.iter().enumerate().map(|(i, (k, _))| format!("\"{k}\"=?{}", i + 3)).collect();
    let sql = format!(
        r#"UPDATE "payroll_runs" SET "employeeCount"=?1,"updatedAt"=?2,{} WHERE "id"=?{}"#,
        sets.join(","),
        values.len() + 3
    );
    let mut q = sqlx::query(&sql).bind(i64::try_from(lines.len()).unwrap_or(i64::MAX)).bind(now_iso());
    for (_, v) in &values {
        q = q.bind(v.cents());
    }
    q.bind(run_id).execute(conn).await?;
    Ok(())
}

// ─── Rates and company details (settings) ───────────────────────────────────

async fn settings_map(conn: &mut SqliteConnection) -> AppResult<Map<String, Value>> {
    let rows: Vec<(String, String)> = sqlx::query_as(r#"SELECT "key","value" FROM "settings""#).fetch_all(conn).await?;
    Ok(rows.into_iter().map(|(k, v)| (k, Value::String(v))).collect())
}

pub async fn get_rates(db: &Db) -> AppResult<Value> {
    let mut c = db.read().acquire().await?;
    Ok(rates_from_settings(&settings_map(&mut c).await?).to_json())
}

/// Sent rate values as the strings the reference stores (`String(value)`, slabs as JSON).
pub async fn update_rates(db: &Db, values: Vec<(&'static str, String)>) -> AppResult<Value> {
    let mut tx = db.begin().await?;
    for (key, value) in &values {
        database::settings::upsert(&mut tx, key, value).await?;
    }
    let rates = rates_from_settings(&settings_map(&mut tx).await?).to_json();
    tx.commit().await?;
    Ok(rates)
}

pub const COMPANY_KEYS: &[&str] =
    &["companyName", "epfRegNo", "etfRegNo", "careOf", "epfAddress", "etfAddress", "telephone", "hrSignatory"];

async fn company_of(conn: &mut SqliteConnection) -> AppResult<Value> {
    let map = settings_map(conn).await?;
    let mut o = Map::new();
    for k in COMPANY_KEYS {
        let v = map.get(&format!("payroll.{k}")).and_then(Value::as_str).filter(|s| !s.is_empty());
        o.insert((*k).into(), v.map(|s| Value::String(s.into())).unwrap_or(Value::Null));
    }
    Ok(Value::Object(o))
}

pub async fn get_company(db: &Db) -> AppResult<Value> {
    let mut c = db.read().acquire().await?;
    company_of(&mut c).await
}

/// Sent fields only; null clears a field (stored as an empty string, as the reference did).
pub async fn update_company(db: &Db, values: Vec<(String, Option<String>)>) -> AppResult<Value> {
    let mut tx = db.begin().await?;
    for (k, v) in &values {
        database::settings::upsert(&mut tx, &format!("payroll.{k}"), v.as_deref().unwrap_or("")).await?;
    }
    let company = company_of(&mut tx).await?;
    tx.commit().await?;
    Ok(company)
}

// ─── Creating a run ─────────────────────────────────────────────────────────

/// The last day of a `YYYY-MM` month.
fn period_end(period: &str) -> AppResult<String> {
    let (y, m) = period.split_once('-').ok_or_else(|| AppError::bad_request("period must be YYYY-MM"))?;
    let (y, m): (i32, u32) = (y.parse().map_err(AppError::internal)?, m.parse().map_err(AppError::internal)?);
    let first_next = if m == 12 {
        chrono::NaiveDate::from_ymd_opt(y + 1, 1, 1)
    } else {
        chrono::NaiveDate::from_ymd_opt(y, m + 1, 1)
    }
    .ok_or_else(|| AppError::bad_request("period must be YYYY-MM"))?;
    Ok(first_next.pred_opt().ok_or_else(|| AppError::internal("date underflow"))?.to_string())
}

struct EmployeeRow {
    id: String,
    employee_id: String,
    name: String,
    job_title: Option<String>,
    department: Option<String>,
    join_date: Option<String>,
    status: String,
    employment_type: String,
    epf_number: Option<String>,
    national_id: Option<String>,
    resigned_date: Option<String>,
    bank_name: Option<String>,
    bank_branch: Option<String>,
    bank_account_no: Option<String>,
    staff_loan_balance: Money,
}

const EMPLOYEE_COLUMNS: &str = r#""id","employeeId","name","jobTitle","department","joinDate","status","employmentType",
"epfNumber","nationalId","resignedDate","bankName","bankBranch","bankAccountNo","staffLoanBalance""#;

fn employee_from_row(r: &SqliteRow) -> AppResult<EmployeeRow> {
    Ok(EmployeeRow {
        id: r.try_get("id")?,
        employee_id: r.try_get("employeeId")?,
        name: r.try_get("name")?,
        job_title: r.try_get("jobTitle")?,
        department: r.try_get("department")?,
        join_date: r.try_get("joinDate")?,
        status: r.try_get("status")?,
        employment_type: r.try_get("employmentType")?,
        epf_number: r.try_get("epfNumber")?,
        national_id: r.try_get("nationalId")?,
        resigned_date: r.try_get("resignedDate")?,
        bank_name: r.try_get("bankName")?,
        bank_branch: r.try_get("bankBranch")?,
        bank_account_no: r.try_get("bankAccountNo")?,
        staff_loan_balance: Money::from_cents(r.try_get("staffLoanBalance")?),
    })
}

/// Why an employee is left out of a month, or `None` if eligible.
fn ineligible(e: &EmployeeRow, start: &str, end: &str) -> Option<String> {
    if e.status == "LEFT" {
        return Some("Marked as left".into());
    }
    if let Some(j) = e.join_date.as_deref().filter(|j| *j > end) {
        return Some(format!("Joins on {j}"));
    }
    if let Some(r) = e.resigned_date.as_deref().filter(|r| *r < start) {
        return Some(format!("Resigned on {r}"));
    }
    None
}

/// Joiners and leavers mid-month get the calendar days they were employed (hundredths).
fn default_days_worked(e: &EmployeeRow, run: &Run, days_in_month: i64) -> i64 {
    let start = e.join_date.as_deref().filter(|j| *j > run.period_start.as_str()).unwrap_or(&run.period_start);
    let end = e.resigned_date.as_deref().filter(|r| *r < run.period_end.as_str()).unwrap_or(&run.period_end);
    if start == run.period_start && end == run.period_end {
        return days_in_month * 100;
    }
    let parse = |s: &str| chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").ok();
    let days = match (parse(start), parse(end)) {
        (Some(a), Some(b)) => (b - a).num_days() + 1,
        _ => 0,
    };
    days.clamp(0, days_in_month) * 100
}

fn build_line(run: &Run, e: &EmployeeRow, s: &employees::salaries::Salary, rates: &Rates) -> AppResult<Line> {
    let days_in_month = rates.line_days_in_month();
    let contract = e.employment_type == "CONTRACT";
    let now = now_iso();
    let inputs = LineInputs {
        epf_applicable: s.epf_applicable && !contract,
        days_in_month,
        days_worked: default_days_worked(e, run, days_in_month),
        basic: s.basic,
        bra2005: s.bra2005,
        bra2016: s.bra2016,
        vehicle_allowance: s.vehicle_allowance,
        other_allowance: s.other_allowance,
        welfare: s.welfare_deduction,
        loan_recovery: s.loan_installment.min(e.staff_loan_balance),
        ..LineInputs::default()
    };
    let figures = compute_line(&inputs, rates)?;
    Ok(Line {
        id: new_id(),
        run_id: run.id.clone(),
        employee_id: Some(e.id.clone()),
        salary_id: Some(s.id.clone()),
        slip_number: format!("{}/{}", run.run_number, e.employee_id),
        employee_code: e.employee_id.clone(),
        employee_name: e.name.clone(),
        job_title: e.job_title.clone(),
        department: e.department.clone(),
        employment_type: e.employment_type.clone(),
        epf_number: e.epf_number.clone(),
        national_id: e.national_id.clone(),
        bank_name: e.bank_name.clone(),
        bank_branch: e.bank_branch.clone(),
        bank_account_no: e.bank_account_no.clone(),
        inputs,
        figures,
        notes: None,
        created_at: now.clone(),
        updated_at: now,
    })
}

/// Creates the month's DRAFT and fills it with every eligible employee (not LEFT, joined by
/// the period end, not resigned before the period start, with a salary in force).
pub async fn create(db: &Db, period: &str, notes: Option<String>, by: &Actor) -> AppResult<Value> {
    let mut tx = db.begin().await?;
    let live: Option<(String, String)> = sqlx::query_as(
        r#"SELECT "runNumber","status" FROM "payroll_runs" WHERE "period" = ?1 AND "status" IN ('DRAFT','POSTED','PAID') LIMIT 1"#,
    )
    .bind(period)
    .fetch_optional(&mut *tx)
    .await?;
    if let Some((number, status)) = live {
        return Err(AppError::bad_request(format!("A payroll run for {period} already exists ({number}, {status})")));
    }
    let start = format!("{period}-01");
    let end = period_end(period)?;
    let rates = rates_from_settings(&settings_map(&mut tx).await?);
    let id = new_id();
    let number = counters::next_formatted(&mut tx, "PRUN", 4).await?;
    let now = now_iso();
    sqlx::query(
        r#"INSERT INTO "payroll_runs" ("id","runNumber","period","periodStart","periodEnd","status","notes","ratesSnapshot",
           "createdById","createdByName","createdAt","updatedAt") VALUES (?1,?2,?3,?4,?5,'DRAFT',?6,?7,?8,?9,?10,?10)"#,
    )
    .bind(&id)
    .bind(&number)
    .bind(period)
    .bind(&start)
    .bind(&end)
    .bind(&notes)
    .bind(rates.to_json().to_string())
    .bind(&by.id)
    .bind(&by.username)
    .bind(&now)
    .execute(&mut *tx)
    .await?;
    let run = get_run(&mut tx, &id).await?;

    let rows =
        sqlx::query(&format!(r#"SELECT {EMPLOYEE_COLUMNS} FROM "employees" ORDER BY "employeeId" ASC, rowid ASC"#))
            .fetch_all(&mut *tx)
            .await?;
    let mut skipped = Vec::new();
    for row in &rows {
        let e = employee_from_row(row)?;
        let skip = |reason: String| json!({"employeeId": e.id, "employeeCode": e.employee_id, "name": e.name, "reason": reason});
        if let Some(why) = ineligible(&e, &start, &end) {
            skipped.push(skip(why));
            continue;
        }
        let Some(salary) = employees::salaries::salary_as_of(&mut tx, &e.id, &end).await? else {
            skipped.push(skip("No salary set for this month".into()));
            continue;
        };
        write_line(&mut tx, &build_line(&run, &e, &salary, &rates)?).await?;
    }
    recalc_totals(&mut tx, &id).await?;
    let mut out = detail(&mut tx, &id).await?;
    tx.commit().await?;
    if let Some(o) = out.as_object_mut() {
        o.insert("skipped".into(), Value::Array(skipped));
    }
    Ok(out)
}

pub async fn add_line(db: &Db, run_id: &str, employee_id: &str) -> AppResult<Value> {
    let mut tx = db.begin().await?;
    let run = draft_only(&mut tx, run_id).await?;
    let row = sqlx::query(&format!(r#"SELECT {EMPLOYEE_COLUMNS} FROM "employees" WHERE "id" = ?1"#))
        .bind(employee_id)
        .fetch_optional(&mut *tx)
        .await?;
    let e = employee_from_row(row.as_ref().ok_or_else(|| AppError::NotFound("Employee not found".into()))?)?;
    let dup: Option<String> =
        sqlx::query_scalar(r#"SELECT "id" FROM "payroll_lines" WHERE "runId" = ?1 AND "employeeId" = ?2"#)
            .bind(run_id)
            .bind(&e.id)
            .fetch_optional(&mut *tx)
            .await?;
    if dup.is_some() {
        return Err(AppError::bad_request(format!("{} is already in this run", e.name)));
    }
    let salary = employees::salaries::salary_as_of(&mut tx, &e.id, &run.period_end)
        .await?
        .ok_or_else(|| AppError::bad_request(format!("{} has no salary in force for {}", e.name, run.period)))?;
    let rates = Rates::from_json(&run.rates_snapshot);
    write_line(&mut tx, &build_line(&run, &e, &salary, &rates)?).await?;
    recalc_totals(&mut tx, run_id).await?;
    let out = detail(&mut tx, run_id).await?;
    tx.commit().await?;
    Ok(out)
}

/// Sent line fields after validation. `paye_override: Some(None)` clears the override.
#[derive(Debug, Default, Clone)]
pub struct LinePatch {
    pub days_worked: Option<i64>,
    pub project_bonus: Option<Money>,
    pub arrears: Option<Money>,
    pub medical: Option<Money>,
    pub other_earnings: Option<Money>,
    pub no_pay_amount: Option<Money>,
    pub welfare: Option<Money>,
    pub loan_recovery: Option<Money>,
    pub salary_advance: Option<Money>,
    pub phone_bill: Option<Money>,
    pub other_deductions: Option<Money>,
    pub paye_override: Option<Option<Money>>,
    pub notes: Option<Option<String>>,
}

/// Edits a draft line, recomputes it and the run totals. Returns the line as the reference
/// did: sent and recomputed fields as numbers, the rest as stored strings.
pub async fn update_line(db: &Db, run_id: &str, line_id: &str, p: LinePatch) -> AppResult<Value> {
    let mut tx = db.begin().await?;
    let run = draft_only(&mut tx, run_id).await?;
    let mut line = get_line(&mut tx, run_id, line_id).await?;
    if let Some(d) = p.days_worked {
        if d > line.inputs.days_in_month * 100 {
            return Err(AppError::bad_request(format!("Days worked cannot exceed {}", line.inputs.days_in_month)));
        }
    }
    if let (Some(rec), Some(emp)) = (p.loan_recovery, &line.employee_id) {
        let balance: Option<i64> = sqlx::query_scalar(r#"SELECT "staffLoanBalance" FROM "employees" WHERE "id" = ?1"#)
            .bind(emp)
            .fetch_optional(&mut *tx)
            .await?;
        if let Some(b) = balance.map(Money::from_cents) {
            if rec > b {
                return Err(AppError::bad_request(format!("Loan recovery exceeds the outstanding balance of {b}")));
            }
        }
    }
    let mut sent: Vec<(&str, Value)> = Vec::new();
    let i = &mut line.inputs;
    if let Some(d) = p.days_worked {
        i.days_worked = d;
        sent.push(("daysWorked", Money::from_cents(d).to_json_number()));
    }
    for (key, slot, v) in [
        ("projectBonus", &mut i.project_bonus, p.project_bonus),
        ("arrears", &mut i.arrears, p.arrears),
        ("medical", &mut i.medical, p.medical),
        ("otherEarnings", &mut i.other_earnings, p.other_earnings),
        ("noPayAmount", &mut i.no_pay_amount, p.no_pay_amount),
        ("welfare", &mut i.welfare, p.welfare),
        ("loanRecovery", &mut i.loan_recovery, p.loan_recovery),
        ("salaryAdvance", &mut i.salary_advance, p.salary_advance),
        ("phoneBill", &mut i.phone_bill, p.phone_bill),
        ("otherDeductions", &mut i.other_deductions, p.other_deductions),
    ] {
        if let Some(v) = v {
            *slot = v;
            sent.push((key, v.to_json_number()));
        }
    }
    if let Some(o) = p.paye_override {
        i.paye_override = o;
        sent.push(("payeOverride", o.map(Money::to_json_number).unwrap_or(Value::Null)));
    }
    if let Some(n) = p.notes {
        sent.push(("notes", opt(&n)));
        line.notes = n;
    }
    line.figures = compute_line(&line.inputs, &Rates::from_json(&run.rates_snapshot))?;
    line.updated_at = now_iso();
    write_line(&mut tx, &line).await?;
    recalc_totals(&mut tx, run_id).await?;
    tx.commit().await?;

    let mut v = line.to_json();
    if let Some(o) = v.as_object_mut() {
        for (k, val) in sent {
            o.insert(k.into(), val);
        }
        for k in FIGURE_KEYS {
            o.insert((*k).into(), line.col(k).to_json_number());
        }
    }
    Ok(v)
}

pub async fn remove_line(db: &Db, run_id: &str, line_id: &str) -> AppResult<Value> {
    let mut tx = db.begin().await?;
    draft_only(&mut tx, run_id).await?;
    get_line(&mut tx, run_id, line_id).await?;
    sqlx::query(r#"DELETE FROM "payroll_lines" WHERE "id" = ?1"#).bind(line_id).execute(&mut *tx).await?;
    recalc_totals(&mut tx, run_id).await?;
    let out = detail(&mut tx, run_id).await?;
    tx.commit().await?;
    Ok(out)
}

/// Drafts only; lines cascade.
pub async fn remove(db: &Db, run_id: &str) -> AppResult<()> {
    let mut tx = db.begin().await?;
    draft_only(&mut tx, run_id).await?;
    sqlx::query(r#"DELETE FROM "payroll_runs" WHERE "id" = ?1"#).bind(run_id).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(())
}

// ─── Lifecycle: post / pay / remit / void ───────────────────────────────────

fn debit(account: &str, amount: Money, description: &str) -> Vec<NewLine> {
    if amount > Money::ZERO {
        vec![NewLine {
            account_id: account.into(),
            debit: amount,
            credit: Money::ZERO,
            description: Some(description.into()),
        }]
    } else {
        Vec::new()
    }
}

fn credit(account: &str, amount: Money, description: &str) -> Vec<NewLine> {
    if amount > Money::ZERO {
        vec![NewLine {
            account_id: account.into(),
            debit: Money::ZERO,
            credit: amount,
            description: Some(description.into()),
        }]
    } else {
        Vec::new()
    }
}

async fn account_id(tx: &mut Tx, code: &str) -> AppResult<String> {
    Ok(accounts::find_by_code(tx, code).await?.id)
}

/// Books the month: salaries and employer contributions as expense, net pay and every
/// statutory amount as a payable. Loan balances come down here.
pub async fn post(db: &Db, run_id: &str) -> AppResult<Value> {
    let mut tx = db.begin().await?;
    draft_only(&mut tx, run_id).await?;
    let lines = lines_of(&mut tx, run_id).await?;
    if lines.is_empty() {
        return Err(AppError::bad_request("Add at least one employee before posting"));
    }
    recalc_totals(&mut tx, run_id).await?;
    let run = get_run(&mut tx, run_id).await?;

    let mut ids = Vec::new();
    for code in [
        acc::STAFF_LOANS,
        acc::SALARIES_PAYABLE,
        acc::EPF_PAYABLE,
        acc::ETF_PAYABLE,
        acc::PAYE_PAYABLE,
        acc::STAMP_DUTY_PAYABLE,
        acc::WELFARE_FUND,
        acc::STAFF_DEDUCTIONS_PAYABLE,
        acc::SALARIES_EXPENSE,
        acc::EMPLOYER_CONTRIBUTIONS,
        acc::WELFARE_EXPENSE,
    ] {
        ids.push(account_id(&mut tx, code).await?);
    }
    let [loans, payable, epf, etf_acc, paye, stamp, welfare, deductions, expense, contributions, welfare_expense] =
        <[String; 11]>::try_from(ids).map_err(|_| AppError::internal("account list"))?;
    let sum = |col: &str| -> AppResult<Money> { lines.iter().try_fold(Money::ZERO, |a, l| add(a, l.col(col))) };

    let mut entry_lines = Vec::new();
    entry_lines.extend(debit(&expense, sum("grossSalary")?, "Gross salaries"));
    entry_lines.extend(debit(&contributions, add(sum("epfEmployer")?, sum("etf")?)?, "Employer EPF 12% + ETF 3%"));
    entry_lines.extend(debit(&welfare_expense, sum("employerWelfare")?, "Employer welfare contribution"));
    entry_lines.extend(credit(&payable, sum("netSalary")?, "Net salaries payable"));
    entry_lines.extend(credit(&epf, add(sum("epfEmployee")?, sum("epfEmployer")?)?, "EPF 8% + 12%"));
    entry_lines.extend(credit(&etf_acc, sum("etf")?, "ETF 3%"));
    entry_lines.extend(credit(&paye, sum("paye")?, "PAYE"));
    entry_lines.extend(credit(&stamp, sum("stampDuty")?, "Stamp duty"));
    entry_lines.extend(credit(&welfare, add(sum("welfare")?, sum("employerWelfare")?)?, "Staff welfare fund"));
    entry_lines.extend(credit(
        &loans,
        add(sum("loanRecovery")?, sum("salaryAdvance")?)?,
        "Staff loan / advance recovery",
    ));
    entry_lines.extend(credit(&deductions, add(sum("phoneBill")?, sum("otherDeductions")?)?, "Other staff deductions"));

    let entry = entries::post(
        &mut tx,
        NewEntry {
            date: run.period_end.clone(),
            description: format!("Payroll {} ({})", run.period, run.run_number),
            reference: Some(run.run_number.clone()),
            source_type: Some("PAYROLL".into()),
            source_id: Some(run.id.clone()),
            lines: entry_lines,
        },
    )
    .await?;

    // Knock the recovered instalments off each employee's loan balance.
    for l in &lines {
        let recovered = add(l.inputs.loan_recovery, l.inputs.salary_advance)?;
        if let (Some(emp), true) = (&l.employee_id, recovered > Money::ZERO) {
            sqlx::query(r#"UPDATE "employees" SET "staffLoanBalance" = "staffLoanBalance" - ?1, "updatedAt" = ?2 WHERE "id" = ?3"#)
                .bind(recovered.cents())
                .bind(now_iso())
                .bind(emp)
                .execute(&mut *tx)
                .await?;
        }
    }
    let now = now_iso();
    sqlx::query(
        r#"UPDATE "payroll_runs" SET "status"='POSTED',"journalEntryId"=?1,"postedAt"=?2,"updatedAt"=?2 WHERE "id"=?3"#,
    )
    .bind(&entry.id)
    .bind(&now)
    .bind(run_id)
    .execute(&mut *tx)
    .await?;
    let out = detail(&mut tx, run_id).await?;
    tx.commit().await?;
    Ok(out)
}

/// Clears Salaries Payable against the chosen bank / cash account.
pub async fn pay(db: &Db, run_id: &str, payment_account_id: &str, pay_date: &str) -> AppResult<Value> {
    let mut tx = db.begin().await?;
    let run = get_run(&mut tx, run_id).await?;
    if run.status != "POSTED" {
        return Err(AppError::bad_request(format!("Only a POSTED run can be paid — this one is {}", run.status)));
    }
    let bank = accounts::find_one(&mut tx, payment_account_id).await?;
    let payable = account_id(&mut tx, acc::SALARIES_PAYABLE).await?;
    let net = run.total("netTotal");
    if net <= Money::ZERO {
        return Err(AppError::bad_request("Nothing to pay — net total is zero"));
    }
    let entry = entries::post(
        &mut tx,
        NewEntry {
            date: pay_date.into(),
            description: format!("Salaries paid {} ({})", run.period, run.run_number),
            reference: Some(run.run_number.clone()),
            source_type: Some("PAYROLL_PAYMENT".into()),
            source_id: Some(run.id.clone()),
            lines: vec![
                NewLine {
                    account_id: payable,
                    debit: net,
                    credit: Money::ZERO,
                    description: Some("Salaries payable cleared".into()),
                },
                NewLine {
                    account_id: bank.id.clone(),
                    debit: Money::ZERO,
                    credit: net,
                    description: Some(format!("Paid from {}", bank.name)),
                },
            ],
        },
    )
    .await?;
    let now = now_iso();
    sqlx::query(
        r#"UPDATE "payroll_runs" SET "status"='PAID',"paymentJournalEntryId"=?1,"paymentAccountId"=?2,"payDate"=?3,
           "paidAt"=?4,"updatedAt"=?4 WHERE "id"=?5"#,
    )
    .bind(&entry.id)
    .bind(&bank.id)
    .bind(pay_date)
    .bind(&now)
    .bind(run_id)
    .execute(&mut *tx)
    .await?;
    let out = detail(&mut tx, run_id).await?;
    tx.commit().await?;
    Ok(out)
}

pub struct Remit {
    pub kind: String,
    pub payment_account_id: String,
    pub date: String,
    pub amount: Option<Money>,
    pub reference: Option<String>,
}

/// Pays EPF / ETF / PAYE / stamp duty / welfare over to the authority.
pub async fn remit(db: &Db, run_id: &str, r: Remit, by: &Actor) -> AppResult<Value> {
    let mut tx = db.begin().await?;
    let run = get_run(&mut tx, run_id).await?;
    if run.status != "POSTED" && run.status != "PAID" {
        return Err(AppError::bad_request("Post the run before recording remittances"));
    }
    let (_, total, remitted) = owed(&mut tx, &run)
        .await?
        .into_iter()
        .find(|(k, _, _)| *k == r.kind)
        .ok_or_else(|| AppError::internal("unknown remittance type"))?;
    let outstanding = total.checked_sub(remitted).ok_or_else(too_large)?;
    let amount = r.amount.unwrap_or(outstanding);
    if amount <= Money::ZERO {
        return Err(AppError::bad_request(format!("Nothing outstanding for {} on this run", r.kind)));
    }
    if amount > outstanding {
        return Err(AppError::bad_request(format!("{} remittance exceeds the outstanding {outstanding}", r.kind)));
    }
    let bank = accounts::find_one(&mut tx, &r.payment_account_id).await?;
    let payable = accounts::find_by_code(&mut tx, remit_payable(&r.kind)).await?;
    let entry = entries::post(
        &mut tx,
        NewEntry {
            date: r.date.clone(),
            description: format!("{} remitted for {} ({})", r.kind.replacen('_', " ", 1), run.period, run.run_number),
            reference: Some(r.reference.clone().unwrap_or_else(|| run.run_number.clone())),
            source_type: Some("PAYROLL_REMITTANCE".into()),
            source_id: Some(run.id.clone()),
            lines: vec![
                NewLine {
                    account_id: payable.id.clone(),
                    debit: amount,
                    credit: Money::ZERO,
                    description: Some(format!("{} cleared", payable.name)),
                },
                NewLine {
                    account_id: bank.id.clone(),
                    debit: Money::ZERO,
                    credit: amount,
                    description: Some(format!("Paid from {}", bank.name)),
                },
            ],
        },
    )
    .await?;
    sqlx::query(
        r#"INSERT INTO "payroll_remittances" ("id","runId","type","amount","date","paymentAccountId","reference",
           "journalEntryId","createdByName","createdAt") VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)"#,
    )
    .bind(new_id())
    .bind(run_id)
    .bind(&r.kind)
    .bind(amount.cents())
    .bind(&r.date)
    .bind(&bank.id)
    .bind(&r.reference)
    .bind(&entry.id)
    .bind(&by.username)
    .bind(now_iso())
    .execute(&mut *tx)
    .await?;
    let out = detail(&mut tx, run_id).await?;
    tx.commit().await?;
    Ok(out)
}

/// The client generates and uploads the slips; this records that it happened.
pub async fn slips_filed(db: &Db, run_id: &str) -> AppResult<Value> {
    let mut tx = db.begin().await?;
    let run = get_run(&mut tx, run_id).await?;
    if run.status != "POSTED" && run.status != "PAID" {
        return Err(AppError::bad_request("Slips can only be filed for a posted or paid run"));
    }
    let now = now_iso();
    sqlx::query(r#"UPDATE "payroll_runs" SET "slipsFiledAt"=?1,"updatedAt"=?1 WHERE "id"=?2"#)
        .bind(&now)
        .bind(run_id)
        .execute(&mut *tx)
        .await?;
    let out = detail(&mut tx, run_id).await?;
    tx.commit().await?;
    Ok(out)
}

/// Reverses every ledger entry the run produced (remittances, payment, posting), restores the
/// loan balances and frees the month. Reversals are dated today (local date, D5).
pub async fn void(db: &Db, run_id: &str) -> AppResult<Value> {
    let mut tx = db.begin().await?;
    let run = get_run(&mut tx, run_id).await?;
    if run.status == "VOID" {
        return Err(AppError::bad_request("Run is already void"));
    }
    if run.status == "DRAFT" {
        return Err(AppError::bad_request("A draft can simply be deleted"));
    }
    let today = today_local().to_string();
    let remitted: Vec<Option<String>> =
        sqlx::query_scalar(r#"SELECT "journalEntryId" FROM "payroll_remittances" WHERE "runId" = ?1 ORDER BY rowid"#)
            .bind(run_id)
            .fetch_all(&mut *tx)
            .await?;
    for je in remitted.into_iter().flatten() {
        entries::reverse(&mut tx, &je, &today).await?;
    }
    if let Some(je) = &run.payment_journal_entry_id {
        entries::reverse(&mut tx, je, &today).await?;
    }
    if let Some(je) = &run.journal_entry_id {
        entries::reverse(&mut tx, je, &today).await?;
    }
    for l in lines_of(&mut tx, run_id).await? {
        let recovered = add(l.inputs.loan_recovery, l.inputs.salary_advance)?;
        if let (Some(emp), true) = (&l.employee_id, recovered > Money::ZERO) {
            sqlx::query(r#"UPDATE "employees" SET "staffLoanBalance" = "staffLoanBalance" + ?1, "updatedAt" = ?2 WHERE "id" = ?3"#)
                .bind(recovered.cents())
                .bind(now_iso())
                .bind(emp)
                .execute(&mut *tx)
                .await?;
        }
    }
    sqlx::query(r#"UPDATE "payroll_runs" SET "status"='VOID',"updatedAt"=?1 WHERE "id"=?2"#)
        .bind(now_iso())
        .bind(run_id)
        .execute(&mut *tx)
        .await?;
    let out = detail(&mut tx, run_id).await?;
    tx.commit().await?;
    Ok(out)
}
