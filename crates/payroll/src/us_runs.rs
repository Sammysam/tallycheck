//! US payroll pay runs (D43, ported from the web edition's waves U3 and U5b): draft to posted,
//! paid, remitted and void. The arithmetic is [`crate::us`]; the employee's pay and Form W-4 come from
//! [`crate::us_setup`].
//!
//! Ledger, like the Sri Lankan runs (one balanced entry per step, in the same transaction). The
//! codes are the defaults; each role's account is chosen in the US payroll settings ([`ROLES`]):
//! - post (dated the period end): Dr 6100 Salaries Expense (gross), Dr 6130 Payroll Tax Expense
//!   (employer social security + Medicare + FUTA + SUTA); Cr 2200 Salaries Payable (net pay),
//!   Cr 2300 federal income tax withheld, Cr 2310 social security (both shares), Cr 2320
//!   Medicare (both shares + Additional Medicare), Cr 2330 FUTA, Cr 2340 SUTA, Cr 2350 401(k),
//!   Cr 2360 section 125, Cr 2370 state income tax withheld.
//! - pay: Dr 2200, Cr the bank / cash account (net pay).
//! - remit: Dr the payable, Cr the bank. A Form 941 deposit clears 2300 + 2310 + 2320 at once.
//! - void: reverses every entry (deposits, payment, posting), dated today.
//!
//! Year-to-date wages for the wage-base limits are the employee's lines on other POSTED / PAID
//! runs paid in the same calendar year; they are worked out again when the run is posted.

use crate::us::{self, FilingStatus, UsFigures, UsInputs, YearToDate, W4};
use crate::us_setup::{load_settings, load_tax, periods_per_year, UsSettings, UsTax};
use crate::us_state::{self, load_rule, StateEmployee, StateRule};
use app_core::ids::new_id;
use app_core::money::Money;
use app_core::time::today_local;
use app_core::{AppError, AppResult};
use database::{counters, Db, Tx};
use employees::Actor;
use ledger::accounts::{self, AccountType};
use ledger::entries::{self, NewEntry, NewLine};
use serde_json::{json, Value};
use sqlx::sqlite::SqliteRow;
use sqlx::{Row, SqliteConnection};

/// What a remittance pays. `FORM_941` is a deposit of the first three together.
pub const REMIT_TYPES: &[&str] = &[
    "FORM_941",
    "FEDERAL_INCOME_TAX",
    "SOCIAL_SECURITY",
    "MEDICARE",
    "FUTA",
    "SUTA",
    "RETIREMENT_401K",
    "SECTION_125",
    "STATE_INCOME_TAX",
];

/// What US payroll posts to. Each role's account is chosen in the US payroll settings
/// (`payroll.us.account.<key>` holds the account id). A role with no choice yet takes the account
/// with its default code, added if the code is free — but only when that code is not already
/// some other account (e.g. a company's own 2300 Sales Tax Payable), which must never quietly
/// receive payroll money (D43).
pub struct Role {
    pub key: &'static str,
    pub label: &'static str,
    pub code: &'static str,
    pub name: &'static str,
    pub kind: AccountType,
    pub subtype: &'static str,
}

const fn role(
    key: &'static str,
    label: &'static str,
    code: &'static str,
    name: &'static str,
    kind: AccountType,
) -> Role {
    let subtype = match kind {
        AccountType::Expense => "operating_expense",
        _ => "current_liability",
    };
    Role { key, label, code, name, kind, subtype }
}

pub const ROLES: &[Role] = &[
    role("salariesExpense", "Gross wages", "6100", "Salaries Expense", AccountType::Expense),
    role("payrollTaxExpense", "Employer payroll taxes", "6130", "Payroll Tax Expense", AccountType::Expense),
    role("salariesPayable", "Net pay owed", "2200", "Salaries Payable", AccountType::Liability),
    role(
        "federalIncomeTax",
        "Federal income tax withheld",
        "2300",
        "Federal Income Tax Withheld",
        AccountType::Liability,
    ),
    role("socialSecurity", "Social security", "2310", "Social Security Tax Payable", AccountType::Liability),
    role("medicare", "Medicare", "2320", "Medicare Tax Payable", AccountType::Liability),
    role("futa", "FUTA", "2330", "FUTA Tax Payable", AccountType::Liability),
    role("suta", "State unemployment", "2340", "State Unemployment Tax Payable", AccountType::Liability),
    role("retirement401k", "401(k) contributions", "2350", "401(k) Contributions Payable", AccountType::Liability),
    role("section125", "Section 125 deductions", "2360", "Section 125 Benefits Payable", AccountType::Liability),
    role("stateIncomeTax", "State income tax withheld", "2370", "State Income Tax Withheld", AccountType::Liability),
];

pub fn setting_key(role: &str) -> String {
    format!("payroll.us.account.{role}")
}

/// Why `account` can't take `r`'s postings, if it can't.
pub fn unfit(r: &Role, account: &accounts::Account) -> Option<String> {
    if !account.is_active {
        return Some(format!("{} {} is switched off", account.code, account.name));
    }
    if account.account_type != r.kind {
        let want = if r.kind == AccountType::Expense { "an expense" } else { "a liability" };
        return Some(format!("{} must go to {want} account; {} {} is not one", r.label, account.code, account.name));
    }
    if account.currency.is_some() {
        return Some(format!("{} {} is kept in a foreign currency", account.code, account.name));
    }
    None
}

