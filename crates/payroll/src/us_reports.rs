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
//!   Boxes 15–17: each state's wages and the state income tax withheld for it (D63: one row per
//!   state in `states`; the box fields show the work state, or else the state with the most wages).
//!   D70: box 14 lists the employee's state disability / paid leave contributions (CA SDI, NY PFL,
//!   …) and `locals` boxes 18–20 (local wages, local tax, locality) per locality.
//!
//! D70, step 7 of the US state payroll plan: per state and quarter, the figures for the state's
//! withholding return (tax withheld by month of the pay date, the wages it was withheld on,
//! deposits), its unemployment wage report (each employee's gross wages, wages over the state's
//! limit, taxable wages, the employer's contributions with the state's extra charges, employee
//! unemployment contributions, employees on the payroll for the pay period including the 12th of
//! each month), the disability / paid leave programs and the local taxes of that state; and the
//! state's annual reconciliation (each quarter, and the W-2 state totals). Account numbers come
//! from the state registration.

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
    /// D63: each state's (state, wages, tax) on this line.
    states: Vec<(String, i64, i64)>,
    /// D70: the run's period, the unemployment state and figures, leave and local amounts.
    period_start: String,
    period_end: String,
    suta_state: String,
    suta_wages: i64,
    suta: i64,
    suta_rate: Option<i64>,
    suta_surcharge_rate: Option<i64>,
    leave: Vec<crate::us_leave::Contribution>,
    local: Vec<crate::us_leave::Contribution>,
    /// D83: payroll before TallyCheck gives its Additional Medicare wages itself.
    additional_wages_given: Option<i64>,
}

impl Paid {
    /// Wages in this pay over the $200,000 Additional Medicare withholding threshold.
    fn additional_wages(&self) -> i64 {
        if let Some(given) = self.additional_wages_given {
            return given;
        }
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
                  l."stateIncomeTaxWages", l."stateIncomeTax", l."stateTaxes",
                  r."periodStart", r."periodEnd",
                  COALESCE(l."sutaState", json_extract(r."settingsSnapshot", '$.state'), l."workState") AS "sutaStateUsed",
                  l."sutaWages", l."suta", l."sutaRateUsed", l."sutaSurchargeRate", l."leave", l."local"
           FROM "us_payroll_lines" l JOIN "us_payroll_runs" r ON r."id" = l."runId"
           WHERE r."status" IN ('POSTED','PAID') AND r."payDate" BETWEEN ?1 AND ?2
           ORDER BY r."payDate", r."runNumber", lower(l."employeeCode"), l."rowid""#,
    )
    .bind(from)
    .bind(to)
    .fetch_all(&mut *conn)
    .await?;
    let prior = crate::us_prior::between(conn, from, to).await?;
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
                states: {
                    let v: Value = serde_json::from_str(&r.try_get::<String, _>("stateTaxes")?).unwrap_or(Value::Null);
                    let parts: Vec<(String, i64, i64)> = v
                        .as_array()
                        .map(|a| {
                            a.iter()
                                .filter_map(|p| {
                                    Some((p["state"].as_str()?.to_string(), p["wages"].as_i64()?, p["tax"].as_i64()?))
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                    // Lines made before D63: the work state with the line's figures.
                    if parts.is_empty() {
                        vec![(r.try_get("workState")?, c("stateIncomeTaxWages")?, c("stateIncomeTax")?)]
                    } else {
                        parts
                    }
                },
                period_start: r.try_get("periodStart")?,
                period_end: r.try_get("periodEnd")?,
                suta_state: r.try_get("sutaStateUsed")?,
                suta_wages: c("sutaWages")?,
                suta: c("suta")?,
                suta_rate: r.try_get("sutaRateUsed")?,
                suta_surcharge_rate: r.try_get("sutaSurchargeRate")?,
                leave: contributions(&r.try_get::<String, _>("leave")?),
                local: contributions(&r.try_get::<String, _>("local")?),
                additional_wages_given: None,
            })
        })
        .collect::<AppResult<Vec<Paid>>>()
        .map(|mut v| {
            // D83: payroll before TallyCheck joins the lines; a stable sort keeps each day's order.
            v.extend(prior_lines(&prior));
            v.sort_by(|a, b| a.pay_date.cmp(&b.pay_date));
            v
        })
}

