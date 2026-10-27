//! US payroll forms (D43, ported from the web edition's wave U5a): the figures for Form 941 (quarterly), Form 940 (FUTA, yearly) and each
//! employee's Form W-2, from POSTED and PAID US pay runs. Wages count in the quarter / year they
//! are PAID (the run's pay date), as the instructions require. The app does not file anything;
//! these are the numbers to copy onto the forms.
//!
//! Line rules (Instructions for Form 941, rev. 03/2026; Instructions for Form 940 (2025);
//! Publication 15 (2026)):
//! - 941 line 1: employees paid for the pay period including Mar 12 / Jun 12 / Sep 12 / Dec 12.
//!   Line 2: what goes in W-2 box 1 (income-tax wages: gross less 401(k) and section 125).
//!   Line 3: federal income tax withheld. Lines 5a / 5c / 5d: taxable social security wages ×
//!   0.124, Medicare wages × 0.029, wages over the $200,000 Additional Medicare threshold ×
//!   0.009. Line 7: fractions of cents, so line 10 equals what was actually withheld and owed.
//!   Line 13: deposits. Line 16: liability by month of the pay date.
//! - 940 line 3: all payments to employees (gross). Line 4: exempt payments (section 125; 401(k)
//!   elective deferrals are FUTA wages). Line 5: each employee's payments over $7,000. Line 7:
//!   taxable FUTA wages; line 8: × 0.006. Line 11: credit reduction. Line 16: liability by
//!   quarter, with the credit reduction in the fourth quarter.
//! - W-2: box 1 income-tax wages, 2 federal income tax, 3 social security wages, 4 social
//!   security tax, 5 Medicare wages, 6 Medicare tax (with Additional Medicare), 12 code D 401(k).
//!   Boxes 15–17: the work state, its wages and the state income tax withheld.

use crate::us::PPM;
use crate::us_setup::load_settings;
use app_core::money::Money;
use app_core::{AppError, AppResult};
use database::Db;
use serde_json::{json, Value};
use sqlx::{Row, SqliteConnection};
use std::collections::BTreeMap;

const ADDITIONAL_MEDICARE_THRESHOLD: i64 = 20_000_000;
const FUTA_WAGE_BASE: i64 = 700_000;

fn too_large() -> AppError {
    AppError::bad_request("Amount is too large")
}

/// `cents × ppm / 1e6`, half-up.
fn at(cents: i64, ppm: i64) -> AppResult<Money> {
    let n = i128::from(cents.max(0)) * i128::from(ppm);
    let d = i128::from(PPM);
    i64::try_from((2 * n + d) / (2 * d)).map(Money::from_cents).map_err(|_| too_large())
}

fn m(c: i64) -> Value {
    Money::from_cents(c).to_json_number()
}

/// One paid line with what the forms need.
struct Paid {
    run_id: String,
    pay_date: String,
    reduction: i64,
    employee_id: Option<String>,
    code: String,
    name: String,
    state: String,
    gross: i64,
    pretax_401k: i64,
    pretax_125: i64,
    income_tax_wages: i64,
    fit: i64,
    ss_wages: i64,
    ss_ee: i64,
    ss_er: i64,
    medicare_wages: i64,
    ytd_medicare: i64,
    medicare_ee: i64,
    additional: i64,
    medicare_er: i64,
    futa_wages: i64,
    futa: i64,
    state_wages: i64,
    state_tax: i64,
}

impl Paid {
    /// Wages in this pay over the $200,000 Additional Medicare withholding threshold.
    fn additional_wages(&self) -> i64 {
        let paid = self.ytd_medicare.max(0);
        (paid + self.medicare_wages - ADDITIONAL_MEDICARE_THRESHOLD.max(paid)).clamp(0, self.medicare_wages)
    }

    /// Everything Form 941 covers: income tax plus both shares of FICA and Additional Medicare.
    fn liability_941(&self) -> i64 {
        self.fit + self.ss_ee + self.ss_er + self.medicare_ee + self.additional + self.medicare_er
    }

    fn employee_key(&self) -> String {
        self.employee_id.clone().unwrap_or_else(|| format!("code:{}", self.code))
    }
}

