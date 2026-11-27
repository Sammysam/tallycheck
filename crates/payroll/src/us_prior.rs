//! D83: US payroll paid before the company started using TallyCheck in the same year, by another
//! provider or by hand. Each row is one employee's pay (or a month's or quarter's total) on a pay
//! date, with the figures the old provider's register shows. Nothing is posted to the ledger (that
//! money is already in the opening balances). The rows count towards the year-to-date wage limits
//! of every later pay run (social security, FUTA, state unemployment, Additional Medicare) and are
//! included in Forms 941, 940, W-2 and the state forms; their taxes count as deposited by the
//! previous provider. A row must be dated before the employee's first posted TallyCheck pay run of
//! that year, so nothing is counted twice.

use app_core::ids::new_id;
use app_core::money::Money;
use app_core::time::now_iso;
use app_core::{AppError, AppResult};
use database::Db;
use serde_json::{json, Value};
use sqlx::sqlite::SqliteRow;
use sqlx::{Row, SqliteConnection};

const SS_WAGE_BASE_2026: i64 = 18_450_000;
const FUTA_WAGE_BASE: i64 = 700_000;

/// The figures of one row, in cents.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Amounts {
    pub gross: i64,
    pub pretax_401k: i64,
    pub pretax_125: i64,
    pub income_tax_wages: i64,
    pub federal_income_tax: i64,
    pub ss_wages: i64,
    pub ss_employee: i64,
    pub ss_employer: i64,
    pub medicare_wages: i64,
    pub medicare_employee: i64,
    pub medicare_employer: i64,
    pub additional_medicare_wages: i64,
    pub additional_medicare: i64,
    pub futa_wages: i64,
    pub futa: i64,
    pub suta_wages: i64,
    pub suta: i64,
    pub state_wages: i64,
    pub state_tax: i64,
}

#[derive(Debug, Clone)]
pub struct Prior {
    pub id: String,
    pub employee_id: String,
    pub employee_code: String,
    pub employee_name: String,
    pub pay_date: String,
    pub work_state: String,
    pub suta_state: String,
    pub a: Amounts,
    pub notes: Option<String>,
}

/// What a request gives; any figure left out is worked out (see `fill`).
#[derive(Debug, Clone, Default)]
pub struct PriorInput {
    pub employee_id: String,
    pub pay_date: String,
    pub work_state: Option<String>,
    pub suta_state: Option<String>,
    pub gross: Money,
    pub pretax_401k: Option<Money>,
    pub pretax_125: Option<Money>,
    pub income_tax_wages: Option<Money>,
    pub federal_income_tax: Option<Money>,
    pub ss_wages: Option<Money>,
    pub ss_employee: Option<Money>,
    pub ss_employer: Option<Money>,
    pub medicare_wages: Option<Money>,
    pub medicare_employee: Option<Money>,
    pub medicare_employer: Option<Money>,
    pub additional_medicare_wages: Option<Money>,
    pub additional_medicare: Option<Money>,
    pub futa_wages: Option<Money>,
    pub futa: Option<Money>,
    pub suta_wages: Option<Money>,
    pub suta: Option<Money>,
    pub state_wages: Option<Money>,
    pub state_tax: Option<Money>,
    pub notes: Option<String>,
}

const COLUMNS: &str = r#"p."id", p."employeeId", e."employeeId" AS "employeeCode", e."name" AS "employeeName",
  p."payDate", p."workState", p."sutaState", p."gross", p."pretax401k", p."pretaxSection125", p."incomeTaxWages",
  p."federalIncomeTax", p."socialSecurityWages", p."socialSecurityEmployee", p."socialSecurityEmployer",
  p."medicareWages", p."medicareEmployee", p."medicareEmployer", p."additionalMedicareWages",
  p."additionalMedicare", p."futaWages", p."futa", p."sutaWages", p."suta", p."stateIncomeTaxWages",
  p."stateIncomeTax", p."notes""#;