/// The account for each role (role key, account id), choosing and saving the defaults on first
/// use. Refuses when a default code belongs to some other account.
pub async fn ensure_accounts(tx: &mut Tx) -> AppResult<Vec<(&'static str, String)>> {
    let mut out = Vec::new();
    for r in ROLES {
        let key = setting_key(r.key);
        let chosen: Option<String> = sqlx::query_scalar(r#"SELECT "value" FROM "settings" WHERE "key" = ?1"#)
            .bind(&key)
            .fetch_optional(&mut **tx)
            .await?
            .filter(|v: &String| !v.trim().is_empty());
        if let Some(id) = chosen {
            let a = accounts::get(tx, &id).await?.ok_or_else(|| {
                AppError::bad_request(format!(
                    "The account chosen for {} no longer exists — choose another under US payroll settings",
                    r.label
                ))
            })?;
            if let Some(why) = unfit(r, &a) {
                return Err(AppError::bad_request(format!("{why} — choose another under US payroll settings")));
            }
            out.push((r.key, a.id));
            continue;
        }
        let existing = match accounts::find_by_code(tx, r.code).await {
            Ok(a) => Some(a),
            Err(AppError::NotFound(_)) => None,
            Err(e) => return Err(e),
        };
        let id = match existing {
            Some(a) if a.name.trim().eq_ignore_ascii_case(r.name) && unfit(r, &a).is_none() => a.id,
            Some(a) => {
                return Err(AppError::bad_request(format!(
                    "Account {} is already \"{}\". Choose the account for {} under US payroll settings",
                    a.code, a.name, r.label
                )))
            }
            None => {
                accounts::insert_chart(tx, &[(r.code, r.name, r.kind, r.subtype)]).await?;
                accounts::find_by_code(tx, r.code).await?.id
            }
        };
        database::settings::upsert(tx, &key, &id).await?;
        out.push((r.key, id));
    }
    Ok(out)
}

fn account_of(snapshot: &Value, role: &str) -> AppResult<String> {
    snapshot[role].as_str().map(str::to_string).ok_or_else(|| AppError::internal(format!("run has no {role} account")))
}

fn too_large() -> AppError {
    AppError::bad_request("Amount is too large")
}

fn add(a: Money, b: Money) -> AppResult<Money> {
    a.checked_add(b).ok_or_else(too_large)
}

fn sum(items: &[Money]) -> AppResult<Money> {
    items.iter().try_fold(Money::ZERO, |a, m| add(a, *m))
}

fn half_up(n: i128, d: i128) -> AppResult<Money> {
    i64::try_from((2 * n + d) / (2 * d)).map(Money::from_cents).map_err(|_| too_large())
}

/// Standard hours for one period: 2,080 a year (40 × 52), in hundredths.
pub fn standard_hours(periods: i64) -> Money {
    Money::from_cents(((2 * 208_000 + periods) / (2 * periods)).max(0))
}

// ─── Run ────────────────────────────────────────────────────────────────────

/// Run totals: (column, line column summed).
const TOTALS: &[(&str, &str)] = &[
    ("grossTotal", "gross"),
    ("pretax401kTotal", "pretax401k"),
    ("pretaxSection125Total", "pretaxSection125"),
    ("federalIncomeTaxTotal", "federalIncomeTax"),
    ("socialSecurityEmployeeTotal", "socialSecurityEmployee"),
    ("socialSecurityEmployerTotal", "socialSecurityEmployer"),
    ("medicareEmployeeTotal", "medicareEmployee"),
    ("additionalMedicareTotal", "additionalMedicare"),
    ("medicareEmployerTotal", "medicareEmployer"),
    ("futaTotal", "futa"),
    ("sutaTotal", "suta"),
    ("netPayTotal", "netPay"),
    ("employerTaxesTotal", "employerTaxes"),
    ("stateIncomeTaxTotal", "stateIncomeTax"),
];

#[derive(Debug, Clone)]
struct Run {
    id: String,
    run_number: String,
    period_start: String,
    period_end: String,
    pay_date: String,
    pay_frequency: String,
    status: String,
    notes: Option<String>,
    snapshot: Value,
    /// The accounts the posting used (role key -> account id); empty until posted.
    accounts: Value,
    employee_count: i64,
    totals: Vec<(&'static str, Money)>,
    journal_entry_id: Option<String>,
    payment_journal_entry_id: Option<String>,
    payment_account_id: Option<String>,
    paid_date: Option<String>,
    posted_at: Option<String>,
    paid_at: Option<String>,
    created_by_name: Option<String>,
    created_at: String,
    updated_at: String,
}

impl Run {
    fn total(&self, key: &str) -> Money {
        self.totals.iter().find(|(k, _)| *k == key).map(|(_, m)| *m).unwrap_or(Money::ZERO)
    }

    fn year(&self) -> AppResult<i32> {
        self.pay_date.get(..4).and_then(|y| y.parse().ok()).ok_or_else(|| AppError::internal("pay date"))
    }

    fn to_json(&self) -> Value {
        let mut o = json!({
            "id": self.id,
            "runNumber": self.run_number,
            "periodStart": self.period_start,
            "periodEnd": self.period_end,
            "payDate": self.pay_date,
            "payFrequency": self.pay_frequency,
            "status": self.status,
            "notes": self.notes,
            "settingsSnapshot": self.snapshot,
            "accounts": self.accounts,
            "employeeCount": self.employee_count,
            "journalEntryId": self.journal_entry_id,
            "paymentJournalEntryId": self.payment_journal_entry_id,
            "paymentAccountId": self.payment_account_id,
            "paidDate": self.paid_date,
            "postedAt": self.posted_at,
            "paidAt": self.paid_at,
            "createdByName": self.created_by_name,
            "createdAt": self.created_at,
            "updatedAt": self.updated_at,
        });
        if let Some(map) = o.as_object_mut() {
            for (k, m) in &self.totals {
                map.insert((*k).into(), m.to_json_number());
            }
        }
        o
    }
}

fn run_from_row(r: &SqliteRow) -> AppResult<Run> {
    let mut totals = Vec::new();
    for (k, _) in TOTALS {
        totals.push((*k, Money::from_cents(r.try_get::<i64, _>(*k)?)));
    }
    Ok(Run {
        id: r.try_get("id")?,
        run_number: r.try_get("runNumber")?,
        period_start: r.try_get::<String, _>("periodStart")?,
        period_end: r.try_get::<String, _>("periodEnd")?,
        pay_date: r.try_get::<String, _>("payDate")?,
        pay_frequency: r.try_get("payFrequency")?,
        status: r.try_get("status")?,
        notes: r.try_get("notes")?,
        snapshot: crate::us_setup::json_col(r, "settingsSnapshot")?,
        accounts: crate::us_setup::json_col(r, "accountsSnapshot")?,
        employee_count: r.try_get::<i64, _>("employeeCount")?,
        totals,
        journal_entry_id: r.try_get("journalEntryId")?,
        payment_journal_entry_id: r.try_get("paymentJournalEntryId")?,
        payment_account_id: r.try_get("paymentAccountId")?,
        paid_date: r.try_get::<Option<String>, _>("paidDate")?,
        posted_at: r.try_get::<Option<String>, _>("postedAt")?,
        paid_at: r.try_get::<Option<String>, _>("paidAt")?,
        created_by_name: r.try_get("createdByName")?,
        created_at: r.try_get::<String, _>("createdAt")?,
        updated_at: r.try_get::<String, _>("updatedAt")?,
    })
}

async fn get_run(conn: &mut SqliteConnection, id: &str) -> AppResult<Run> {
    let row = sqlx::query(r#"SELECT * FROM "us_payroll_runs" WHERE "id" = ?1"#)
        .bind(id)
        .fetch_optional(&mut *conn)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("US payroll run {id} not found")))?;
    run_from_row(&row)
}

async fn draft_only(conn: &mut SqliteConnection, id: &str) -> AppResult<Run> {
    let run = get_run(conn, id).await?;
    if run.status != "DRAFT" {
        return Err(AppError::bad_request(format!("Only a DRAFT run can be changed — this one is {}", run.status)));
    }
    Ok(run)
}

/// The SUTA / FUTA settings a run keeps for itself.
fn snapshot_of(s: &UsSettings) -> Value {
    json!({
        "state": s.state,
        "sutaRate": s.suta_rate,
        "sutaWageBase": s.suta_wage_base.map(Money::cents),
        "futaCreditReduction": s.futa_credit_reduction,
    })
}

struct RunTaxSettings {
    suta_rate: i64,
    suta_wage_base: Option<Money>,
    futa_credit_reduction: i64,
}

fn run_settings(run: &Run) -> RunTaxSettings {
    let s = &run.snapshot;
    RunTaxSettings {
        suta_rate: s["sutaRate"].as_i64().unwrap_or(0),
        suta_wage_base: s["sutaWageBase"].as_i64().map(Money::from_cents),
        futa_credit_reduction: s["futaCreditReduction"].as_i64().unwrap_or(0),
    }
}

pub async fn find_all(db: &Db) -> AppResult<Vec<Value>> {
    let rows = sqlx::query(r#"SELECT * FROM "us_payroll_runs" ORDER BY "payDate" DESC, "createdAt" DESC, "id""#)
        .fetch_all(db.read())
        .await?;
    rows.iter().map(|r| run_from_row(r).map(|run| run.to_json())).collect()
}

// ─── Lines ──────────────────────────────────────────────────────────────────

/// A line's figure columns, in table order.
const FIGURES: &[&str] = &[
    "incomeTaxWages",
    "federalIncomeTax",
    "socialSecurityWages",
    "socialSecurityEmployee",
    "socialSecurityEmployer",
    "medicareWages",
    "medicareEmployee",
    "additionalMedicare",
    "medicareEmployer",
    "futaWages",
    "futa",
    "sutaWages",
    "suta",
    "employeeTaxes",
    "netPay",
    "employerTaxes",
    "costToEmployer",
];

fn figure_values(f: &UsFigures) -> [Money; 17] {
    [
        f.income_tax_wages,
        f.federal_income_tax,
        f.social_security_wages,
        f.social_security_employee,
        f.social_security_employer,
        f.medicare_wages,
        f.medicare_employee,
        f.additional_medicare,
        f.medicare_employer,
        f.futa_wages,
        f.futa,
        f.suta_wages,
        f.suta,
        f.employee_taxes,
        f.net_pay,
        f.employer_taxes,
        f.cost_to_employer,
    ]
}

#[derive(Debug, Clone)]
struct Line {
    id: String,
    run_id: String,
    employee_id: Option<String>,
    employee_code: String,
    employee_name: String,
    work_state: String,
    pay_type: String,
    annual_salary: Money,
    hourly_rate: Money,
    hours: Money,
    overtime_hours: Money,
    regular_pay: Money,
    overtime_pay: Money,
    bonus: Money,
    other_earnings: Money,
    gross: Money,
    pretax_401k: Money,
    pretax_section125: Money,
    w4: Value,
    /// The state rule used (None: the company has no rule for the work state yet).
    state_rule: Option<Value>,
    state_cert: Value,
    state_wages: Money,
    state_tax: Money,
    ytd: YearToDate,
    figures: UsFigures,
    notes: Option<String>,
}

/// The Form W-4 part of a [`UsTax`], as stored on a line.
fn w4_snapshot(t: &UsTax) -> Value {
    json!({
        "w4Version": t.w4_version,
        "filingStatus": t.filing_status,
        "multipleJobs": t.multiple_jobs,
        "dependentsCredit": t.dependents_credit.cents(),
        "otherIncome": t.other_income.cents(),
        "deductions": t.deductions.cents(),
        "extraWithholding": t.extra_withholding.cents(),
        "allowances": t.allowances,
        "exemptFromWithholding": t.exempt_from_withholding,
    })
}

fn w4_from_snapshot(v: &Value) -> (W4, bool) {
    let cents = |k: &str| Money::from_cents(v[k].as_i64().unwrap_or(0));
    let status = v["filingStatus"].as_str().unwrap_or("SINGLE");
    let w4 = if v["w4Version"] == "LEGACY" {
        W4::Before2020 {
            married: status == "MARRIED_JOINTLY",
            allowances: v["allowances"].as_i64().unwrap_or(0),
            extra: cents("extraWithholding"),
        }
    } else {
        W4::Since2020 {
            status: match status {
                "MARRIED_JOINTLY" => FilingStatus::MarriedJointly,
                "HEAD_OF_HOUSEHOLD" => FilingStatus::HeadOfHousehold,
                _ => FilingStatus::Single,
            },
            step2_checkbox: v["multipleJobs"].as_bool().unwrap_or(false),
            credits: cents("dependentsCredit"),
            other_income: cents("otherIncome"),
            deductions: cents("deductions"),
            extra: cents("extraWithholding"),
        }
    };
    (w4, v["exemptFromWithholding"].as_bool().unwrap_or(false))
}

fn line_from_row(r: &SqliteRow) -> AppResult<Line> {
    let m = |k: &str| -> AppResult<Money> { Ok(Money::from_cents(r.try_get::<i64, _>(k)?)) };
    let mut f = [Money::ZERO; 17];
    for (slot, k) in f.iter_mut().zip(FIGURES) {
        *slot = m(k)?;
    }
    let [income_tax_wages, federal_income_tax, social_security_wages, social_security_employee, social_security_employer, medicare_wages, medicare_employee, additional_medicare, medicare_employer, futa_wages, futa, suta_wages, suta, employee_taxes, net_pay, employer_taxes, cost_to_employer] =
        f;
    Ok(Line {
        id: r.try_get("id")?,
        run_id: r.try_get("runId")?,
        employee_id: r.try_get("employeeId")?,
        employee_code: r.try_get("employeeCode")?,
        employee_name: r.try_get("employeeName")?,
        work_state: r.try_get("workState")?,
        pay_type: r.try_get("payType")?,
        annual_salary: m("annualSalary")?,
        hourly_rate: m("hourlyRate")?,
        hours: m("hours")?,
        overtime_hours: m("overtimeHours")?,
        regular_pay: m("regularPay")?,
        overtime_pay: m("overtimePay")?,
        bonus: m("bonus")?,
        other_earnings: m("otherEarnings")?,
        gross: m("gross")?,
        pretax_401k: m("pretax401k")?,
        pretax_section125: m("pretaxSection125")?,
        w4: crate::us_setup::json_col(r, "w4Snapshot")?,
        state_rule: crate::us_setup::json_opt(r, "stateRule")?,
        state_cert: crate::us_setup::json_col(r, "stateCertificate")?,
        state_wages: m("stateIncomeTaxWages")?,
        state_tax: m("stateIncomeTax")?,
        ytd: YearToDate {
            social_security_wages: m("ytdSocialSecurityWages")?,
            medicare_wages: m("ytdMedicareWages")?,
            futa_wages: m("ytdFutaWages")?,
            suta_wages: m("ytdSutaWages")?,
        },
        figures: UsFigures {
            income_tax_wages,
            federal_income_tax,
            social_security_wages,
            social_security_employee,
            social_security_employer,
            medicare_wages,
            medicare_employee,
            additional_medicare,
            medicare_employer,
            futa_wages,
            futa,
            suta_wages,
            suta,
            employee_taxes,
            net_pay,
            employer_taxes,
            cost_to_employer,
        },
        notes: r.try_get("notes")?,
    })
}

fn line_json(l: &Line) -> Value {
    let mut o = json!({
        "id": l.id,
        "runId": l.run_id,
        "employeeId": l.employee_id,
        "employeeCode": l.employee_code,
        "employeeName": l.employee_name,
        "workState": l.work_state,
        "payType": l.pay_type,
        "annualSalary": l.annual_salary.to_json_number(),
        "hourlyRate": l.hourly_rate.to_json_number(),
        "hours": l.hours.to_json_number(),
        "overtimeHours": l.overtime_hours.to_json_number(),
        "regularPay": l.regular_pay.to_json_number(),
        "overtimePay": l.overtime_pay.to_json_number(),
        "bonus": l.bonus.to_json_number(),
        "otherEarnings": l.other_earnings.to_json_number(),
        "gross": l.gross.to_json_number(),
        "pretax401k": l.pretax_401k.to_json_number(),
        "pretaxSection125": l.pretax_section125.to_json_number(),
        "stateTaxConfigured": l.state_rule.is_some(),
        "stateIncomeTaxWages": l.state_wages.to_json_number(),
        "stateIncomeTax": l.state_tax.to_json_number(),
        "w4": l.w4,
        "ytdSocialSecurityWages": l.ytd.social_security_wages.to_json_number(),
        "ytdMedicareWages": l.ytd.medicare_wages.to_json_number(),
        "ytdFutaWages": l.ytd.futa_wages.to_json_number(),
        "ytdSutaWages": l.ytd.suta_wages.to_json_number(),
        "notes": l.notes,
    });
    if let Some(map) = o.as_object_mut() {
        for (k, v) in FIGURES.iter().zip(figure_values(&l.figures)) {
            map.insert((*k).into(), v.to_json_number());
        }
    }
    o
}

async fn lines_of(conn: &mut SqliteConnection, run_id: &str) -> AppResult<Vec<Line>> {
    let rows =
        sqlx::query(r#"SELECT * FROM "us_payroll_lines" WHERE "runId" = ?1 ORDER BY lower("employeeCode"), "rowid""#)
            .bind(run_id)
            .fetch_all(&mut *conn)
            .await?;
    rows.iter().map(line_from_row).collect()
}

async fn get_line(conn: &mut SqliteConnection, run_id: &str, id: &str) -> AppResult<Line> {
    let row = sqlx::query(r#"SELECT * FROM "us_payroll_lines" WHERE "id" = ?1 AND "runId" = ?2"#)
        .bind(id)
        .bind(run_id)
        .fetch_optional(&mut *conn)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("Line {id} not found on this run")))?;
    line_from_row(&row)
}

async fn write_line(conn: &mut SqliteConnection, l: &Line) -> AppResult<()> {
    let figs = figure_values(&l.figures);
    let mut cols: Vec<&str> = vec![
        "id",
        "runId",
        "employeeId",
        "employeeCode",
        "employeeName",
        "workState",
        "payType",
        "notes",
        "w4Snapshot",
        "stateRule",
        "stateCertificate",
        "annualSalary",
        "hourlyRate",
        "hours",
        "overtimeHours",
        "regularPay",
        "overtimePay",
        "bonus",
        "otherEarnings",
        "gross",
        "pretax401k",
        "pretaxSection125",
        "ytdSocialSecurityWages",
        "ytdMedicareWages",
        "ytdFutaWages",
        "ytdSutaWages",
        "stateIncomeTaxWages",
        "stateIncomeTax",
    ];
    cols.extend_from_slice(FIGURES);
    let placeholders: Vec<String> = (1..=cols.len())
        .map(|i| if (9..=11).contains(&i) { format!("${i}::jsonb") } else { format!("${i}") })
        .collect();
    let updates: Vec<String> = cols.iter().skip(1).map(|c| format!(r#""{c}" = excluded."{c}""#)).collect();
    let sql = format!(
        r#"INSERT INTO "us_payroll_lines" ({}) VALUES ({}) ON CONFLICT ("id") DO UPDATE SET {}"#,
        cols.iter().map(|c| format!(r#""{c}""#)).collect::<Vec<_>>().join(","),
        placeholders.join(","),
        updates.join(", ")
    );
    let mut q = sqlx::query(&sql)
        .bind(&l.id)
        .bind(&l.run_id)
        .bind(&l.employee_id)
        .bind(&l.employee_code)
        .bind(&l.employee_name)
        .bind(&l.work_state)
        .bind(&l.pay_type)
        .bind(&l.notes)
        .bind(l.w4.to_string())
        .bind(l.state_rule.as_ref().map(Value::to_string))
        .bind(l.state_cert.to_string());
    for m in [
        l.annual_salary,
        l.hourly_rate,
        l.hours,
        l.overtime_hours,
        l.regular_pay,
        l.overtime_pay,
        l.bonus,
        l.other_earnings,
        l.gross,
        l.pretax_401k,
        l.pretax_section125,
        l.ytd.social_security_wages,
        l.ytd.medicare_wages,
        l.ytd.futa_wages,
        l.ytd.suta_wages,
        l.state_wages,
        l.state_tax,
    ]
    .into_iter()
    .chain(figs)
    {
        q = q.bind(m.cents());
    }
    q.execute(&mut *conn).await?;
    Ok(())
}

/// Wages on the employee's other posted / paid runs paid in `year`.
async fn year_to_date(
    conn: &mut SqliteConnection,
    employee_id: &str,
    year: i32,
    except_run: &str,
) -> AppResult<YearToDate> {
    let row: (i64, i64, i64, i64) = sqlx::query_as(
        r#"SELECT COALESCE(SUM(l."socialSecurityWages"),0), COALESCE(SUM(l."medicareWages"),0),
                  COALESCE(SUM(l."futaWages"),0), COALESCE(SUM(l."sutaWages"),0)
           FROM "us_payroll_lines" l JOIN "us_payroll_runs" r ON r."id" = l."runId"
           WHERE l."employeeId" = ?1 AND r."status" IN ('POSTED','PAID') AND r."id" <> ?2
             AND CAST(substr(r."payDate", 1, 4) AS INTEGER) = ?3"#,
    )
    .bind(employee_id)
    .bind(except_run)
    .bind(year)
    .fetch_one(&mut *conn)
    .await?;
    Ok(YearToDate {
        social_security_wages: Money::from_cents(row.0),
        medicare_wages: Money::from_cents(row.1),
        futa_wages: Money::from_cents(row.2),
        suta_wages: Money::from_cents(row.3),
    })
}

/// Works out a line's pay and taxes from its inputs (regular pay of a salaried line is kept as
/// stored, so a partial period can be entered by hand).
fn work_out(l: &mut Line, run: &Run) -> AppResult<()> {
    if l.pay_type == "HOURLY" {
        l.regular_pay = Money::line_amount(l.hours, l.hourly_rate).ok_or_else(too_large)?;
        // Time and a half: hours (hundredths) × rate (cents) × 1.5, to the cent.
        l.overtime_pay = half_up(i128::from(l.overtime_hours.cents()) * i128::from(l.hourly_rate.cents()) * 3, 200)?;
    } else {
        l.overtime_pay = Money::ZERO;
    }
    l.gross = sum(&[l.regular_pay, l.overtime_pay, l.bonus, l.other_earnings])?;
    let (w4, exempt) = w4_from_snapshot(&l.w4);
    let s = run_settings(run);
    l.figures = us::compute(&UsInputs {
        year: run.year()?,
        periods_per_year: periods_per_year(&run.pay_frequency),
        gross: l.gross,
        pretax_income_tax_only: l.pretax_401k,
        pretax_all_taxes: l.pretax_section125,
        w4,
        exempt_from_withholding: exempt,
        ytd: l.ytd,
        suta_rate: s.suta_rate,
        suta_wage_base: s.suta_wage_base,
        futa_credit_reduction: s.futa_credit_reduction,
    })
    .map_err(|e| match e {
        AppError::BadRequest(_) => AppError::bad_request(format!("{}: {}", l.employee_name, e.user_message())),
        other => other,
    })?;
    let rule = l.state_rule.as_ref().and_then(StateRule::from_snapshot);
    (l.state_wages, l.state_tax) = match rule {
        Some(r) => {
            let wages = us_state::state_wages(l.gross, l.pretax_401k, l.pretax_section125, &r);
            let cert = cert_from_snapshot(&l.state_cert);
            (wages, us_state::withholding(wages, periods_per_year(&run.pay_frequency), &r, &cert)?)
        }
        None => (Money::ZERO, Money::ZERO),
    };
    l.figures.employee_taxes = add(l.figures.employee_taxes, l.state_tax)?;
    l.figures.net_pay = l.figures.net_pay.checked_sub(l.state_tax).ok_or_else(too_large)?;
    Ok(())
}

/// The employee's state certificate, as stored on a line.
fn cert_snapshot(t: &UsTax) -> Value {
    json!({
        "married": t.state_filing_status == "MARRIED",
        "allowances": t.state_allowances,
        "extraWithholding": t.state_extra_withholding.cents(),
        "exempt": t.state_exempt,
    })
}

fn cert_from_snapshot(v: &Value) -> StateEmployee {
    StateEmployee {
        married: v["married"].as_bool().unwrap_or(false),
        allowances: v["allowances"].as_i64().unwrap_or(0),
        extra: Money::from_cents(v["extraWithholding"].as_i64().unwrap_or(0)),
        exempt: v["exempt"].as_bool().unwrap_or(false),
    }
}

/// Fills in the state rule of a line that has none yet, if the company has entered one since.
async fn fill_rule(conn: &mut SqliteConnection, l: &mut Line) -> AppResult<()> {
    if l.state_rule.is_none() {
        l.state_rule = load_rule(conn, &l.work_state).await?.map(|r| r.to_snapshot());
    }
    Ok(())
}

async fn recalc_totals(conn: &mut SqliteConnection, run_id: &str) -> AppResult<()> {
    let sums: Vec<String> = TOTALS
        .iter()
        .map(|(k, col)| {
            format!(r#""{k}" = (SELECT COALESCE(SUM("{col}"),0) FROM "us_payroll_lines" WHERE "runId" = ?1)"#)
        })
        .collect();
    sqlx::query(&format!(
        r#"UPDATE "us_payroll_runs" SET {}, "employeeCount" = (SELECT count(*) FROM "us_payroll_lines" WHERE "runId" = ?1),
           "updatedAt" = strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE "id" = ?1"#,
        sums.join(", ")
    ))
    .bind(run_id)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

// ─── Remittances ────────────────────────────────────────────────────────────

/// (type, owed on this run, already remitted) for each payable a remittance can clear.
async fn owed(conn: &mut SqliteConnection, run: &Run) -> AppResult<Vec<(&'static str, Money, Money)>> {
    let paid: Vec<(String, i64)> = sqlx::query_as(
        r#"SELECT "type", SUM("amount") FROM "us_payroll_remittances" WHERE "runId" = ?1 GROUP BY "type""#,
    )
    .bind(&run.id)
    .fetch_all(&mut *conn)
    .await?;
    let done = |k: &str| Money::from_cents(paid.iter().find(|(t, _)| t == k).map(|(_, a)| *a).unwrap_or(0));
    let t = |k: &str| run.total(k);
    Ok(vec![
        ("FEDERAL_INCOME_TAX", t("federalIncomeTaxTotal"), done("FEDERAL_INCOME_TAX")),
        (
            "SOCIAL_SECURITY",
            add(t("socialSecurityEmployeeTotal"), t("socialSecurityEmployerTotal"))?,
            done("SOCIAL_SECURITY"),
        ),
        (
            "MEDICARE",
            sum(&[t("medicareEmployeeTotal"), t("additionalMedicareTotal"), t("medicareEmployerTotal")])?,
            done("MEDICARE"),
        ),
        ("FUTA", t("futaTotal"), done("FUTA")),
        ("SUTA", t("sutaTotal"), done("SUTA")),
        ("RETIREMENT_401K", t("pretax401kTotal"), done("RETIREMENT_401K")),
        ("SECTION_125", t("pretaxSection125Total"), done("SECTION_125")),
        ("STATE_INCOME_TAX", t("stateIncomeTaxTotal"), done("STATE_INCOME_TAX")),
    ])
}

fn payable_role(kind: &str) -> &'static str {
    match kind {
        "FEDERAL_INCOME_TAX" => "federalIncomeTax",
        "SOCIAL_SECURITY" => "socialSecurity",
        "MEDICARE" => "medicare",
        "FUTA" => "futa",
        "SUTA" => "suta",
        "RETIREMENT_401K" => "retirement401k",
        "STATE_INCOME_TAX" => "stateIncomeTax",
        _ => "section125",
    }
}

async fn detail(conn: &mut SqliteConnection, id: &str) -> AppResult<Value> {
    let run = get_run(conn, id).await?;
    let lines = lines_of(conn, id).await?;
    let rows = sqlx::query(r#"SELECT * FROM "us_payroll_remittances" WHERE "runId" = ?1 ORDER BY "createdAt", "id""#)
        .bind(id)
        .fetch_all(&mut *conn)
        .await?;
    let mut remittances = Vec::new();
    for r in &rows {
        remittances.push(json!({
            "id": r.try_get::<String, _>("id")?,
            "type": r.try_get::<String, _>("type")?,
            "amount": Money::from_cents(r.try_get::<i64, _>("amount")?).to_json_number(),
            "date": r.try_get::<String, _>("date")?,
            "paymentAccountId": r.try_get::<String, _>("paymentAccountId")?,
            "reference": r.try_get::<Option<String>, _>("reference")?,
            "journalEntryId": r.try_get::<String, _>("journalEntryId")?,
            "createdByName": r.try_get::<Option<String>, _>("createdByName")?,
            "createdAt": r.try_get::<String, _>("createdAt")?,
        }));
    }
    let mut outstanding = Vec::new();
    for (kind, total, remitted) in owed(conn, &run).await? {
        let left = total.checked_sub(remitted).ok_or_else(too_large)?;
        outstanding.push(json!({
            "type": kind,
            "total": total.to_json_number(),
            "remitted": remitted.to_json_number(),
            "outstanding": left.to_json_number(),
        }));
    }
    let mut o = run.to_json();
    if let Some(map) = o.as_object_mut() {
        map.insert("lines".into(), Value::Array(lines.iter().map(line_json).collect()));
        map.insert("remittances".into(), Value::Array(remittances));
        map.insert("taxes".into(), Value::Array(outstanding));
    }
    Ok(o)
}

pub async fn find_one(db: &Db, id: &str) -> AppResult<Value> {
    let mut c = db.read().acquire().await?;
    detail(&mut c, id).await
}

// ─── Creating a run and editing lines ───────────────────────────────────────

pub struct NewRun {
    pub period_start: String,
    pub period_end: String,
    pub pay_date: String,
    pub notes: Option<String>,
}

struct Who {
    id: String,
    code: String,
    name: String,
    status: String,
    join_date: Option<String>,
    resigned_date: Option<String>,
}

fn ineligible(e: &Who, start: &str, end: &str) -> Option<String> {
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

async fn new_line(conn: &mut SqliteConnection, run: &Run, e: &Who, t: &UsTax) -> AppResult<Line> {
    let periods = periods_per_year(&run.pay_frequency);
    let mut l = Line {
        id: new_id(),
        run_id: run.id.clone(),
        employee_id: Some(e.id.clone()),
        employee_code: e.code.clone(),
        employee_name: e.name.clone(),
        work_state: t.work_state.clone(),
        pay_type: t.pay_type.clone(),
        annual_salary: t.annual_salary,
        hourly_rate: t.hourly_rate,
        hours: if t.pay_type == "HOURLY" { standard_hours(periods) } else { Money::ZERO },
        overtime_hours: Money::ZERO,
        regular_pay: if t.pay_type == "SALARY" {
            half_up(i128::from(t.annual_salary.cents()), i128::from(periods))?
        } else {
            Money::ZERO
        },
        overtime_pay: Money::ZERO,
        bonus: Money::ZERO,
        other_earnings: Money::ZERO,
        gross: Money::ZERO,
        pretax_401k: t.pretax_401k,
        pretax_section125: t.pretax_section125,
        w4: w4_snapshot(t),
        state_rule: load_rule(conn, &t.work_state).await?.map(|r| r.to_snapshot()),
        state_cert: cert_snapshot(t),
        state_wages: Money::ZERO,
        state_tax: Money::ZERO,
        ytd: year_to_date(conn, &e.id, run.year()?, &run.id).await?,
        figures: UsFigures::default(),
        notes: None,
    };
    work_out(&mut l, run)?;
    Ok(l)
}

async fn who(conn: &mut SqliteConnection, employee_id: Option<&str>) -> AppResult<Vec<Who>> {
    let rows = sqlx::query(
        r#"SELECT "id","employeeId","name","status","joinDate","resignedDate" FROM "employees"
           WHERE ?1 IS NULL OR "id" = ?1 ORDER BY lower("employeeId"), "createdAt", "id""#,
    )
    .bind(employee_id)
    .fetch_all(&mut *conn)
    .await?;
    rows.iter()
        .map(|r| {
            Ok(Who {
                id: r.try_get("id")?,
                code: r.try_get("employeeId")?,
                name: r.try_get("name")?,
                status: r.try_get("status")?,
                join_date: r.try_get::<Option<String>, _>("joinDate")?,
                resigned_date: r.try_get::<Option<String>, _>("resignedDate")?,
            })
        })
        .collect()
}

pub async fn create(db: &Db, n: NewRun, by: &Actor) -> AppResult<Value> {
    let mut tx = db.begin().await?;
    let settings = load_settings(&mut tx).await?;
    if settings.country != "US" {
        return Err(AppError::bad_request("Switch payroll to the United States in the US payroll settings first"));
    }
    crate::us_setup::check_currency(&mut tx).await?;
    if n.period_start > n.period_end {
        return Err(AppError::bad_request("periodStart must be on or before periodEnd"));
    }
    let year: i32 = n
        .pay_date
        .get(..4)
        .and_then(|y| y.parse().ok())
        .ok_or_else(|| AppError::bad_request("payDate must be YYYY-MM-DD"))?;
    us::tables(year)?;
    let clash: Option<(String, String)> = sqlx::query_as(
        r#"SELECT "runNumber","status" FROM "us_payroll_runs" WHERE "status" <> 'VOID'
           AND "periodStart" <= ?2 AND "periodEnd" >= ?1 ORDER BY "periodStart" LIMIT 1"#,
    )
    .bind(&n.period_start)
    .bind(&n.period_end)
    .fetch_optional(&mut *tx)
    .await?;
    if let Some((number, status)) = clash {
        return Err(AppError::bad_request(format!("This pay period overlaps {number} ({status})")));
    }
    ensure_accounts(&mut tx).await?;
    let id = new_id();
    let number = counters::next_formatted(&mut tx, "USPR", 4).await?;
    sqlx::query(
        r#"INSERT INTO "us_payroll_runs" ("id","runNumber","periodStart","periodEnd","payDate","payFrequency","notes",
           "settingsSnapshot","createdById","createdByName") VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)"#,
    )
    .bind(&id)
    .bind(&number)
    .bind(&n.period_start)
    .bind(&n.period_end)
    .bind(&n.pay_date)
    .bind(&settings.pay_frequency)
    .bind(&n.notes)
    .bind(snapshot_of(&settings).to_string())
    .bind(&by.id)
    .bind(&by.username)
    .execute(&mut *tx)
    .await?;
    let run = get_run(&mut tx, &id).await?;

    let mut skipped = Vec::new();
    for e in who(&mut tx, None).await? {
        let skip =
            |reason: String| json!({"employeeId": e.id, "employeeCode": e.code, "name": e.name, "reason": reason});
        if let Some(why) = ineligible(&e, &n.period_start, &n.period_end) {
            skipped.push(skip(why));
            continue;
        }
        let Some((t, _)) = load_tax(&mut tx, &e.id).await? else {
            skipped.push(skip("No US pay and Form W-4 details".into()));
            continue;
        };
        let line = new_line(&mut tx, &run, &e, &t).await?;
        write_line(&mut tx, &line).await?;
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
    let e = who(&mut tx, Some(employee_id))
        .await?
        .into_iter()
        .next()
        .ok_or_else(|| AppError::NotFound(format!("Employee {employee_id} not found")))?;
    let taken: Option<String> =
        sqlx::query_scalar(r#"SELECT "id" FROM "us_payroll_lines" WHERE "runId" = ?1 AND "employeeId" = ?2"#)
            .bind(run_id)
            .bind(employee_id)
            .fetch_optional(&mut *tx)
            .await?;
    if taken.is_some() {
        return Err(AppError::bad_request(format!("{} is already on this run", e.name)));
    }
    let (t, _) = load_tax(&mut tx, employee_id)
        .await?
        .ok_or_else(|| AppError::bad_request(format!("Add the US pay and Form W-4 details for {} first", e.name)))?;
    let line = new_line(&mut tx, &run, &e, &t).await?;
    write_line(&mut tx, &line).await?;
    recalc_totals(&mut tx, run_id).await?;
    let out = detail(&mut tx, run_id).await?;
    tx.commit().await?;
    Ok(out)
}

/// Sent fields only. `refresh` re-reads the employee's current pay and Form W-4.
#[derive(Debug, Default)]
pub struct LinePatch {
    pub hours: Option<Money>,
    pub overtime_hours: Option<Money>,
    pub regular_pay: Option<Money>,
    pub bonus: Option<Money>,
    pub other_earnings: Option<Money>,
    pub notes: Option<Option<String>>,
    pub refresh: bool,
}

pub async fn update_line(db: &Db, run_id: &str, line_id: &str, p: LinePatch) -> AppResult<Value> {
    let mut tx = db.begin().await?;
    let run = draft_only(&mut tx, run_id).await?;
    let mut l = get_line(&mut tx, run_id, line_id).await?;
    if p.refresh {
        let emp = l.employee_id.clone().ok_or_else(|| AppError::bad_request("This employee no longer exists"))?;
        let (t, _) = load_tax(&mut tx, &emp)
            .await?
            .ok_or_else(|| AppError::bad_request(format!("{} has no US pay and Form W-4 details", l.employee_name)))?;
        let periods = periods_per_year(&run.pay_frequency);
        if t.pay_type != l.pay_type {
            l.hours = if t.pay_type == "HOURLY" { standard_hours(periods) } else { Money::ZERO };
            l.overtime_hours = Money::ZERO;
        }
        l.work_state = t.work_state.clone();
        l.pay_type = t.pay_type.clone();
        l.annual_salary = t.annual_salary;
        l.hourly_rate = t.hourly_rate;
        if t.pay_type == "SALARY" {
            l.regular_pay = half_up(i128::from(t.annual_salary.cents()), i128::from(periods))?;
        }
        l.pretax_401k = t.pretax_401k;
        l.pretax_section125 = t.pretax_section125;
        l.w4 = w4_snapshot(&t);
        l.state_rule = load_rule(&mut tx, &t.work_state).await?.map(|r| r.to_snapshot());
        l.state_cert = cert_snapshot(&t);
    }
    fill_rule(&mut tx, &mut l).await?;
    let hourly = l.pay_type == "HOURLY";
    if let Some(h) = p.hours {
        if !hourly {
            return Err(AppError::bad_request("hours only apply to an hourly employee"));
        }
        l.hours = h;
    }
    if let Some(h) = p.overtime_hours {
        if !hourly && h > Money::ZERO {
            return Err(AppError::bad_request("overtimeHours only apply to an hourly employee"));
        }
        l.overtime_hours = h;
    }
    if let Some(r) = p.regular_pay {
        if hourly {
            return Err(AppError::bad_request("An hourly employee's regular pay is rate × hours"));
        }
        l.regular_pay = r;
    }
    if let Some(b) = p.bonus {
        l.bonus = b;
    }
    if let Some(o) = p.other_earnings {
        l.other_earnings = o;
    }
    if let Some(n) = p.notes {
        l.notes = n;
    }
    if let Some(emp) = l.employee_id.clone() {
        l.ytd = year_to_date(&mut tx, &emp, run.year()?, run_id).await?;
    }
    work_out(&mut l, &run)?;
    write_line(&mut tx, &l).await?;
    recalc_totals(&mut tx, run_id).await?;
    let out = detail(&mut tx, run_id).await?;
    tx.commit().await?;
    Ok(out)
}

pub async fn remove_line(db: &Db, run_id: &str, line_id: &str) -> AppResult<Value> {
    let mut tx = db.begin().await?;
    draft_only(&mut tx, run_id).await?;
    get_line(&mut tx, run_id, line_id).await?;
    sqlx::query(r#"DELETE FROM "us_payroll_lines" WHERE "id" = ?1"#).bind(line_id).execute(&mut *tx).await?;
    recalc_totals(&mut tx, run_id).await?;
    let out = detail(&mut tx, run_id).await?;
    tx.commit().await?;
    Ok(out)
}

pub async fn remove(db: &Db, run_id: &str) -> AppResult<()> {
    let mut tx = db.begin().await?;
    draft_only(&mut tx, run_id).await?;
    sqlx::query(r#"DELETE FROM "us_payroll_runs" WHERE "id" = ?1"#).bind(run_id).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(())
}

// ─── Post / pay / remit / void ──────────────────────────────────────────────

fn line(account: &str, debit: Money, credit: Money, description: &str) -> Vec<NewLine> {
    if debit > Money::ZERO || credit > Money::ZERO {
        vec![NewLine { account_id: account.into(), debit, credit, description: Some(description.into()) }]
    } else {
        Vec::new()
    }
}

pub async fn post(db: &Db, run_id: &str) -> AppResult<Value> {
    let mut tx = db.begin().await?;
    let run = draft_only(&mut tx, run_id).await?;
    let mut lines = lines_of(&mut tx, run_id).await?;
    if lines.is_empty() {
        return Err(AppError::bad_request("Add at least one employee before posting"));
    }
    // Every work state needs its rules before money moves.
    let mut missing = std::collections::BTreeSet::new();
    for l in &mut lines {
        fill_rule(&mut tx, l).await?;
        if l.state_rule.is_none() {
            missing.insert(l.work_state.clone());
        }
    }
    if !missing.is_empty() {
        return Err(AppError::bad_request(format!(
            "Add the state income tax rules for {} under US payroll settings before posting",
            missing.into_iter().collect::<Vec<_>>().join(", ")
        )));
    }
    // Other runs may have been posted since this one was drafted: year-to-date again.
    for l in &mut lines {
        if let Some(emp) = l.employee_id.clone() {
            l.ytd = year_to_date(&mut tx, &emp, run.year()?, run_id).await?;
        }
        work_out(l, &run)?;
        write_line(&mut tx, l).await?;
    }
    recalc_totals(&mut tx, run_id).await?;
    let run = get_run(&mut tx, run_id).await?;
    let chosen = ensure_accounts(&mut tx).await?;
    let snap = Value::Object(chosen.into_iter().map(|(k, id)| (k.to_string(), Value::String(id))).collect());
    let t = |k: &str| run.total(k);
    let zero = Money::ZERO;

    let mut e = Vec::new();
    e.extend(line(&account_of(&snap, "salariesExpense")?, t("grossTotal"), zero, "Gross wages"));
    e.extend(line(
        &account_of(&snap, "payrollTaxExpense")?,
        t("employerTaxesTotal"),
        zero,
        "Employer social security, Medicare, FUTA and SUTA",
    ));
    e.extend(line(&account_of(&snap, "salariesPayable")?, zero, t("netPayTotal"), "Net pay"));
    e.extend(line(
        &account_of(&snap, "federalIncomeTax")?,
        zero,
        t("federalIncomeTaxTotal"),
        "Federal income tax withheld",
    ));
    e.extend(line(
        &account_of(&snap, "socialSecurity")?,
        zero,
        add(t("socialSecurityEmployeeTotal"), t("socialSecurityEmployerTotal"))?,
        "Social security 6.2% + 6.2%",
    ));
    e.extend(line(
        &account_of(&snap, "medicare")?,
        zero,
        sum(&[t("medicareEmployeeTotal"), t("additionalMedicareTotal"), t("medicareEmployerTotal")])?,
        "Medicare 1.45% + 1.45% and Additional Medicare",
    ));
    e.extend(line(&account_of(&snap, "futa")?, zero, t("futaTotal"), "FUTA"));
    e.extend(line(&account_of(&snap, "suta")?, zero, t("sutaTotal"), "State unemployment"));
    e.extend(line(&account_of(&snap, "retirement401k")?, zero, t("pretax401kTotal"), "401(k) contributions"));
    e.extend(line(&account_of(&snap, "section125")?, zero, t("pretaxSection125Total"), "Section 125 deductions"));
    e.extend(line(&account_of(&snap, "stateIncomeTax")?, zero, t("stateIncomeTaxTotal"), "State income tax withheld"));

    let entry = entries::post(
        &mut tx,
        NewEntry {
            date: run.period_end.clone(),
            description: format!("US payroll {} to {} ({})", run.period_start, run.period_end, run.run_number),
            reference: Some(run.run_number.clone()),
            source_type: Some("US_PAYROLL".into()),
            source_id: Some(run.id.clone()),
            lines: e,
        },
    )
    .await?;
    sqlx::query(r#"UPDATE "us_payroll_runs" SET "status"='POSTED',"journalEntryId"=?1,"accountsSnapshot"=?3,"postedAt"=strftime('%Y-%m-%dT%H:%M:%fZ','now'),"updatedAt"=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE "id"=?2"#)
        .bind(&entry.id)
        .bind(run_id)
        .bind(snap.to_string())
        .execute(&mut *tx)
        .await?;
    let out = detail(&mut tx, run_id).await?;
    tx.commit().await?;
    Ok(out)
}

pub async fn pay(db: &Db, run_id: &str, payment_account_id: &str, date: Option<String>) -> AppResult<Value> {
    let mut tx = db.begin().await?;
    let run = get_run(&mut tx, run_id).await?;
    if run.status != "POSTED" {
        return Err(AppError::bad_request(format!("Only a POSTED run can be paid — this one is {}", run.status)));
    }
    let net = run.total("netPayTotal");
    if net <= Money::ZERO {
        return Err(AppError::bad_request("Nothing to pay — net pay is zero"));
    }
    let bank = accounts::find_one(&mut tx, payment_account_id).await?;
    let paid_on = date.unwrap_or_else(|| run.pay_date.clone());
    let mut lines = line(&account_of(&run.accounts, "salariesPayable")?, net, Money::ZERO, "Net pay cleared");
    lines.extend(line(&bank.id, Money::ZERO, net, &format!("Paid from {}", bank.name)));
    let entry = entries::post(
        &mut tx,
        NewEntry {
            date: paid_on.clone(),
            description: format!("US payroll paid ({})", run.run_number),
            reference: Some(run.run_number.clone()),
            source_type: Some("US_PAYROLL_PAYMENT".into()),
            source_id: Some(run.id.clone()),
            lines,
        },
    )
    .await?;
    sqlx::query(
        r#"UPDATE "us_payroll_runs" SET "status"='PAID',"paymentJournalEntryId"=?1,"paymentAccountId"=?2,"paidDate"=?3,
           "paidAt"=strftime('%Y-%m-%dT%H:%M:%fZ','now'),"updatedAt"=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE "id"=?4"#,
    )
    .bind(&entry.id)
    .bind(&bank.id)
    .bind(&paid_on)
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

fn label(kind: &str) -> &'static str {
    match kind {
        "FORM_941" => "Form 941 deposit",
        "FEDERAL_INCOME_TAX" => "Federal income tax",
        "SOCIAL_SECURITY" => "Social security",
        "MEDICARE" => "Medicare",
        "FUTA" => "FUTA",
        "SUTA" => "State unemployment",
        "RETIREMENT_401K" => "401(k)",
        "STATE_INCOME_TAX" => "State income tax",
        _ => "Section 125",
    }
}

pub async fn remit(db: &Db, run_id: &str, r: Remit, by: &Actor) -> AppResult<Value> {
    let mut tx = db.begin().await?;
    let run = get_run(&mut tx, run_id).await?;
    if run.status != "POSTED" && run.status != "PAID" {
        return Err(AppError::bad_request("Post the run before recording tax payments"));
    }
    let owed = owed(&mut tx, &run).await?;
    let left = |k: &str| -> AppResult<Money> {
        let (_, total, done) =
            owed.iter().find(|(t, _, _)| *t == k).copied().ok_or_else(|| AppError::internal("type"))?;
        total.checked_sub(done).ok_or_else(too_large)
    };
    // The parts this payment clears: (type, amount).
    let parts: Vec<(&str, Money)> = if r.kind == "FORM_941" {
        if r.amount.is_some() {
            return Err(AppError::bad_request("A Form 941 deposit pays everything outstanding — leave amount out"));
        }
        let mut v = Vec::new();
        for k in ["FEDERAL_INCOME_TAX", "SOCIAL_SECURITY", "MEDICARE"] {
            let a = left(k)?;
            if a > Money::ZERO {
                v.push((k, a));
            }
        }
        v
    } else {
        let outstanding = left(&r.kind)?;
        let amount = r.amount.unwrap_or(outstanding);
        if amount > outstanding {
            return Err(AppError::bad_request(format!(
                "{} payment exceeds the outstanding {outstanding}",
                label(&r.kind)
            )));
        }
        if amount > Money::ZERO {
            vec![(REMIT_TYPES.iter().copied().find(|t| *t == r.kind).unwrap_or("SECTION_125"), amount)]
        } else {
            Vec::new()
        }
    };
    if parts.is_empty() {
        return Err(AppError::bad_request(format!("Nothing outstanding for {} on this run", label(&r.kind))));
    }
    let total = sum(&parts.iter().map(|(_, a)| *a).collect::<Vec<_>>())?;
    let bank = accounts::find_one(&mut tx, &r.payment_account_id).await?;
    let mut lines = Vec::new();
    for (k, a) in &parts {
        let payable = accounts::find_one(&mut tx, &account_of(&run.accounts, payable_role(k))?).await?;
        lines.extend(line(&payable.id, *a, Money::ZERO, &format!("{} cleared", payable.name)));
    }
    lines.extend(line(&bank.id, Money::ZERO, total, &format!("Paid from {}", bank.name)));
    let entry = entries::post(
        &mut tx,
        NewEntry {
            date: r.date.clone(),
            description: format!("{} for {} ({})", label(&r.kind), run.pay_date, run.run_number),
            reference: Some(r.reference.clone().unwrap_or_else(|| run.run_number.clone())),
            source_type: Some("US_PAYROLL_REMITTANCE".into()),
            source_id: Some(run.id.clone()),
            lines,
        },
    )
    .await?;
    for (k, a) in &parts {
        sqlx::query(
            r#"INSERT INTO "us_payroll_remittances" ("id","runId","type","amount","date","paymentAccountId","reference",
               "journalEntryId","createdByName") VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)"#,
        )
        .bind(new_id())
        .bind(run_id)
        .bind(k)
        .bind(a.cents())
        .bind(&r.date)
        .bind(&bank.id)
        .bind(&r.reference)
        .bind(&entry.id)
        .bind(&by.username)
        .execute(&mut *tx)
        .await?;
    }
    let out = detail(&mut tx, run_id).await?;
    tx.commit().await?;
    Ok(out)
}

/// Reverses every entry the run made (tax payments, net pay, posting), dated today.
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
    let deposits: Vec<String> = sqlx::query_scalar(
        r#"SELECT "journalEntryId" FROM "us_payroll_remittances" WHERE "runId" = ?1
           GROUP BY "journalEntryId" ORDER BY MIN("createdAt"), "journalEntryId""#,
    )
    .bind(run_id)
    .fetch_all(&mut *tx)
    .await?;
    for je in deposits {
        entries::reverse(&mut tx, &je, &today).await?;
    }
    if let Some(je) = &run.payment_journal_entry_id {
        entries::reverse(&mut tx, je, &today).await?;
    }
    if let Some(je) = &run.journal_entry_id {
        entries::reverse(&mut tx, je, &today).await?;
    }
    sqlx::query(r#"UPDATE "us_payroll_runs" SET "status"='VOID',"updatedAt"=strftime('%Y-%m-%dT%H:%M:%fZ','now') WHERE "id"=?1"#)
        .bind(run_id)
        .execute(&mut *tx)
        .await?;
    let out = detail(&mut tx, run_id).await?;
    tx.commit().await?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_hours_per_period() {
        assert_eq!(standard_hours(52), Money::from_cents(4_000));
        assert_eq!(standard_hours(26), Money::from_cents(8_000));
        assert_eq!(standard_hours(24), Money::from_cents(8_667));
        assert_eq!(standard_hours(12), Money::from_cents(17_333));
    }

    #[test]
    fn w4_snapshot_round_trips() {
        let mut t = crate::us_setup::UsTax {
            work_state: "TX".into(),
            pay_type: "SALARY".into(),
            annual_salary: Money::from_cents(5_200_000),
            hourly_rate: Money::ZERO,
            w4_version: "2020".into(),
            filing_status: "HEAD_OF_HOUSEHOLD".into(),
            multiple_jobs: true,
            dependents_credit: Money::from_cents(200_000),
            other_income: Money::from_cents(10_000),
            deductions: Money::from_cents(20_000),
            extra_withholding: Money::from_cents(2_500),
            allowances: 0,
            exempt_from_withholding: false,
            pretax_401k: Money::ZERO,
            pretax_section125: Money::ZERO,
            state_filing_status: "MARRIED".into(),
            state_allowances: 2,
            state_extra_withholding: Money::from_cents(1_500),
            state_exempt: false,
        };
        assert_eq!(w4_from_snapshot(&w4_snapshot(&t)), (t.w4(), false));
        assert_eq!(cert_from_snapshot(&cert_snapshot(&t)), t.state_employee());
        t.w4_version = "LEGACY".into();
        t.filing_status = "MARRIED_JOINTLY".into();
        t.allowances = 3;
        t.exempt_from_withholding = true;
        assert_eq!(w4_from_snapshot(&w4_snapshot(&t)), (t.w4(), true));
    }
}