async fn paid_lines(conn: &mut SqliteConnection, from: &str, to: &str) -> AppResult<Vec<Paid>> {
    let rows = sqlx::query(
        r#"SELECT r."id" AS "runId", r."payDate",
                  COALESCE(json_extract(r."settingsSnapshot", '$.futaCreditReduction'), 0) AS "reduction",
                  l."employeeId", l."employeeCode", l."employeeName", l."workState", l."gross", l."pretax401k",
                  l."pretaxSection125", l."incomeTaxWages", l."federalIncomeTax", l."socialSecurityWages",
                  l."socialSecurityEmployee", l."socialSecurityEmployer", l."medicareWages", l."ytdMedicareWages",
                  l."medicareEmployee", l."additionalMedicare", l."medicareEmployer", l."futaWages", l."futa",
                  l."stateIncomeTaxWages", l."stateIncomeTax"
           FROM "us_payroll_lines" l JOIN "us_payroll_runs" r ON r."id" = l."runId"
           WHERE r."status" IN ('POSTED','PAID') AND r."payDate" BETWEEN ?1 AND ?2
           ORDER BY r."payDate", r."runNumber", lower(l."employeeCode"), l."rowid""#,
    )
    .bind(from)
    .bind(to)
    .fetch_all(&mut *conn)
    .await?;
    rows.iter()
        .map(|r| {
            let c = |k: &str| r.try_get::<i64, _>(k);
            Ok(Paid {
                run_id: r.try_get("runId")?,
                pay_date: r.try_get::<String, _>("payDate")?,
                reduction: c("reduction")?,
                employee_id: r.try_get("employeeId")?,
                code: r.try_get("employeeCode")?,
                name: r.try_get("employeeName")?,
                state: r.try_get("workState")?,
                gross: c("gross")?,
                pretax_401k: c("pretax401k")?,
                pretax_125: c("pretaxSection125")?,
                income_tax_wages: c("incomeTaxWages")?,
                fit: c("federalIncomeTax")?,
                ss_wages: c("socialSecurityWages")?,
                ss_ee: c("socialSecurityEmployee")?,
                ss_er: c("socialSecurityEmployer")?,
                medicare_wages: c("medicareWages")?,
                ytd_medicare: c("ytdMedicareWages")?,
                medicare_ee: c("medicareEmployee")?,
                additional: c("additionalMedicare")?,
                medicare_er: c("medicareEmployer")?,
                futa_wages: c("futaWages")?,
                futa: c("futa")?,
                state_wages: c("stateIncomeTaxWages")?,
                state_tax: c("stateIncomeTax")?,
            })
        })
        .collect()
}

/// Tax paid over (remittances) of the given types, for the runs paid in the period.
async fn deposits(conn: &mut SqliteConnection, from: &str, to: &str, types: &[&str]) -> AppResult<i64> {
    let types = serde_json::to_string(types).map_err(|e| AppError::internal(e.to_string()))?;
    let total: i64 = sqlx::query_scalar(
        r#"SELECT COALESCE(SUM(x."amount"),0) FROM "us_payroll_remittances" x
           JOIN "us_payroll_runs" r ON r."id" = x."runId"
           WHERE r."status" IN ('POSTED','PAID') AND r."payDate" BETWEEN ?1 AND ?2 AND x."type" IN (SELECT "value" FROM json_each(?3))"#,
    )
    .bind(from)
    .bind(to)
    .bind(&types)
    .fetch_one(&mut *conn)
    .await?;
    Ok(total)
}

fn check_year(year: i32) -> AppResult<()> {
    if !(2000..=2100).contains(&year) {
        return Err(AppError::bad_request("year must be between 2000 and 2100"));
    }
    Ok(())
}

fn quarter_range(year: i32, quarter: u32) -> AppResult<(String, String, String)> {
    let (from, to, twelfth) = match quarter {
        1 => ("01-01", "03-31", "03-12"),
        2 => ("04-01", "06-30", "06-12"),
        3 => ("07-01", "09-30", "09-12"),
        4 => ("10-01", "12-31", "12-12"),
        _ => return Err(AppError::bad_request("quarter must be 1, 2, 3 or 4")),
    };
    Ok((format!("{year}-{from}"), format!("{year}-{to}"), format!("{year}-{twelfth}")))
}