fn from_row(r: &SqliteRow) -> AppResult<Prior> {
    let c = |k: &str| r.try_get::<i64, _>(k);
    Ok(Prior {
        id: r.try_get("id")?,
        employee_id: r.try_get("employeeId")?,
        employee_code: r.try_get("employeeCode")?,
        employee_name: r.try_get("employeeName")?,
        pay_date: r.try_get("payDate")?,
        work_state: r.try_get("workState")?,
        suta_state: r.try_get("sutaState")?,
        a: Amounts {
            gross: c("gross")?,
            pretax_401k: c("pretax401k")?,
            pretax_125: c("pretaxSection125")?,
            income_tax_wages: c("incomeTaxWages")?,
            federal_income_tax: c("federalIncomeTax")?,
            ss_wages: c("socialSecurityWages")?,
            ss_employee: c("socialSecurityEmployee")?,
            ss_employer: c("socialSecurityEmployer")?,
            medicare_wages: c("medicareWages")?,
            medicare_employee: c("medicareEmployee")?,
            medicare_employer: c("medicareEmployer")?,
            additional_medicare_wages: c("additionalMedicareWages")?,
            additional_medicare: c("additionalMedicare")?,
            futa_wages: c("futaWages")?,
            futa: c("futa")?,
            suta_wages: c("sutaWages")?,
            suta: c("suta")?,
            state_wages: c("stateIncomeTaxWages")?,
            state_tax: c("stateIncomeTax")?,
        },
        notes: r.try_get("notes")?,
    })
}

fn m(c: i64) -> Value {
    Money::from_cents(c).to_json_number()
}

impl Prior {
    pub fn to_json(&self) -> Value {
        let a = &self.a;
        json!({
            "id": self.id,
            "employeeId": self.employee_id,
            "employeeCode": self.employee_code,
            "employeeName": self.employee_name,
            "payDate": self.pay_date,
            "workState": self.work_state,
            "sutaState": self.suta_state,
            "gross": m(a.gross),
            "pretax401k": m(a.pretax_401k),
            "pretaxSection125": m(a.pretax_125),
            "incomeTaxWages": m(a.income_tax_wages),
            "federalIncomeTax": m(a.federal_income_tax),
            "socialSecurityWages": m(a.ss_wages),
            "socialSecurityEmployee": m(a.ss_employee),
            "socialSecurityEmployer": m(a.ss_employer),
            "medicareWages": m(a.medicare_wages),
            "medicareEmployee": m(a.medicare_employee),
            "medicareEmployer": m(a.medicare_employer),
            "additionalMedicareWages": m(a.additional_medicare_wages),
            "additionalMedicare": m(a.additional_medicare),
            "futaWages": m(a.futa_wages),
            "futa": m(a.futa),
            "sutaWages": m(a.suta_wages),
            "suta": m(a.suta),
            "stateIncomeTaxWages": m(a.state_wages),
            "stateIncomeTax": m(a.state_tax),
            "notes": self.notes,
        })
    }
}

fn year_of(date: &str) -> AppResult<i32> {
    date.get(..4).and_then(|y| y.parse().ok()).ok_or_else(|| AppError::bad_request("payDate must be YYYY-MM-DD"))
}

/// Every row of `year` (all employees), by pay date.
pub async fn list(conn: &mut SqliteConnection, year: i32) -> AppResult<Vec<Prior>> {
    let sql = format!(
        r#"SELECT {COLUMNS} FROM "us_payroll_prior" p JOIN "employees" e ON e."id" = p."employeeId"
           WHERE CAST(substr(p."payDate", 1, 4) AS INTEGER) = ?1
           ORDER BY p."payDate", lower(e."employeeId"), p."createdAt""#
    );
    let rows = sqlx::query(sqlx::AssertSqlSafe(&*sql)).bind(year).fetch_all(&mut *conn).await?;
    rows.iter().map(from_row).collect()
}

/// Rows paid from `from` to `to` (inclusive), for the forms.
pub async fn between(conn: &mut SqliteConnection, from: &str, to: &str) -> AppResult<Vec<Prior>> {
    let sql = format!(
        r#"SELECT {COLUMNS} FROM "us_payroll_prior" p JOIN "employees" e ON e."id" = p."employeeId"
           WHERE p."payDate" BETWEEN ?1 AND ?2 ORDER BY p."payDate", lower(e."employeeId")"#
    );
    let rows = sqlx::query(sqlx::AssertSqlSafe(&*sql)).bind(from).bind(to).fetch_all(&mut *conn).await?;
    rows.iter().map(from_row).collect()
}

async fn find_one(conn: &mut SqliteConnection, id: &str) -> AppResult<Prior> {
    let sql = format!(
        r#"SELECT {COLUMNS} FROM "us_payroll_prior" p JOIN "employees" e ON e."id" = p."employeeId" WHERE p."id" = ?1"#
    );
    let row = sqlx::query(sqlx::AssertSqlSafe(&*sql)).bind(id).fetch_optional(&mut *conn).await?;
    row.as_ref().map(from_row).transpose()?.ok_or_else(|| AppError::NotFound(format!("Prior payroll {id} not found")))
}

