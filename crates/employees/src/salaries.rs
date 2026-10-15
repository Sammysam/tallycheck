//! Dated salary structures, increments and bank details (reference `payroll.service.ts`, the
//! part behind `/payroll/employees`). Amounts are monthly rupees, held as integer cents.

use crate::{records, Actor};
use app_core::ids::new_id;
use app_core::money::Money;
use app_core::time::{now_iso, today_local};
use app_core::{AppError, AppResult};
use database::Db;
use serde_json::{json, Map, Value};
use sqlx::sqlite::SqliteRow;
use sqlx::{Row, SqliteConnection};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Salary {
    pub id: String,
    pub employee_id: String,
    pub effective_from: String,
    pub basic: Money,
    pub bra2005: Money,
    pub bra2016: Money,
    pub vehicle_allowance: Money,
    pub other_allowance: Money,
    pub epf_applicable: bool,
    pub welfare_deduction: Money,
    pub loan_installment: Money,
    pub reason: Option<String>,
    pub notes: Option<String>,
    pub created_by_id: Option<String>,
    pub created_by_name: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl Salary {
    /// Fixed monthly pay before month-specific items (reference `monthlyTotal`).
    pub fn monthly_total(&self) -> Option<Money> {
        self.basic
            .checked_add(self.bra2005)?
            .checked_add(self.bra2016)?
            .checked_add(self.vehicle_allowance)?
            .checked_add(self.other_allowance)
    }

    /// What EPF/ETF is calculated on: basic plus the BRA allowances, zero without EPF.
    pub fn epf_base(&self) -> Option<Money> {
        if !self.epf_applicable {
            return Some(Money::ZERO);
        }
        self.basic.checked_add(self.bra2005)?.checked_add(self.bra2016)
    }
}

const COLUMNS: &str = r#""id","employeeId","effectiveFrom","basic","bra2005","bra2016","vehicleAllowance","otherAllowance",
"epfApplicable","welfareDeduction","loanInstallment","reason","notes","createdById","createdByName","createdAt","updatedAt""#;

fn from_row(r: &SqliteRow) -> AppResult<Salary> {
    let m = |col: &str| -> AppResult<Money> { Ok(Money::from_cents(r.try_get(col)?)) };
    Ok(Salary {
        id: r.try_get("id")?,
        employee_id: r.try_get("employeeId")?,
        effective_from: r.try_get("effectiveFrom")?,
        basic: m("basic")?,
        bra2005: m("bra2005")?,
        bra2016: m("bra2016")?,
        vehicle_allowance: m("vehicleAllowance")?,
        other_allowance: m("otherAllowance")?,
        epf_applicable: r.try_get::<i64, _>("epfApplicable")? != 0,
        welfare_deduction: m("welfareDeduction")?,
        loan_installment: m("loanInstallment")?,
        reason: r.try_get("reason")?,
        notes: r.try_get("notes")?,
        created_by_id: r.try_get("createdById")?,
        created_by_name: r.try_get("createdByName")?,
        created_at: r.try_get("createdAt")?,
        updated_at: r.try_get("updatedAt")?,
    })
}

/// Rows ascending by effective date.
pub async fn rows(conn: &mut SqliteConnection, employee_id: &str) -> AppResult<Vec<Salary>> {
    let rows = sqlx::query(&format!(
        r#"SELECT {COLUMNS} FROM "employee_salaries" WHERE "employeeId" = ?1 ORDER BY "effectiveFrom" ASC"#
    ))
    .bind(employee_id)
    .fetch_all(conn)
    .await?;
    rows.iter().map(from_row).collect()
}

/// The salary in force on `date` (latest effectiveFrom on or before it) — what a payroll run
/// for that month uses.
pub async fn salary_as_of(conn: &mut SqliteConnection, employee_id: &str, date: &str) -> AppResult<Option<Salary>> {
    let row = sqlx::query(&format!(
        r#"SELECT {COLUMNS} FROM "employee_salaries" WHERE "employeeId" = ?1 AND "effectiveFrom" <= ?2
           ORDER BY "effectiveFrom" DESC LIMIT 1"#
    ))
    .bind(employee_id)
    .bind(date)
    .fetch_optional(conn)
    .await?;
    row.as_ref().map(from_row).transpose()
}

pub async fn get_salary(conn: &mut SqliteConnection, employee_id: &str, id: &str) -> AppResult<Salary> {
    let row =
        sqlx::query(&format!(r#"SELECT {COLUMNS} FROM "employee_salaries" WHERE "id" = ?1 AND "employeeId" = ?2"#))
            .bind(id)
            .bind(employee_id)
            .fetch_optional(conn)
            .await?;
    row.as_ref().map(from_row).transpose()?.ok_or_else(|| AppError::NotFound(format!("Salary record {id} not found")))
}

fn overflow() -> AppError {
    AppError::bad_request("Amount is too large")
}

/// `changePercent`: change ÷ previous total × 100, to 2 decimals, half away from zero
/// (exact; the reference's `toFixed(2)` on a float agrees except at rare binary ties, D4).
fn change_percent(change: Money, prev: Money) -> Option<Money> {
    if prev.cents() == 0 {
        return None;
    }
    let num = i128::from(change.cents()) * 10_000;
    let den = i128::from(prev.cents());
    let (q, r) = (num / den, num % den);
    let rounded = if (r.abs() * 2) >= den.abs() { q + if (num < 0) != (den < 0) { -1 } else { 1 } } else { q };
    i64::try_from(rounded).ok().map(Money::from_cents)
}

/// One salary row with the figures shown next to it. Every amount is a JSON number here (the
/// reference converted them with `Number()`).
fn decorate(rows: &[Salary], as_of: &str) -> AppResult<Vec<Value>> {
    let in_force = rows.iter().rposition(|r| r.effective_from.as_str() <= as_of);
    let mut out = Vec::with_capacity(rows.len());
    let mut prev_total: Option<Money> = None;
    for (i, row) in rows.iter().enumerate() {
        let total = row.monthly_total().ok_or_else(overflow)?;
        let change = match prev_total {
            Some(p) => Some(total.checked_sub(p).ok_or_else(overflow)?),
            None => None,
        };
        let percent = match (change, prev_total) {
            (Some(c), Some(p)) => change_percent(c, p),
            _ => None,
        };
        let state = if Some(i) == in_force {
            "CURRENT"
        } else if row.effective_from.as_str() > as_of {
            "UPCOMING"
        } else {
            "PAST"
        };
        out.push(json!({
            "id": row.id,
            "employeeId": row.employee_id,
            "effectiveFrom": row.effective_from,
            "basic": row.basic.to_json_number(),
            "bra2005": row.bra2005.to_json_number(),
            "bra2016": row.bra2016.to_json_number(),
            "vehicleAllowance": row.vehicle_allowance.to_json_number(),
            "otherAllowance": row.other_allowance.to_json_number(),
            "epfApplicable": row.epf_applicable,
            "welfareDeduction": row.welfare_deduction.to_json_number(),
            "loanInstallment": row.loan_installment.to_json_number(),
            "reason": row.reason,
            "notes": row.notes,
            "createdById": row.created_by_id,
            "createdByName": row.created_by_name,
            "createdAt": row.created_at,
            "updatedAt": row.updated_at,
            "monthlyTotal": total.to_json_number(),
            "epfBase": row.epf_base().ok_or_else(overflow)?.to_json_number(),
            "changeAmount": change.map(Money::to_json_number),
            "changePercent": percent.map(Money::to_json_number),
            "state": state,
        }));
        prev_total = Some(total);
    }
    Ok(out)
}

fn today() -> String {
    // Deviation D5: the local (Sri Lanka) date; the reference used the UTC date.
    today_local().to_string()
}

/// `GET /payroll/employees/:id`: bank details, loan balance, current and upcoming salary and
/// the full history (newest first). `as_of` defaults to today.
pub async fn employee_payroll(db: &Db, employee_id: &str, as_of: Option<&str>) -> AppResult<Value> {
    let mut c = db.read().acquire().await?;
    payroll_view(&mut c, employee_id, as_of).await
}

async fn payroll_view(conn: &mut SqliteConnection, employee_id: &str, as_of: Option<&str>) -> AppResult<Value> {
    let as_of = as_of.filter(|s| !s.is_empty()).map(str::to_string).unwrap_or_else(today);
    let e = records::find_one(&mut *conn, employee_id).await?;
    let views = decorate(&rows(conn, employee_id).await?, &as_of)?;
    let current = views.iter().rev().find(|v| v["state"] == "CURRENT").cloned().unwrap_or(Value::Null);
    let upcoming = views.iter().find(|v| v["state"] == "UPCOMING").cloned().unwrap_or(Value::Null);
    let history: Vec<Value> = views.into_iter().rev().collect();
    Ok(json!({
        "employeeId": employee_id,
        "bank": { "bankName": e.bank_name, "bankBranch": e.bank_branch, "bankAccountNo": e.bank_account_no },
        "staffLoanBalance": e.staff_loan_balance.to_json_number(),
        "current": current,
        "upcoming": upcoming,
        "history": history,
    }))
}

async fn view_of(conn: &mut SqliteConnection, employee_id: &str, id: &str) -> AppResult<Value> {
    decorate(&rows(conn, employee_id).await?, &today())?
        .into_iter()
        .find(|v| v["id"] == id)
        .ok_or_else(|| AppError::NotFound(format!("Salary record {id} not found")))
}

/// PATCH bank: only sent fields change; null clears a text field.
#[derive(Debug, Default, Clone)]
pub struct BankFields {
    pub bank_name: Option<Option<String>>,
    pub bank_branch: Option<Option<String>>,
    pub bank_account_no: Option<Option<String>>,
    pub staff_loan_balance: Option<Money>,
}

pub async fn update_bank(db: &Db, employee_id: &str, f: BankFields) -> AppResult<Value> {
    let mut tx = db.begin().await?;
    let current = records::find_one(&mut tx, employee_id).await?;
    let mut next = current.clone();
    if let Some(v) = f.bank_name {
        next.bank_name = v;
    }
    if let Some(v) = f.bank_branch {
        next.bank_branch = v;
    }
    if let Some(v) = f.bank_account_no {
        next.bank_account_no = v;
    }
    if let Some(v) = f.staff_loan_balance {
        next.staff_loan_balance = v;
    }
    records::save_if_changed(&mut tx, &current, &next).await?;
    let view = payroll_view(&mut tx, employee_id, None).await?;
    tx.commit().await?;
    Ok(view)
}

/// Create / PATCH fields after validation. For `reason` and `notes`, `Some(None)` clears.
#[derive(Debug, Default, Clone)]
pub struct SalaryFields {
    pub effective_from: Option<String>,
    pub basic: Option<Money>,
    pub bra2005: Option<Money>,
    pub bra2016: Option<Money>,
    pub vehicle_allowance: Option<Money>,
    pub other_allowance: Option<Money>,
    pub epf_applicable: Option<bool>,
    pub welfare_deduction: Option<Money>,
    pub loan_installment: Option<Money>,
    pub reason: Option<Option<String>>,
    pub notes: Option<Option<String>>,
}

async fn date_taken(conn: &mut SqliteConnection, employee_id: &str, date: &str) -> AppResult<bool> {
    let hit: Option<String> =
        sqlx::query_scalar(r#"SELECT "id" FROM "employee_salaries" WHERE "employeeId" = ?1 AND "effectiveFrom" = ?2"#)
            .bind(employee_id)
            .bind(date)
            .fetch_optional(conn)
            .await?;
    Ok(hit.is_some())
}

async fn write(conn: &mut SqliteConnection, s: &Salary) -> AppResult<()> {
    sqlx::query(
        r#"INSERT INTO "employee_salaries" ("id","employeeId","effectiveFrom","basic","bra2005","bra2016",
           "vehicleAllowance","otherAllowance","epfApplicable","welfareDeduction","loanInstallment","reason","notes",
           "createdById","createdByName","createdAt","updatedAt")
           VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17)
           ON CONFLICT ("id") DO UPDATE SET "effectiveFrom"=excluded."effectiveFrom","basic"=excluded."basic",
           "bra2005"=excluded."bra2005","bra2016"=excluded."bra2016","vehicleAllowance"=excluded."vehicleAllowance",
           "otherAllowance"=excluded."otherAllowance","epfApplicable"=excluded."epfApplicable",
           "welfareDeduction"=excluded."welfareDeduction","loanInstallment"=excluded."loanInstallment",
           "reason"=excluded."reason","notes"=excluded."notes","updatedAt"=excluded."updatedAt""#,
    )
    .bind(&s.id)
    .bind(&s.employee_id)
    .bind(&s.effective_from)
    .bind(s.basic.cents())
    .bind(s.bra2005.cents())
    .bind(s.bra2016.cents())
    .bind(s.vehicle_allowance.cents())
    .bind(s.other_allowance.cents())
    .bind(i64::from(s.epf_applicable))
    .bind(s.welfare_deduction.cents())
    .bind(s.loan_installment.cents())
    .bind(&s.reason)
    .bind(&s.notes)
    .bind(&s.created_by_id)
    .bind(&s.created_by_name)
    .bind(&s.created_at)
    .bind(&s.updated_at)
    .execute(conn)
    .await?;
    Ok(())
}

/// First salary or an increment: a new dated row either way.
pub async fn add(db: &Db, employee_id: &str, f: SalaryFields, by: &Actor) -> AppResult<Value> {
    let mut tx = db.begin().await?;
    let e = records::find_one(&mut tx, employee_id).await?;
    let date = f.effective_from.clone().unwrap_or_default();
    if date_taken(&mut tx, employee_id, &date).await? {
        return Err(AppError::bad_request(format!(
            "A salary already starts on {date} — edit that one or pick another date"
        )));
    }
    let now = now_iso();
    let row = Salary {
        id: new_id(),
        employee_id: employee_id.to_string(),
        effective_from: date,
        basic: f.basic.unwrap_or(Money::ZERO),
        bra2005: f.bra2005.unwrap_or(Money::ZERO),
        bra2016: f.bra2016.unwrap_or(Money::ZERO),
        vehicle_allowance: f.vehicle_allowance.unwrap_or(Money::ZERO),
        other_allowance: f.other_allowance.unwrap_or(Money::ZERO),
        // Contract labour never pays EPF; permanent staff default to yes.
        epf_applicable: f.epf_applicable.unwrap_or(e.employment_type != "CONTRACT"),
        welfare_deduction: f.welfare_deduction.unwrap_or(Money::from_cents(30_000)),
        loan_installment: f.loan_installment.unwrap_or(Money::ZERO),
        reason: f.reason.flatten(),
        notes: f.notes.flatten(),
        created_by_id: by.id.clone(),
        created_by_name: by.username.clone(),
        created_at: now.clone(),
        updated_at: now,
    };
    row.monthly_total().ok_or_else(overflow)?;
    write(&mut tx, &row).await?;
    let view = view_of(&mut tx, employee_id, &row.id).await?;
    tx.commit().await?;
    Ok(view)
}

pub async fn update(db: &Db, employee_id: &str, id: &str, f: SalaryFields) -> AppResult<Value> {
    let mut tx = db.begin().await?;
    let current = get_salary(&mut tx, employee_id, id).await?;
    if let Some(d) = &f.effective_from {
        if *d != current.effective_from && date_taken(&mut tx, employee_id, d).await? {
            return Err(AppError::bad_request(format!("A salary already starts on {d}")));
        }
    }
    let mut next = current.clone();
    if let Some(v) = f.effective_from {
        next.effective_from = v;
    }
    for (slot, v) in [
        (&mut next.basic, f.basic),
        (&mut next.bra2005, f.bra2005),
        (&mut next.bra2016, f.bra2016),
        (&mut next.vehicle_allowance, f.vehicle_allowance),
        (&mut next.other_allowance, f.other_allowance),
        (&mut next.welfare_deduction, f.welfare_deduction),
        (&mut next.loan_installment, f.loan_installment),
    ] {
        if let Some(v) = v {
            *slot = v;
        }
    }
    if let Some(v) = f.epf_applicable {
        next.epf_applicable = v;
    }
    if let Some(v) = f.reason {
        next.reason = v;
    }
    if let Some(v) = f.notes {
        next.notes = v;
    }
    next.monthly_total().ok_or_else(overflow)?;
    if next != current {
        next.updated_at = now_iso();
        write(&mut tx, &next).await?;
    }
    let view = view_of(&mut tx, employee_id, id).await?;
    tx.commit().await?;
    Ok(view)
}

pub async fn remove(db: &Db, employee_id: &str, id: &str) -> AppResult<()> {
    let mut tx = db.begin().await?;
    get_salary(&mut tx, employee_id, id).await?;
    sqlx::query(r#"DELETE FROM "employee_salaries" WHERE "id" = ?1"#).bind(id).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(())
}

/// Removes the bank fields from an employee JSON object for callers without `payroll`.
pub fn redact(obj: &mut Map<String, Value>) {
    for f in records::PAYROLL_ONLY_FIELDS {
        obj.remove(*f);
    }
}