/// D83: payroll before TallyCheck as paid lines, so every form includes it.
fn prior_lines(rows: &[crate::us_prior::Prior]) -> Vec<Paid> {
    rows.iter()
        .map(|p| {
            let a = &p.a;
            Paid {
                run_id: format!("prior:{}", p.id),
                pay_date: p.pay_date.clone(),
                reduction: 0,
                employee_id: Some(p.employee_id.clone()),
                code: p.employee_code.clone(),
                name: p.employee_name.clone(),
                state: p.work_state.clone(),
                gross: a.gross,
                pretax_401k: a.pretax_401k,
                pretax_125: a.pretax_125,
                income_tax_wages: a.income_tax_wages,
                fit: a.federal_income_tax,
                ss_wages: a.ss_wages,
                ss_ee: a.ss_employee,
                ss_er: a.ss_employer,
                medicare_wages: a.medicare_wages,
                ytd_medicare: 0,
                medicare_ee: a.medicare_employee,
                additional: a.additional_medicare,
                medicare_er: a.medicare_employer,
                futa_wages: a.futa_wages,
                futa: a.futa,
                state_wages: a.state_wages,
                state_tax: a.state_tax,
                states: vec![(p.work_state.clone(), a.state_wages, a.state_tax)],
                period_start: p.pay_date.clone(),
                period_end: p.pay_date.clone(),
                suta_state: p.suta_state.clone(),
                suta_wages: a.suta_wages,
                suta: a.suta,
                suta_rate: None,
                suta_surcharge_rate: None,
                leave: Vec::new(),
                local: Vec::new(),
                additional_wages_given: Some(a.additional_medicare_wages),
            }
        })
        .collect()
}

fn contributions(text: &str) -> Vec<crate::us_leave::Contribution> {
    serde_json::from_str::<Value>(text)
        .ok()
        .and_then(|v| v.as_array().map(|a| a.iter().filter_map(crate::us_leave::Contribution::from_json).collect()))
        .unwrap_or_default()
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
    // D83: what the previous provider deposited for payroll before TallyCheck.
    let types: Vec<&str> = serde_json::from_str(&types).unwrap_or_default();
    Ok(total + crate::us_prior::deposited(conn, from, to, &types).await?)
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
    let runs: std::collections::BTreeSet<&str> =
        lines.iter().map(|p| p.run_id.as_str()).filter(|r| !r.starts_with("prior:")).collect();

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
        // Boxes 15–17 per state; the box fields show the state with the most wages.
        let mut by_state: BTreeMap<String, (i64, i64)> = BTreeMap::new();
        for p in &ps {
            for (st, wages, tax) in &p.states {
                let e = by_state.entry(st.clone()).or_insert((0, 0));
                e.0 += wages;
                e.1 += tax;
            }
        }
        // The work state when it has a row, else the state with the most wages.
        let main = by_state
            .get(&last.state)
            .map(|(wages, tax)| (last.state.clone(), *wages, *tax))
            .or_else(|| {
                by_state
                    .iter()
                    .max_by_key(|(_, (wages, _))| *wages)
                    .map(|(st, (wages, tax))| (st.clone(), *wages, *tax))
            })
            .unwrap_or((last.state.clone(), 0, 0));
        let states: Vec<Value> = by_state
            .iter()
            .map(|(st, (wages, tax))| json!({"box15": st, "box16": m(*wages), "box17": m(*tax)}))
            .collect();
        // D70: box 14 (state disability / paid leave the employee paid) and boxes 18-20 per locality.
        let mut box14: BTreeMap<String, i64> = BTreeMap::new();
        let mut locals: BTreeMap<String, (String, String, i64, i64)> = BTreeMap::new();
        for p in &ps {
            for c in p.leave.iter().filter(|c| c.employee > 0) {
                *box14.entry(c.name.clone()).or_insert(0) += c.employee;
            }
            for c in p.local.iter().filter(|c| c.employee > 0) {
                let e = locals.entry(c.id.clone()).or_insert((c.name.clone(), c.state.clone(), 0, 0));
                e.2 += c.wages;
                e.3 += c.employee;
            }
        }
        let box14: Vec<Value> =
            box14.iter().map(|(name, amount)| json!({"label": name, "amount": m(*amount)})).collect();
        let locals: Vec<Value> = locals
            .iter()
            .map(|(code, (name, st, wages, tax))| json!({"code": code, "state": st, "box18": m(*wages), "box19": m(*tax), "box20": name}))
            .collect();
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
            "box15": main.0,
            "box16": m(main.1),
            "box17": m(main.2),
            "states": states,
            "box14": box14,
            "locals": locals,
            "stateWagesTotal": m(s(&|p| p.state_wages)),
            "stateTaxTotal": m(s(&|p| p.state_tax)),
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
        "box16": total("stateWagesTotal"), "box17": total("stateTaxTotal"),
    });
    Ok(json!({ "year": year, "employer": employer(&mut c).await?, "employees": out, "totals": totals }))
}