/// Wages already counted in `year` before TallyCheck: (social security, Medicare, FUTA, state
/// unemployment in `suta_state` — None = every state).
pub async fn wages_in_year(
    conn: &mut SqliteConnection,
    employee_id: &str,
    year: i32,
    suta_state: Option<&str>,
    except: Option<&str>,
) -> AppResult<(i64, i64, i64, i64)> {
    let row: (i64, i64, i64, i64) = sqlx::query_as(
        r#"SELECT COALESCE(SUM("socialSecurityWages"),0), COALESCE(SUM("medicareWages"),0),
                  COALESCE(SUM("futaWages"),0),
                  COALESCE(SUM(CASE WHEN ?3 IS NULL OR "sutaState" = ?3 THEN "sutaWages" ELSE 0 END),0)
           FROM "us_payroll_prior"
           WHERE "employeeId" = ?1 AND CAST(substr("payDate", 1, 4) AS INTEGER) = ?2 AND "id" <> COALESCE(?4, '')"#,
    )
    .bind(employee_id)
    .bind(year)
    .bind(suta_state)
    .bind(except)
    .fetch_one(&mut *conn)
    .await?;
    Ok(row)
}

/// Taxes the previous provider deposited, for Form 941 / 940 line 13: by remittance type.
pub async fn deposited(conn: &mut SqliteConnection, from: &str, to: &str, types: &[&str]) -> AppResult<i64> {
    let rows = between(conn, from, to).await?;
    let mut total = 0;
    for p in &rows {
        let a = &p.a;
        for t in types {
            total += match *t {
                "FEDERAL_INCOME_TAX" => a.federal_income_tax,
                "SOCIAL_SECURITY" => a.ss_employee + a.ss_employer,
                "MEDICARE" => a.medicare_employee + a.additional_medicare + a.medicare_employer,
                "FUTA" => a.futa,
                _ => 0,
            };
        }
    }
    Ok(total)
}

/// The same for one state: STATE_INCOME_TAX by work state, SUTA by unemployment state.
pub async fn deposited_for(
    conn: &mut SqliteConnection,
    from: &str,
    to: &str,
    kind: &str,
    state: &str,
) -> AppResult<i64> {
    let rows = between(conn, from, to).await?;
    Ok(rows
        .iter()
        .map(|p| match kind {
            "STATE_INCOME_TAX" if p.work_state == state => p.a.state_tax,
            "SUTA" if p.suta_state == state => p.a.suta,
            _ => 0,
        })
        .sum())
}

fn check_state(s: &str, what: &str) -> AppResult<String> {
    let st = s.trim().to_ascii_uppercase();
    if !crate::us_setup::STATES.contains(&st.as_str()) {
        return Err(AppError::bad_request(format!("{what} must be a two-letter US state code, like CA")));
    }
    Ok(st)
}

