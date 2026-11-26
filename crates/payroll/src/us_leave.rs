//! D67, step 4 of the US state payroll plan: state disability insurance and paid family /
//! medical leave contributions, built in from each program's own 2026 publications.
//!
//! A program belongs to the state the employee mainly works in (the unemployment state). Each
//! paycheck: program wages = gross, less 401(k) deferrals and section 125 amounts where the
//! program leaves them out; capped at the program's yearly wage limit (wages already counted on
//! the employee's other posted runs this year) or weekly limit; employee and employer shares at
//! the program's rates, half-up to the cent. Small-employer rules use the headcount the company
//! enters on its state registration (none entered = a large employer). Companies can switch a
//! program off for their employees (a private plan, an employer that pays the employee share,
//! New York disability contributions not collected) and enter their own employer rate where the
//! state assigns one (New Jersey TDI) or the insurer charges one (Hawaii TDI).
//!
//! Not built in: Oregon, Massachusetts and Rhode Island (their 2026 pages could not be read
//! directly), Maryland (contributions start in 2027), Puerto Rico. An employer paying the
//! employee's share is not worked out (switch the program off and pay it outside payroll).

use app_core::money::Money;
use app_core::{AppError, AppResult};
use serde_json::{json, Value};
use std::collections::BTreeMap;

const PPM: i128 = 1_000_000;

/// One built-in program.
#[derive(Debug, Clone, Copy)]
pub struct Program {
    pub id: &'static str,
    pub state: &'static str,
    pub name: &'static str,
    /// Applies to pay dates from this date (YYYY-MM-DD).
    pub from: &'static str,
    pub source: &'static str,
    /// Whether 401(k) deferrals / section 125 amounts count as program wages.
    pub includes_401k: bool,
    pub includes_125: bool,
}