/// One employee's quarter on the unemployment report: (code, name, gross, taxable, employer, employee).
type UiRow = (String, String, i64, i64, i64, i64);

/// D70: the registration's account numbers for a state (blank when not registered).
async fn state_numbers(conn: &mut SqliteConnection, state: &str) -> AppResult<Value> {
    let reg = crate::us_states::registration(conn, state).await?;
    Ok(json!({
        "withholdingAccountNo": reg.as_ref().and_then(|r| r.withholding_account_no.clone()),
        "unemploymentAccountNo": reg.as_ref().and_then(|r| r.unemployment_account_no.clone()),
        "registered": reg.is_some(),
    }))
}

/// Remittances for one state (or leave program / locality) of a type, for runs paid in the period.
async fn deposits_for(conn: &mut SqliteConnection, from: &str, to: &str, kind: &str, key: &str) -> AppResult<i64> {
    let total: i64 = sqlx::query_scalar(
        r#"SELECT COALESCE(SUM(x."amount"),0) FROM "us_payroll_remittances" x
           JOIN "us_payroll_runs" r ON r."id" = x."runId"
           WHERE r."status" IN ('POSTED','PAID') AND r."payDate" BETWEEN ?1 AND ?2 AND x."type" = ?3 AND x."state" = ?4"#,
    )
    .bind(from)
    .bind(to)
    .bind(kind)
    .bind(key)
    .fetch_one(&mut *conn)
    .await?;
    Ok(total + crate::us_prior::deposited_for(conn, from, to, kind, key).await?)
}

fn check_state(state: &str) -> AppResult<String> {
    let st = state.trim().to_ascii_uppercase();
    if !crate::us_setup::STATES.contains(&st.as_str()) {
        return Err(AppError::bad_request("state must be a two-letter US state code, like CA"));
    }
    Ok(st)
}

/// Sums of (wages, employee, employer) by id for contributions of one state.
fn by_id<'a>(
    items: impl Iterator<Item = &'a crate::us_leave::Contribution>,
    state: &str,
) -> BTreeMap<String, (String, i64, i64, i64)> {
    let mut out: BTreeMap<String, (String, i64, i64, i64)> = BTreeMap::new();
    for c in items.filter(|c| c.state == state) {
        let e = out.entry(c.id.clone()).or_insert((c.name.clone(), 0, 0, 0));
        e.1 += c.wages;
        e.2 += c.employee;
        e.3 += c.employer;
    }
    out
}