/// Works out a row: states, and every figure left out from the gross (and the year's limits).
async fn fill(
    conn: &mut SqliteConnection,
    input: &PriorInput,
    except: Option<&str>,
) -> AppResult<(String, String, Amounts)> {
    let date = chrono::NaiveDate::parse_from_str(&input.pay_date, "%Y-%m-%d")
        .map_err(|_| AppError::bad_request("payDate must be a date (YYYY-MM-DD)"))?;
    let year = year_of(&input.pay_date)?;
    if !(2000..=2100).contains(&year) {
        return Err(AppError::bad_request("payDate must be between 2000 and 2100"));
    }
    if date > app_core::time::today_local() {
        return Err(AppError::bad_request("Payroll before TallyCheck can't be dated in the future"));
    }
    let exists: Option<String> = sqlx::query_scalar(r#"SELECT "id" FROM "employees" WHERE "id" = ?1"#)
        .bind(&input.employee_id)
        .fetch_optional(&mut *conn)
        .await?;
    if exists.is_none() {
        return Err(AppError::NotFound(format!("Employee {} not found", input.employee_id)));
    }
    // Never on or after a pay run TallyCheck already made for the employee that year.
    let first: Option<String> = sqlx::query_scalar(
        r#"SELECT MIN(r."payDate") FROM "us_payroll_lines" l JOIN "us_payroll_runs" r ON r."id" = l."runId"
           WHERE l."employeeId" = ?1 AND r."status" IN ('POSTED','PAID') AND CAST(substr(r."payDate", 1, 4) AS INTEGER) = ?2"#,
    )
    .bind(&input.employee_id)
    .bind(year)
    .fetch_one(&mut *conn)
    .await?;
    if let Some(first) = first {
        if input.pay_date >= first {
            return Err(AppError::bad_request(format!(
                "Payroll before TallyCheck must be dated before this employee's first pay run in TallyCheck ({first})"
            )));
        }
    }

    let settings = crate::us_setup::load_settings(conn).await?;
    let tax_state = crate::us_setup::load_tax(conn, &input.employee_id).await?.map(|(t, _)| t.work_state);
    let work_state = match input.work_state.as_deref().filter(|s| !s.trim().is_empty()) {
        Some(s) => check_state(s, "workState")?,
        None => tax_state
            .filter(|s| !s.is_empty())
            .or_else(|| settings.state.clone())
            .ok_or_else(|| AppError::bad_request("Give the work state (the employee has no US tax details yet)"))?,
    };
    let (default_suta, _, wage_base) = crate::us_states::unemployment_for(conn, &settings, &work_state).await?;
    let suta_state = match input.suta_state.as_deref().filter(|s| !s.trim().is_empty()) {
        Some(s) => check_state(s, "sutaState")?,
        None => default_suta,
    };

    let c = |v: Option<Money>| v.map(Money::cents);
    let gross = input.gross.cents();
    let k401 = c(input.pretax_401k).unwrap_or(0);
    let s125 = c(input.pretax_125).unwrap_or(0);
    if k401 + s125 > gross {
        return Err(AppError::bad_request("Pre-tax deductions can't be more than the gross pay"));
    }
    let fica_base = gross - s125;
    let (ss_before, _, futa_before, suta_before) =
        wages_in_year(conn, &input.employee_id, year, Some(&suta_state), except).await?;
    let ss_wages = c(input.ss_wages).unwrap_or_else(|| fica_base.min((SS_WAGE_BASE_2026 - ss_before).max(0)));
    let medicare_wages = c(input.medicare_wages).unwrap_or(fica_base);
    let ss_employee = c(input.ss_employee).unwrap_or(0);
    let medicare_employee = c(input.medicare_employee).unwrap_or(0);
    let a = Amounts {
        gross,
        pretax_401k: k401,
        pretax_125: s125,
        income_tax_wages: c(input.income_tax_wages).unwrap_or(gross - k401 - s125),
        federal_income_tax: c(input.federal_income_tax).unwrap_or(0),
        ss_wages,
        ss_employee,
        ss_employer: c(input.ss_employer).unwrap_or(ss_employee),
        medicare_wages,
        medicare_employee,
        medicare_employer: c(input.medicare_employer).unwrap_or(medicare_employee),
        additional_medicare_wages: c(input.additional_medicare_wages).unwrap_or(0),
        additional_medicare: c(input.additional_medicare).unwrap_or(0),
        futa_wages: c(input.futa_wages).unwrap_or_else(|| fica_base.min((FUTA_WAGE_BASE - futa_before).max(0))),
        futa: c(input.futa).unwrap_or(0),
        suta_wages: c(input.suta_wages).unwrap_or_else(|| match wage_base {
            Some(base) => fica_base.min((base.cents() - suta_before).max(0)),
            None => fica_base,
        }),
        suta: c(input.suta).unwrap_or(0),
        state_wages: c(input.state_wages).unwrap_or(gross - k401 - s125),
        state_tax: c(input.state_tax).unwrap_or(0),
    };
    let wages = [a.income_tax_wages, a.ss_wages, a.medicare_wages, a.futa_wages, a.suta_wages, a.state_wages];
    if wages.iter().any(|w| *w > gross) {
        return Err(AppError::bad_request("Taxable wages can't be more than the gross pay"));
    }
    if a.additional_medicare_wages > a.medicare_wages {
        return Err(AppError::bad_request("Additional Medicare wages can't be more than the Medicare wages"));
    }
    Ok((work_state, suta_state, a))
}

async fn write(
    conn: &mut SqliteConnection,
    id: &str,
    input: &PriorInput,
    states: (String, String),
    a: &Amounts,
    insert: bool,
) -> AppResult<()> {
    let sql = if insert {
        r#"INSERT INTO "us_payroll_prior" ("employeeId","payDate","workState","sutaState","gross","pretax401k",
             "pretaxSection125","incomeTaxWages","federalIncomeTax","socialSecurityWages","socialSecurityEmployee",
             "socialSecurityEmployer","medicareWages","medicareEmployee","medicareEmployer","additionalMedicareWages",
             "additionalMedicare","futaWages","futa","sutaWages","suta","stateIncomeTaxWages","stateIncomeTax","notes",
             "updatedAt","id")
           VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20,?21,?22,?23,?24,?25,?26)"#
    } else {
        r#"UPDATE "us_payroll_prior" SET "employeeId"=?1,"payDate"=?2,"workState"=?3,"sutaState"=?4,"gross"=?5,
             "pretax401k"=?6,"pretaxSection125"=?7,"incomeTaxWages"=?8,"federalIncomeTax"=?9,"socialSecurityWages"=?10,
             "socialSecurityEmployee"=?11,"socialSecurityEmployer"=?12,"medicareWages"=?13,"medicareEmployee"=?14,
             "medicareEmployer"=?15,"additionalMedicareWages"=?16,"additionalMedicare"=?17,"futaWages"=?18,"futa"=?19,
             "sutaWages"=?20,"suta"=?21,"stateIncomeTaxWages"=?22,"stateIncomeTax"=?23,"notes"=?24,"updatedAt"=?25
           WHERE "id"=?26"#
    };
    sqlx::query(sql)
        .bind(&input.employee_id)
        .bind(&input.pay_date)
        .bind(&states.0)
        .bind(&states.1)
        .bind(a.gross)
        .bind(a.pretax_401k)
        .bind(a.pretax_125)
        .bind(a.income_tax_wages)
        .bind(a.federal_income_tax)
        .bind(a.ss_wages)
        .bind(a.ss_employee)
        .bind(a.ss_employer)
        .bind(a.medicare_wages)
        .bind(a.medicare_employee)
        .bind(a.medicare_employer)
        .bind(a.additional_medicare_wages)
        .bind(a.additional_medicare)
        .bind(a.futa_wages)
        .bind(a.futa)
        .bind(a.suta_wages)
        .bind(a.suta)
        .bind(a.state_wages)
        .bind(a.state_tax)
        .bind(input.notes.as_deref().map(str::trim).filter(|n| !n.is_empty()))
        .bind(now_iso())
        .bind(id)
        .execute(&mut *conn)
        .await?;
    Ok(())
}