pub const PROGRAMS: &[Program] = &[
    Program {
        id: "CA_SDI",
        state: "CA",
        name: "California SDI",
        from: "2026-01-01",
        source: "EDD DE 44 (Rev. 52, 4-26) and DE 201 (1-26): 1.3% employee, no wage limit; DE 231TP: 401(k) deferrals subject, section 125 not",
        includes_401k: true,
        includes_125: false,
    },
    Program {
        id: "CO_FAMLI",
        state: "CO",
        name: "Colorado FAMLI",
        from: "2026-01-01",
        source: "C.R.S. 8-13.3-507 and 7 CCR 1107-1: 0.88% split 50/50 to the social security wage base; 9 or fewer employees pay no employer share",
        includes_401k: true,
        includes_125: true,
    },
    Program {
        id: "CT_PL",
        state: "CT",
        name: "Connecticut Paid Leave",
        from: "2026-01-01",
        source: "CT Paid Leave Authority, Contributions and Guide for Employers: 0.5% employee to the social security wage base; FICA wages",
        includes_401k: true,
        includes_125: false,
    },
    Program {
        id: "DC_PFL",
        state: "DC",
        name: "DC Paid Family Leave",
        from: "2026-01-01",
        source: "D.C. Code § 32-541.03 and DOES Employer Information (2026): 0.75% employer only, never deducted from pay",
        includes_401k: true,
        includes_125: true,
    },
    Program {
        id: "DE_PL",
        state: "DE",
        name: "Delaware Paid Leave",
        from: "2026-01-01",
        source: "19 Del. C. §§ 3701, 3705: 0.80% (parental 0.32% only for 10–24 employees; none under 10), up to half from the employee; FICA wages to the FICA wage limit",
        includes_401k: true,
        includes_125: false,
    },
    Program {
        id: "HI_TDI",
        state: "HI",
        name: "Hawaii TDI",
        from: "2026-01-01",
        source: "DLIR 2026 Maximum Weekly Wage Base ($1,500.21) and HRS § 392-43: employee 0.5% of weekly wages up to $7.50, never more than half the insurer's premium; the employer pays the rest to its insurer",
        includes_401k: true,
        includes_125: true,
    },
    Program {
        id: "ME_PFML",
        state: "ME",
        name: "Maine Paid Family and Medical Leave",
        from: "2026-01-01",
        source: "Maine PFML Employer FAQ (July 2026): 1% (half from the employee); under 15 employees 0.5%, all from the employee; to the social security wage base",
        includes_401k: true,
        includes_125: false,
    },
    Program {
        id: "MN_PL",
        state: "MN",
        name: "Minnesota Paid Leave",
        from: "2026-01-01",
        source: "Minn. Stat. § 268B.14 and DEED: 0.88% (employee at most 0.44%; small employer designation 0.66%) to $185,000; 401(k) and section 125 included (§ 268.035 subd. 29)",
        includes_401k: true,
        includes_125: true,
    },
    Program {
        id: "NJ_FLI",
        state: "NJ",
        name: "New Jersey FLI",
        from: "2026-01-01",
        source: "NJ Department of Labor, 2026 rate information: employee 0.23% of the first $171,100 (max $393.53)",
        includes_401k: true,
        includes_125: true,
    },
    Program {
        id: "NJ_TDI",
        state: "NJ",
        name: "New Jersey TDI",
        from: "2026-01-01",
        source: "NJ Department of Labor, 2026 rate information: employee 0.19% of the first $171,100 (max $325.09); employer 0.10%–0.75% (assigned rate; 0.5% for new employers) of the first $44,800",
        includes_401k: true,
        includes_125: true,
    },
    Program {
        id: "NY_DBL",
        state: "NY",
        name: "New York disability (DBL)",
        from: "2026-01-01",
        source: "Workers' Compensation Law § 209: employee 0.5% of wages, at most $0.60 a week (collecting it is optional); the employer pays the rest of its insurer's premium",
        includes_401k: true,
        includes_125: true,
    },
    Program {
        id: "NY_PFL",
        state: "NY",
        name: "New York Paid Family Leave",
        from: "2026-01-01",
        source: "DFS decision of 29 August 2025 and paidfamilyleave.ny.gov/2026: employee 0.432% of gross wages, at most $411.91 for the year",
        includes_401k: true,
        includes_125: true,
    },
    Program {
        id: "WA_CARES",
        state: "WA",
        name: "WA Cares Fund",
        from: "2026-01-01",
        source: "ESD Employer Wage Reporting and Premiums Toolkit (June 2026): 0.58% employee, no wage limit; approved exemptions",
        includes_401k: true,
        includes_125: true,
    },
    Program {
        id: "WA_PFML",
        state: "WA",
        name: "Washington Paid Family & Medical Leave",
        from: "2026-01-01",
        source: "ESD Employer Wage Reporting and Premiums Toolkit (June 2026): 1.13% (employee 71.43%) to $184,500; under 50 employees no employer share; premium and employee share each rounded, employer = the difference",
        includes_401k: true,
        includes_125: true,
    },
];

/// The company's choices for one program on its state registration.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Options {
    pub off: bool,
    /// ppm: New Jersey TDI employer rate; Hawaii TDI insurer premium rate.
    pub employer_rate: Option<i64>,
    /// Minnesota small employer premium rate designation.
    pub small: bool,
}

impl Options {
    pub fn from_json(v: &Value) -> Options {
        Options {
            off: v["off"].as_bool().unwrap_or(false),
            employer_rate: v["employerRate"].as_i64(),
            small: v["small"].as_bool().unwrap_or(false),
        }
    }
}