/// `GET /payroll/us/reports/state?state&year&quarter`: one state's quarter.
pub async fn state_quarter(db: &Db, state: &str, year: i32, quarter: u32) -> AppResult<Value> {
    check_year(year)?;
    let st = check_state(state)?;
    let (from, to, _) = quarter_range(year, quarter)?;
    let mut c = db.read().acquire().await?;
    let lines = paid_lines(&mut c, &from, &to).await?;

    // Withholding: tax and wages for this state by month of the pay date.
    let first_month = (quarter - 1) * 3 + 1;
    let mut months = Vec::new();
    let mut withheld_total = 0;
    let mut wages_total = 0;
    let mut employees = std::collections::BTreeSet::new();
    for mo in first_month..first_month + 3 {
        let prefix = format!("{year}-{mo:02}");
        let (mut wages, mut tax) = (0, 0);
        for p in lines.iter().filter(|p| p.pay_date.starts_with(&prefix)) {
            for (s, w, t) in p.states.iter().filter(|(s, _, _)| *s == st) {
                let _ = s;
                wages += w;
                tax += t;
                employees.insert(p.employee_key());
            }
        }
        withheld_total += tax;
        wages_total += wages;
        months.push(json!({"month": prefix, "wages": m(wages), "withheld": m(tax)}));
    }
    let withholding = json!({
        "months": months,
        "wages": m(wages_total),
        "withheld": m(withheld_total),
        "employees": employees.len(),
        "deposits": m(deposits_for(&mut c, &from, &to, "STATE_INCOME_TAX", &st).await?),
    });

    // Unemployment wage report: employees whose unemployment went to this state.
    let ui: Vec<&Paid> = lines.iter().filter(|p| p.suta_state == st).collect();
    let mut people: BTreeMap<String, UiRow> = BTreeMap::new();
    for p in &ui {
        let ee: i64 =
            p.leave.iter().filter(|c| c.state == st && crate::us_leave::is_ui(&c.id)).map(|c| c.employee).sum();
        let e = people.entry(p.employee_key()).or_insert((p.code.clone(), p.name.clone(), 0, 0, 0, 0));
        e.2 += p.gross;
        e.3 += p.suta_wages;
        e.4 += p.suta;
        e.5 += ee;
    }
    let rows: Vec<Value> = people
        .values()
        .map(|(code, name, gross, taxable, employer, employee)| {
            json!({"employeeCode": code, "name": name, "grossWages": m(*gross), "excessWages": m((gross - taxable).max(0)),
                   "taxableWages": m(*taxable), "employerContributions": m(*employer), "employeeContributions": m(*employee)})
        })
        .collect();
    // Employees paid for a pay period that includes the 12th of each month.
    let twelfths: Vec<Value> = (first_month..first_month + 3)
        .map(|mo| {
            let day = format!("{year}-{mo:02}-12");
            let n = ui
                .iter()
                .filter(|p| p.period_start.as_str() <= day.as_str() && day.as_str() <= p.period_end.as_str())
                .map(|p| p.employee_key())
                .collect::<std::collections::BTreeSet<_>>()
                .len();
            json!({"date": day, "employees": n})
        })
        .collect();
    let rates: std::collections::BTreeSet<(i64, i64)> =
        ui.iter().map(|p| (p.suta_rate.unwrap_or(0), p.suta_surcharge_rate.unwrap_or(0))).collect();
    let sum = |f: &dyn Fn(&UiRow) -> i64| people.values().map(f).sum::<i64>();
    let unemployment = json!({
        "employees": rows,
        "grossWages": m(sum(&|x| x.2)),
        "excessWages": m(sum(&|x| (x.2 - x.3).max(0))),
        "taxableWages": m(sum(&|x| x.3)),
        "employerContributions": m(sum(&|x| x.4)),
        "employeeContributions": m(sum(&|x| x.5)),
        "ratesUsed": rates.iter().map(|(r, sc)| json!({"rate": crate::us_setup::ppm_to_percent(*r), "surcharges": crate::us_setup::ppm_to_percent(*sc)})).collect::<Vec<_>>(),
        "employeesOnThe12th": twelfths,
        "deposits": m(deposits_for(&mut c, &from, &to, "SUTA", &st).await?),
    });

    // Disability / paid leave programs and local taxes of this state.
    let mut leave = Vec::new();
    for (id, (name, wages, ee, er)) in by_id(lines.iter().flat_map(|p| p.leave.iter()), &st) {
        if crate::us_leave::is_ui(&id) {
            continue;
        }
        let paid = deposits_for(&mut c, &from, &to, "STATE_LEAVE", &id).await?;
        leave.push(json!({"id": id, "name": name, "wages": m(wages), "employee": m(ee), "employer": m(er), "deposits": m(paid)}));
    }
    let mut local = Vec::new();
    for (id, (name, wages, ee, er)) in by_id(lines.iter().flat_map(|p| p.local.iter()), &st) {
        let paid = deposits_for(&mut c, &from, &to, "LOCAL_TAX", &id).await?;
        local.push(json!({"code": id, "name": name, "wages": m(wages), "employee": m(ee), "employer": m(er), "deposits": m(paid)}));
    }
    Ok(json!({
        "state": st,
        "year": year,
        "quarter": quarter,
        "from": from,
        "to": to,
        "employer": employer(&mut c).await?,
        "accountNumbers": state_numbers(&mut c, &st).await?,
        "withholding": withholding,
        "unemployment": unemployment,
        "leave": leave,
        "local": local,
    }))
}