pub async fn create(db: &Db, input: PriorInput) -> AppResult<Value> {
    let mut tx = db.begin().await?;
    let (work, suta, a) = fill(&mut tx, &input, None).await?;
    let id = new_id();
    write(&mut tx, &id, &input, (work, suta), &a, true).await?;
    let out = find_one(&mut tx, &id).await?.to_json();
    tx.commit().await?;
    Ok(out)
}

pub async fn update(db: &Db, id: &str, input: PriorInput) -> AppResult<Value> {
    let mut tx = db.begin().await?;
    find_one(&mut tx, id).await?;
    let (work, suta, a) = fill(&mut tx, &input, Some(id)).await?;
    write(&mut tx, id, &input, (work, suta), &a, false).await?;
    let out = find_one(&mut tx, id).await?.to_json();
    tx.commit().await?;
    Ok(out)
}

pub async fn delete(db: &Db, id: &str) -> AppResult<()> {
    let mut tx = db.begin().await?;
    find_one(&mut tx, id).await?;
    sqlx::query(r#"DELETE FROM "us_payroll_prior" WHERE "id" = ?1"#).bind(id).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(())
}

/// `GET /payroll/us/prior?year`: the rows and their totals.
pub async fn list_json(db: &Db, year: i32) -> AppResult<Value> {
    let mut c = db.read().acquire().await?;
    let rows = list(&mut c, year).await?;
    let sum = |f: &dyn Fn(&Amounts) -> i64| m(rows.iter().map(|p| f(&p.a)).sum());
    Ok(json!({
        "year": year,
        "rows": rows.iter().map(Prior::to_json).collect::<Vec<_>>(),
        "totals": {
            "gross": sum(&|a| a.gross),
            "federalIncomeTax": sum(&|a| a.federal_income_tax),
            "socialSecurityEmployee": sum(&|a| a.ss_employee),
            "medicareEmployee": sum(&|a| a.medicare_employee + a.additional_medicare),
            "stateIncomeTax": sum(&|a| a.state_tax),
        },
    }))
}