/// The parameters a pay line uses for one program (kept on the line, so a posted run never
/// changes when the built-in figures do).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub id: String,
    pub state: String,
    pub name: String,
    /// "RATE", or "WA_PFML" (total premium rounded, employee share rounded, employer = rest).
    pub method: String,
    /// ppm.
    pub employee_rate: i64,
    pub employer_rate: i64,
    /// For WA_PFML: the total rate (ppm) and the employee's part of it (ppm of the total).
    pub total_rate: i64,
    pub employee_part: i64,
    /// Yearly wage limits (cents) for the employee and the employer share; None = no limit.
    pub wage_cap: Option<i64>,
    pub employer_wage_cap: Option<i64>,
    /// Yearly maximum employee contribution (cents).
    pub employee_max: Option<i64>,
    /// Weekly wage limit (cents) and weekly maximum employee contribution (cents).
    pub weekly_wage_cap: Option<i64>,
    pub weekly_employee_max: Option<i64>,
    /// Hawaii: the employee pays at most half the insurer's premium (ppm; None = not entered).
    pub half_of_premium: Option<i64>,
    pub includes_401k: bool,
    pub includes_125: bool,
}

impl Plan {
    pub fn to_json(&self) -> Value {
        json!({
            "id": self.id, "state": self.state, "name": self.name, "method": self.method,
            "employeeRate": self.employee_rate, "employerRate": self.employer_rate,
            "totalRate": self.total_rate, "employeePart": self.employee_part,
            "wageCap": self.wage_cap, "employerWageCap": self.employer_wage_cap,
            "employeeMax": self.employee_max, "weeklyWageCap": self.weekly_wage_cap,
            "weeklyEmployeeMax": self.weekly_employee_max, "halfOfPremium": self.half_of_premium,
            "includes401k": self.includes_401k, "includes125": self.includes_125,
        })
    }

    pub fn from_json(v: &Value) -> Option<Plan> {
        Some(Plan {
            id: v["id"].as_str()?.to_string(),
            state: v["state"].as_str()?.to_string(),
            name: v["name"].as_str().unwrap_or_default().to_string(),
            method: v["method"].as_str().unwrap_or("RATE").to_string(),
            employee_rate: v["employeeRate"].as_i64().unwrap_or(0),
            employer_rate: v["employerRate"].as_i64().unwrap_or(0),
            total_rate: v["totalRate"].as_i64().unwrap_or(0),
            employee_part: v["employeePart"].as_i64().unwrap_or(0),
            wage_cap: v["wageCap"].as_i64(),
            employer_wage_cap: v["employerWageCap"].as_i64(),
            employee_max: v["employeeMax"].as_i64(),
            weekly_wage_cap: v["weeklyWageCap"].as_i64(),
            weekly_employee_max: v["weeklyEmployeeMax"].as_i64(),
            half_of_premium: v["halfOfPremium"].as_i64(),
            includes_401k: v["includes401k"].as_bool().unwrap_or(true),
            includes_125: v["includes125"].as_bool().unwrap_or(true),
        })
    }
}

/// The 2026 social security wage base, the limit several programs use.
const SS_BASE: i64 = 18_450_000;