async fn employer(conn: &mut SqliteConnection) -> AppResult<Value> {
    let s = load_settings(conn).await?;
    let name: Option<String> = sqlx::query_scalar(r#"SELECT "value" FROM "settings" WHERE "key" = 'company.name'"#)
        .fetch_optional(&mut *conn)
        .await?;
    Ok(json!({ "name": name.filter(|n| !n.trim().is_empty()), "ein": s.ein, "state": s.state }))
}

/// `GET /payroll/us/reports/941?year&quarter`.
pub async fn form_941(db: &Db, year: i32, quarter: u32) -> AppResult<Value> {
    check_year(year)?;
    let (from, to, twelfth) = quarter_range(year, quarter)?;
    let mut c = db.read().acquire().await?;
    let lines = paid_lines(&mut c, &from, &to).await?;

    // Line 1 looks at the pay period, wherever the pay date falls.
    let line1: i64 = sqlx::query_scalar(
        r#"SELECT count(DISTINCT COALESCE(l."employeeId", 'code:' || l."employeeCode"))
           FROM "us_payroll_lines" l JOIN "us_payroll_runs" r ON r."id" = l."runId"
           WHERE r."status" IN ('POSTED','PAID') AND r."periodStart" <= ?1 AND r."periodEnd" >= ?1
             AND l."gross" > 0"#,
    )
    .bind(&twelfth)
    .fetch_one(&mut *c)
    .await?;

    let sum = |f: &dyn Fn(&Paid) -> i64| lines.iter().map(f).sum::<i64>();
    let line2 = sum(&|p| p.income_tax_wages);
    let line3 = sum(&|p| p.fit);
    let ss_wages = sum(&|p| p.ss_wages);
    let medicare_wages = sum(&|p| p.medicare_wages);
    let additional_wages = sum(&|p| p.additional_wages());
    let l5a = at(ss_wages, 124_000)?.cents();
    let l5c = at(medicare_wages, 29_000)?.cents();
    let l5d = at(additional_wages, 9_000)?.cents();
    let l5e = l5a + l5c + l5d;
    let line6 = line3 + l5e;
    let actual = sum(&|p| p.liability_941());
    let line7 = actual - line6;
    let line10 = line6 + line7;
    let line12 = line10.max(0);
    let line13 = deposits(&mut c, &from, &to, &["FEDERAL_INCOME_TAX", "SOCIAL_SECURITY", "MEDICARE"]).await?;

    let mut months: BTreeMap<String, i64> = BTreeMap::new();
    for mo in 0..3 {
        let first = quarter * 3 - 2 + mo;
        months.insert(format!("{year}-{first:02}"), 0);
    }
    for p in &lines {
        *months.entry(p.pay_date[..7].to_string()).or_insert(0) += p.liability_941();
    }
    let runs: std::collections::BTreeSet<&str> = lines.iter().map(|p| p.run_id.as_str()).collect();

    Ok(json!({
        "year": year,
        "quarter": quarter,
        "from": from,
        "to": to,
        "employer": employer(&mut c).await?,
        "payRuns": runs.len(),
        "line1": line1,
        "line2": m(line2),
        "line3": m(line3),
        "line5a": { "wages": m(ss_wages), "tax": m(l5a) },
        "line5c": { "wages": m(medicare_wages), "tax": m(l5c) },
        "line5d": { "wages": m(additional_wages), "tax": m(l5d) },
        "line5e": m(l5e),
        "line6": m(line6),
        "line7": m(line7),
        "line10": m(line10),
        "line12": m(line12),
        "line13": m(line13),
        "line14": m((line12 - line13).max(0)),
        "line15": m((line13 - line12).max(0)),
        "line16": months.into_iter().map(|(month, c)| json!({ "month": month, "liability": m(c) })).collect::<Vec<_>>(),
        "line16Total": m(actual),
    }))
}

/// `GET /payroll/us/reports/940?year`.
pub async fn form_940(db: &Db, year: i32) -> AppResult<Value> {
    check_year(year)?;
    let (from, to) = (format!("{year}-01-01"), format!("{year}-12-31"));
    let mut c = db.read().acquire().await?;
    let lines = paid_lines(&mut c, &from, &to).await?;

    let line3: i64 = lines.iter().map(|p| p.gross).sum();
    let line4: i64 = lines.iter().map(|p| p.pretax_125).sum();
    // Line 5: each employee's FUTA-able payments over $7,000 for the year.
    let mut per_employee: BTreeMap<String, i64> = BTreeMap::new();
    for p in &lines {
        *per_employee.entry(p.employee_key()).or_insert(0) += p.gross - p.pretax_125;
    }
    let line5: i64 = per_employee.values().map(|w| (w - FUTA_WAGE_BASE).max(0)).sum();
    let line6 = line4 + line5;
    let line7 = line3 - line6;
    let line8 = at(line7, 6_000)?.cents();
    let mut line11 = 0;
    for p in &lines {
        line11 += at(p.futa_wages, p.reduction)?.cents();
    }
    let line12 = line8 + line11;
    let line13 = deposits(&mut c, &from, &to, &["FUTA"]).await?;

    // Line 16: taxable FUTA wages paid in each quarter × 0.006; credit reduction in the fourth.
    let mut quarters = [0i64; 4];
    for p in &lines {
        let month: usize = p.pay_date[5..7].parse().unwrap_or(1);
        quarters[(month - 1) / 3] += p.futa_wages;
    }
    let mut q = Vec::new();
    for (i, wages) in quarters.iter().enumerate() {
        let mut liability = at(*wages, 6_000)?.cents();
        if i == 3 {
            liability += line11;
        }
        q.push(json!({ "quarter": i + 1, "futaWages": m(*wages), "liability": m(liability) }));
    }
    let recorded: i64 = lines.iter().map(|p| p.futa).sum();

    Ok(json!({
        "year": year,
        "employer": employer(&mut c).await?,
        "line3": m(line3),
        "line4": m(line4),
        "line5": m(line5),
        "line6": m(line6),
        "line7": m(line7),
        "line8": m(line8),
        "line11": m(line11),
        "line12": m(line12),
        "line13": m(line13),
        "line14": m((line12 - line13).max(0)),
        "line15": m((line13 - line12).max(0)),
        "line16": q,
        "recordedFuta": m(recorded),
    }))
}

/// `GET /payroll/us/reports/w2?year`: one row per employee paid in the year.
pub async fn w2(db: &Db, year: i32) -> AppResult<Value> {
    check_year(year)?;
    let (from, to) = (format!("{year}-01-01"), format!("{year}-12-31"));
    let mut c = db.read().acquire().await?;
    let lines = paid_lines(&mut c, &from, &to).await?;
    let mut by: BTreeMap<String, Vec<&Paid>> = BTreeMap::new();
    for p in &lines {
        by.entry(p.employee_key()).or_default().push(p);
    }
    let mut out = Vec::new();
    for (_, ps) in by {
        let last = ps[ps.len() - 1];
        let s = |f: &dyn Fn(&Paid) -> i64| ps.iter().map(|p| f(p)).sum::<i64>();
        out.push(json!({
            "employeeId": last.employee_id,
            "employeeCode": last.code,
            "name": last.name,
            "workState": last.state,
            "paychecks": ps.len(),
            "box1": m(s(&|p| p.income_tax_wages)),
            "box2": m(s(&|p| p.fit)),
            "box3": m(s(&|p| p.ss_wages)),
            "box4": m(s(&|p| p.ss_ee)),
            "box5": m(s(&|p| p.medicare_wages)),
            "box6": m(s(&|p| p.medicare_ee + p.additional)),
            "box12D": m(s(&|p| p.pretax_401k)),
            "box15": last.state,
            "box16": m(s(&|p| p.state_wages)),
            "box17": m(s(&|p| p.state_tax)),
        }));
    }
    out.sort_by(|a, b| {
        let k = |v: &Value| v["employeeCode"].as_str().unwrap_or("").to_lowercase();
        k(a).cmp(&k(b))
    });
    let total =
        |k: &str| -> Value { m(out.iter().map(|r| Money::from_json(&r[k]).map(Money::cents).unwrap_or(0)).sum()) };
    let totals = json!({
        "box1": total("box1"), "box2": total("box2"), "box3": total("box3"), "box4": total("box4"),
        "box5": total("box5"), "box6": total("box6"), "box12D": total("box12D"),
        "box16": total("box16"), "box17": total("box17"),
    });
    Ok(json!({ "year": year, "employer": employer(&mut c).await?, "employees": out, "totals": totals }))
}