/// `GET /payroll/us/reports/state-annual?state&year`: the state's annual reconciliation.
pub async fn state_annual(db: &Db, state: &str, year: i32) -> AppResult<Value> {
    check_year(year)?;
    let st = check_state(state)?;
    let mut quarters = Vec::new();
    let (mut withheld, mut wages, mut deposited, mut taxable, mut contributions) = (0, 0, 0, 0, 0);
    for q in 1..=4 {
        let r = state_quarter(db, &st, year, q).await?;
        let cents = |v: &Value| Money::from_json(v).map(Money::cents).unwrap_or(0);
        withheld += cents(&r["withholding"]["withheld"]);
        wages += cents(&r["withholding"]["wages"]);
        deposited += cents(&r["withholding"]["deposits"]);
        taxable += cents(&r["unemployment"]["taxableWages"]);
        contributions +=
            cents(&r["unemployment"]["employerContributions"]) + cents(&r["unemployment"]["employeeContributions"]);
        quarters.push(json!({
            "quarter": q,
            "wages": r["withholding"]["wages"],
            "withheld": r["withholding"]["withheld"],
            "deposits": r["withholding"]["deposits"],
            "unemploymentTaxableWages": r["unemployment"]["taxableWages"],
            "unemploymentContributions": m(cents(&r["unemployment"]["employerContributions"]) + cents(&r["unemployment"]["employeeContributions"])),
        }));
    }
    // W-2s with a row for this state (boxes 15-17).
    let w = w2(db, year).await?;
    let (mut forms, mut w2_wages, mut w2_tax) = (0, 0, 0);
    for e in w["employees"].as_array().into_iter().flatten() {
        for row in e["states"].as_array().into_iter().flatten().filter(|r| r["box15"] == st.as_str()) {
            forms += 1;
            w2_wages += Money::from_json(&row["box16"]).map(Money::cents).unwrap_or(0);
            w2_tax += Money::from_json(&row["box17"]).map(Money::cents).unwrap_or(0);
        }
    }
    let mut c = db.read().acquire().await?;
    Ok(json!({
        "state": st,
        "year": year,
        "employer": employer(&mut c).await?,
        "accountNumbers": state_numbers(&mut c, &st).await?,
        "quarters": quarters,
        "totals": {
            "wages": m(wages), "withheld": m(withheld), "deposits": m(deposited),
            "unemploymentTaxableWages": m(taxable), "unemploymentContributions": m(contributions),
        },
        "w2": {"forms": forms, "stateWages": m(w2_wages), "stateTax": m(w2_tax)},
        "balanced": w2_tax == withheld,
    }))
}