/// The plan for one program, given the company's headcount in the state (None = not given,
/// treated as a large employer) and its options. None when the program asks nothing of this
/// employer (Delaware under 10 employees).
pub fn plan(p: &Program, employees: Option<i64>, o: &Options) -> Option<Plan> {
    let small = |under: i64| employees.is_some_and(|n| n < under);
    let base = |employee_rate: i64, employer_rate: i64, wage_cap: Option<i64>| Plan {
        id: p.id.into(),
        state: p.state.into(),
        name: p.name.into(),
        method: "RATE".into(),
        employee_rate,
        employer_rate,
        total_rate: 0,
        employee_part: 0,
        wage_cap,
        employer_wage_cap: wage_cap,
        employee_max: None,
        weekly_wage_cap: None,
        weekly_employee_max: None,
        half_of_premium: None,
        includes_401k: p.includes_401k,
        includes_125: p.includes_125,
    };
    let out = match p.id {
        "CA_SDI" => base(13_000, 0, None),
        "CO_FAMLI" => base(4_400, if small(10) { 0 } else { 4_400 }, Some(SS_BASE)),
        "CT_PL" => base(5_000, 0, Some(SS_BASE)),
        "DC_PFL" => base(0, 7_500, None),
        "DE_PL" if small(10) => return None,
        "DE_PL" if small(25) => base(1_600, 1_600, Some(SS_BASE)),
        "DE_PL" => base(4_000, 4_000, Some(SS_BASE)),
        "HI_TDI" => Plan {
            weekly_wage_cap: Some(150_021),
            half_of_premium: o.employer_rate.map(|r| r / 2),
            ..base(5_000, 0, None)
        },
        "ME_PFML" => base(5_000, if small(15) { 0 } else { 5_000 }, Some(SS_BASE)),
        "MN_PL" => base(4_400, if o.small { 2_200 } else { 4_400 }, Some(18_500_000)),
        "NJ_FLI" => base(2_300, 0, Some(17_110_000)),
        "NJ_TDI" => Plan {
            employer_wage_cap: Some(4_480_000),
            ..base(1_900, o.employer_rate.unwrap_or(5_000), Some(17_110_000))
        },
        "NY_DBL" => Plan { weekly_employee_max: Some(60), ..base(5_000, 0, None) },
        "NY_PFL" => Plan { employee_max: Some(41_191), ..base(4_320, 0, None) },
        "WA_CARES" => base(5_800, 0, None),
        "WA_PFML" => Plan {
            method: "WA_PFML".into(),
            total_rate: 11_300,
            employee_part: 714_300,
            employer_rate: if small(50) { 0 } else { 1 },
            ..base(0, 0, Some(SS_BASE))
        },
        _ => return None,
    };
    Some(out)
}

/// The programs in force in `state` on `date` (YYYY-MM-DD).
pub fn programs_in(state: &str, date: &str) -> Vec<&'static Program> {
    PROGRAMS.iter().filter(|p| p.state == state && p.from <= date).collect()
}

/// What the employee's other posted runs this year already counted for a program.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Ytd {
    pub wages: i64,
    pub employer_wages: i64,
    pub employee: i64,
}

/// One program's contribution on a paycheck (cents).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Contribution {
    pub id: String,
    pub state: String,
    pub name: String,
    pub wages: i64,
    pub employer_wages: i64,
    pub employee: i64,
    pub employer: i64,
}

impl Contribution {
    pub fn to_json(&self) -> Value {
        json!({"id": self.id, "state": self.state, "name": self.name, "wages": self.wages,
               "employerWages": self.employer_wages, "employee": self.employee, "employer": self.employer})
    }

    pub fn from_json(v: &Value) -> Option<Contribution> {
        Some(Contribution {
            id: v["id"].as_str()?.to_string(),
            state: v["state"].as_str().unwrap_or_default().to_string(),
            name: v["name"].as_str().unwrap_or_default().to_string(),
            wages: v["wages"].as_i64().unwrap_or(0),
            employer_wages: v["employerWages"].as_i64().unwrap_or(0),
            employee: v["employee"].as_i64().unwrap_or(0),
            employer: v["employer"].as_i64().unwrap_or(0),
        })
    }
}

fn at(wages: i64, rate_ppm: i64) -> i64 {
    let n = i128::from(wages.max(0)) * i128::from(rate_ppm.max(0));
    i64::try_from((2 * n + PPM) / (2 * PPM)).unwrap_or(i64::MAX)
}

/// A weekly amount for one pay period of `periods` a year (52 weeks a year), half-up.
fn per_period(weekly: i64, periods: i64) -> i64 {
    let n = i128::from(weekly) * 52;
    let d = i128::from(periods.max(1));
    i64::try_from((2 * n + d) / (2 * d)).unwrap_or(i64::MAX)
}

/// One program's contribution on a paycheck.
pub fn contribution(
    p: &Plan,
    gross: Money,
    pretax_401k: Money,
    pretax_125: Money,
    periods: i64,
    ytd: Ytd,
) -> AppResult<Contribution> {
    let mut wages = gross.cents();
    if !p.includes_401k {
        wages -= pretax_401k.cents();
    }
    if !p.includes_125 {
        wages -= pretax_125.cents();
    }
    let wages = wages.max(0);
    let capped = |cap: Option<i64>, used: i64| cap.map_or(wages, |c| wages.min((c - used).max(0)));
    let mut employee_wages = capped(p.wage_cap, ytd.wages);
    let employer_wages = capped(p.employer_wage_cap, ytd.employer_wages);
    if let Some(weekly) = p.weekly_wage_cap {
        employee_wages = employee_wages.min(per_period(weekly, periods));
    }
    let (mut employee, employer) = if p.method == "WA_PFML" {
        let total = at(employee_wages, p.total_rate);
        // Employee share = wages × total rate × employee part, rounded once.
        let n = i128::from(employee_wages) * i128::from(p.total_rate) * i128::from(p.employee_part);
        let d = PPM * PPM;
        let employee =
            i64::try_from((2 * n + d) / (2 * d)).map_err(|_| AppError::bad_request("Amount is too large"))?;
        let employer = if p.employer_rate > 0 { total - employee } else { 0 };
        (employee, employer)
    } else {
        (at(employee_wages, p.employee_rate), at(employer_wages, p.employer_rate))
    };
    if let Some(half) = p.half_of_premium {
        employee = employee.min(at(wages, half));
    }
    if let Some(weekly) = p.weekly_employee_max {
        employee = employee.min(per_period(weekly, periods));
    }
    if let Some(max) = p.employee_max {
        employee = employee.min((max - ytd.employee).max(0));
    }
    Ok(Contribution {
        id: p.id.clone(),
        state: p.state.clone(),
        name: p.name.clone(),
        wages: employee_wages,
        employer_wages: if employer > 0 { employer_wages } else { 0 },
        employee,
        employer,
    })
}

/// Every program on a paycheck, skipping those with nothing to pay.
pub fn contributions(
    plans: &[Plan],
    gross: Money,
    pretax_401k: Money,
    pretax_125: Money,
    periods: i64,
    ytd: &BTreeMap<String, Ytd>,
) -> AppResult<Vec<Contribution>> {
    let mut out = Vec::new();
    for p in plans {
        let c = contribution(p, gross, pretax_401k, pretax_125, periods, ytd.get(&p.id).copied().unwrap_or_default())?;
        if c.employee > 0 || c.employer > 0 {
            out.push(c);
        }
    }
    Ok(out)
}

/// The programs and their sources, for the settings screen.
pub fn catalog() -> Value {
    Value::Array(
        PROGRAMS
            .iter()
            .map(|p| json!({"id": p.id, "state": p.state, "name": p.name, "from": p.from, "source": p.source}))
            .collect(),
    )
}

pub fn is_program(id: &str) -> bool {
    PROGRAMS.iter().any(|p| p.id == id)
}

/// The plans for someone working mainly in `state` on `date`, with the company's headcount and
/// options from its registration there, less the programs the employee is exempt from.
pub async fn plans_for(
    conn: &mut sqlx::SqliteConnection,
    state: &str,
    date: &str,
    exempt: &[String],
) -> AppResult<Vec<Plan>> {
    let reg = crate::us_states::registration(conn, state).await?;
    let employees = reg.as_ref().and_then(|r| r.employees);
    let options = reg.as_ref().map(|r| r.leave_options.clone()).unwrap_or_else(|| json!({}));
    Ok(programs_in(state, date)
        .into_iter()
        .filter(|p| !exempt.iter().any(|x| x == p.id))
        .filter_map(|p| {
            let o = Options::from_json(&options[p.id]);
            if o.off {
                None
            } else {
                plan(p, employees, &o)
            }
        })
        .collect())
}

/// What the employee's other posted / paid runs paid in `year` counted for each program.
pub async fn ytd(
    conn: &mut sqlx::SqliteConnection,
    employee_id: &str,
    year: i32,
    except_run: &str,
) -> AppResult<BTreeMap<String, Ytd>> {
    let rows: Vec<(String, i64, i64, i64)> = sqlx::query_as(
        r#"SELECT json_extract(j."value", '$.id'), COALESCE(SUM(json_extract(j."value", '$.wages')),0),
                  COALESCE(SUM(json_extract(j."value", '$.employerWages')),0),
                  COALESCE(SUM(json_extract(j."value", '$.employee')),0)
           FROM "us_payroll_lines" l JOIN "us_payroll_runs" r ON r."id" = l."runId", json_each(l."leave") j
           WHERE l."employeeId" = ?1 AND r."status" IN ('POSTED','PAID') AND r."id" <> ?2
             AND CAST(substr(r."payDate", 1, 4) AS INTEGER) = ?3
           GROUP BY json_extract(j."value", '$.id')"#,
    )
    .bind(employee_id)
    .bind(except_run)
    .bind(year)
    .fetch_all(&mut *conn)
    .await?;
    Ok(rows
        .into_iter()
        .map(|(id, wages, employer_wages, employee)| (id, Ytd { wages, employer_wages, employee }))
        .collect())
}

pub fn ytd_to_json(y: &BTreeMap<String, Ytd>) -> Value {
    Value::Object(
        y.iter()
            .map(|(k, v)| {
                (k.clone(), json!({"wages": v.wages, "employerWages": v.employer_wages, "employee": v.employee}))
            })
            .collect(),
    )
}

pub fn ytd_from_json(v: &Value) -> BTreeMap<String, Ytd> {
    v.as_object()
        .map(|o| {
            o.iter()
                .map(|(k, x)| {
                    (
                        k.clone(),
                        Ytd {
                            wages: x["wages"].as_i64().unwrap_or(0),
                            employer_wages: x["employerWages"].as_i64().unwrap_or(0),
                            employee: x["employee"].as_i64().unwrap_or(0),
                        },
                    )
                })
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn usd(s: &str) -> Money {
        Money::parse_decimal(s).unwrap_or_else(|| panic!("{s}"))
    }

    fn pay(id: &str, gross: &str, periods: i64, employees: Option<i64>, o: Options, ytd: Ytd) -> (i64, i64) {
        let p = PROGRAMS.iter().find(|p| p.id == id).expect("program");
        let plan = plan(p, employees, &o).expect("plan");
        let c = contribution(&plan, usd(gross), usd("100"), usd("50"), periods, ytd).expect("contribution");
        (c.employee, c.employer)
    }

    #[test]
    fn rates_and_limits() {
        let none = Options::default;
        let y = Ytd::default();
        // California: 1.3% of gross less section 125 (401(k) counts): (2,000 − 50) × 1.3% = 25.35.
        assert_eq!(pay("CA_SDI", "2000", 26, None, none(), y), (2_535, 0));
        // Washington, $3,000: premium 33.90, employee 24.21, employer 9.69; under 50 employees no
        // employer share.
        assert_eq!(pay("WA_PFML", "3000", 26, Some(50), none(), y), (2_421, 969));
        assert_eq!(pay("WA_PFML", "3000", 26, Some(49), none(), y), (2_421, 0));
        assert_eq!(pay("WA_CARES", "3000", 26, None, none(), y), (1_740, 0));
        // Colorado 0.44% + 0.44%; 9 employees: employee share only.
        assert_eq!(pay("CO_FAMLI", "3000", 26, None, none(), y), (1_320, 1_320));
        assert_eq!(pay("CO_FAMLI", "3000", 26, Some(9), none(), y), (1_320, 0));
        // Connecticut: $1,000 a week → $5 (official example); section 125 left out here.
        assert_eq!(pay("CT_PL", "1050", 52, None, none(), y), (500, 0));
        // Delaware: 25+ 0.4% + 0.4%; 10–24 parental only 0.16% + 0.16%; under 10 nothing.
        assert_eq!(pay("DE_PL", "3050", 26, None, none(), y), (1_200, 1_200));
        assert_eq!(pay("DE_PL", "3050", 26, Some(12), none(), y), (480, 480));
        let de = PROGRAMS.iter().find(|p| p.id == "DE_PL").expect("DE");
        assert!(plan(de, Some(9), &none()).is_none());
        // Minnesota: 0.44% + 0.44%, small employer designation 0.22%.
        assert_eq!(pay("MN_PL", "3000", 26, None, none(), y), (1_320, 1_320));
        assert_eq!(pay("MN_PL", "3000", 26, None, Options { small: true, ..none() }, y), (1_320, 660));
        // Maine: 0.5% + 0.5%; under 15 employees 0.5% employee only.
        assert_eq!(pay("ME_PFML", "3050", 26, None, none(), y), (1_500, 1_500));
        assert_eq!(pay("ME_PFML", "3050", 26, Some(14), none(), y), (1_500, 0));
        // DC: employer 0.75%.
        assert_eq!(pay("DC_PFL", "3000", 26, None, none(), y), (0, 2_250));
        // New Jersey $3,000: TDI 5.70 + employer 0.5% 15.00; FLI 6.90.
        assert_eq!(pay("NJ_TDI", "3000", 26, None, none(), y), (570, 1_500));
        assert_eq!(pay("NJ_FLI", "3000", 26, None, none(), y), (690, 0));
        assert_eq!(pay("NJ_TDI", "3000", 26, None, Options { employer_rate: Some(1_000), ..none() }, y), (570, 300));
        // New York PFL 0.432%: $1,000 a week → 4.32; stops at $411.91 for the year.
        assert_eq!(pay("NY_PFL", "1000", 52, None, none(), y), (432, 0));
        assert_eq!(pay("NY_PFL", "1000", 52, None, none(), Ytd { employee: 41_000, ..y }), (191, 0));
        // New York DBL: 0.5%, at most $0.60 a week ($1.20 biweekly); $100 a week → 0.50.
        assert_eq!(pay("NY_DBL", "1000", 52, None, none(), y), (60, 0));
        assert_eq!(pay("NY_DBL", "2000", 26, None, none(), y), (120, 0));
        assert_eq!(pay("NY_DBL", "100", 52, None, none(), y), (50, 0));
        // Hawaii: 0.5% of weekly wages up to $1,500.21 → $7.50; at most half the insurer's premium
        // (0.8% → 0.4%: $300 a week → 1.20, the FAQ's example).
        assert_eq!(pay("HI_TDI", "2000", 52, None, none(), y), (750, 0));
        assert_eq!(pay("HI_TDI", "300", 52, None, Options { employer_rate: Some(8_000), ..none() }, y), (120, 0));
    }

    #[test]
    fn yearly_wage_limits() {
        let none = Options::default;
        // New Jersey TDI: employee wages stop at $171,100, the employer's at $44,800.
        let ytd = Ytd { wages: 17_000_000, employer_wages: 4_400_000, employee: 0 };
        assert_eq!(pay("NJ_TDI", "3000", 26, None, none(), ytd), (209, 400));
        // Washington: nothing above $184,500.
        let full = Ytd { wages: SS_BASE, employer_wages: SS_BASE, employee: 0 };
        assert_eq!(pay("WA_PFML", "3000", 26, None, none(), full), (0, 0));
        // California has no limit.
        assert_eq!(pay("CA_SDI", "2000", 26, None, none(), full), (2_535, 0));
    }

    #[test]
    fn plans_round_trip() {
        for p in PROGRAMS {
            if let Some(plan) = plan(p, None, &Options::default()) {
                assert_eq!(Plan::from_json(&plan.to_json()).as_ref(), Some(&plan), "{}", p.id);
            }
            assert!(crate::us_setup::STATES.contains(&p.state) || p.state == "DC", "{}", p.state);
        }
    }
}
