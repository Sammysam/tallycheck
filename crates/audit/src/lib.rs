//! Activity log (reference `modules/audit`): the action summariser used by the audit
//! interceptor, best-effort recording, and the paginated listing.

use app_core::ids::new_id;
use app_core::time::now_iso;
use app_core::AppResult;
use database::Db;
use serde::Serialize;
use serde_json::Value;
use sqlx::Row;

#[derive(Debug, Clone, Default)]
pub struct NewEntry {
    pub user_id: Option<String>,
    pub username: Option<String>,
    pub action: String,
    pub method: Option<String>,
    pub path: Option<String>,
    pub status_code: Option<u16>,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditLog {
    pub id: String,
    pub user_id: Option<String>,
    pub username: Option<String>,
    pub action: String,
    pub method: Option<String>,
    pub path: Option<String>,
    pub status_code: Option<i64>,
    pub detail: Option<String>,
    pub created_at: String,
}

/// Records an activity. Like the reference, a failed audit write never fails the request —
/// but unlike the reference it is logged instead of silently swallowed.
pub async fn record(db: &Db, entry: NewEntry) {
    let result = sqlx::query(
        r#"INSERT INTO "audit_logs" ("id","userId","username","action","method","path","statusCode","detail","createdAt")
           VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)"#,
    )
    .bind(new_id())
    .bind(&entry.user_id)
    .bind(&entry.username)
    .bind(&entry.action)
    .bind(&entry.method)
    .bind(&entry.path)
    .bind(entry.status_code.map(i64::from))
    .bind(&entry.detail)
    .bind(now_iso())
    .execute(db.write())
    .await;
    if let Err(e) = result {
        tracing::error!(action = %entry.action, "audit write failed: {e}");
    }
}

#[derive(Debug, Serialize)]
pub struct Paginated<T> {
    pub data: Vec<T>,
    pub total: i64,
    pub page: i64,
    pub limit: i64,
    #[serde(rename = "totalPages")]
    pub total_pages: i64,
}

impl<T> Paginated<T> {
    /// Reference `paginate()`: `totalPages = Math.ceil(total / limit)`.
    pub fn new(data: Vec<T>, total: i64, page: i64, limit: i64) -> Self {
        let total_pages = if limit > 0 { (total + limit - 1) / limit } else { 0 };
        Paginated { data, total, page, limit, total_pages }
    }
}

/// D36: what the Activity Log can be narrowed to. Every field is optional.
#[derive(Debug, Clone, Default)]
pub struct Filter {
    pub user_id: Option<String>,
    /// Inclusive local dates (`YYYY-MM-DD`), compared with the stored timestamp's local date.
    pub date_from: Option<String>,
    pub date_to: Option<String>,
    /// Text to find in the activity, its details or the user name.
    pub search: Option<String>,
    /// `signin`, `users`, `settings` or `documents`.
    pub kind: Option<String>,
}

const SIGNIN: &str = r#"(lower("action") LIKE '%login%' OR lower("action") LIKE 'logged%' OR lower("action") LIKE '%2fa%' OR lower("action") LIKE '%two-factor%')"#;
const USERS: &str = r#"(lower("action") LIKE '%user%' OR lower("action") LIKE '%role%' OR lower("action") LIKE '%password%' OR lower("action") LIKE '%administrator%')"#;
const SETTINGS: &str = r#"(lower("action") LIKE '%setting%' OR lower("action") LIKE '%tax%' OR lower("action") LIKE '%lock%' OR lower("action") LIKE '%backup%' OR lower("action") LIKE '%restore%')"#;

/// The SQL condition for a kind of activity, on the lower-cased action text.
fn kind_condition(kind: &str) -> Option<String> {
    Some(match kind {
        "signin" => SIGNIN.to_string(),
        "users" => format!("({USERS} AND NOT {SIGNIN})"),
        "settings" => SETTINGS.to_string(),
        "documents" => format!("NOT ({SIGNIN} OR {USERS} OR {SETTINGS})"),
        _ => return None,
    })
}

pub async fn list(db: &Db, page: i64, limit: i64, filter: &Filter) -> AppResult<Paginated<AuditLog>> {
    let offset = (page - 1).saturating_mul(limit);
    let mut binds: Vec<String> = Vec::new();
    let mut conditions: Vec<String> = Vec::new();
    if let Some(u) = &filter.user_id {
        binds.push(u.clone());
        conditions.push(format!(r#""userId" = ?{}"#, binds.len()));
    }
    if let Some(d) = &filter.date_from {
        binds.push(d.clone());
        conditions.push(format!(r#"date("createdAt", 'localtime') >= ?{}"#, binds.len()));
    }
    if let Some(d) = &filter.date_to {
        binds.push(d.clone());
        conditions.push(format!(r#"date("createdAt", 'localtime') <= ?{}"#, binds.len()));
    }
    if let Some(q) = filter.search.as_deref().map(str::trim).filter(|q| !q.is_empty()) {
        binds.push(format!("%{}%", q.to_lowercase()));
        let n = binds.len();
        conditions.push(format!(
            r#"(lower("action") LIKE ?{n} OR lower(COALESCE("detail", '')) LIKE ?{n} OR lower(COALESCE("username", '')) LIKE ?{n})"#
        ));
    }
    if let Some(cond) = filter.kind.as_deref().and_then(kind_condition) {
        conditions.push(cond);
    }
    let clause = if conditions.is_empty() { String::new() } else { format!("WHERE {}", conditions.join(" AND ")) };
    let n = binds.len();
    let sql = format!(
        r#"SELECT * FROM "audit_logs" {clause} ORDER BY "createdAt" DESC, rowid DESC LIMIT ?{} OFFSET ?{}"#,
        n + 1,
        n + 2
    );
    let mut q = sqlx::query(&sql);
    for b in &binds {
        q = q.bind(b);
    }
    let rows = q.bind(limit).bind(offset).fetch_all(db.read()).await?;
    let count_sql = format!(r#"SELECT COUNT(*) FROM "audit_logs" {clause}"#);
    let mut c = sqlx::query_scalar::<_, i64>(&count_sql);
    for b in &binds {
        c = c.bind(b);
    }
    let total = c.fetch_one(db.read()).await?;
    let data = rows
        .iter()
        .map(|r| {
            Ok(AuditLog {
                id: r.try_get("id")?,
                user_id: r.try_get("userId")?,
                username: r.try_get("username")?,
                action: r.try_get("action")?,
                method: r.try_get("method")?,
                path: r.try_get("path")?,
                status_code: r.try_get("statusCode")?,
                detail: r.try_get("detail")?,
                created_at: r.try_get("createdAt")?,
            })
        })
        .collect::<Result<Vec<_>, sqlx::Error>>()?;
    Ok(Paginated::new(data, total, page, limit))
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LastSignIn {
    pub user_id: String,
    pub at: String,
}

/// D36: each user's most recent successful sign-in, from the log.
pub async fn last_sign_ins(db: &Db) -> AppResult<Vec<LastSignIn>> {
    let rows: Vec<(String, String)> = sqlx::query_as(
        r#"SELECT "userId", MAX("createdAt") FROM "audit_logs"
           WHERE "userId" IS NOT NULL AND "action" = 'Logged in' GROUP BY "userId""#,
    )
    .fetch_all(db.read())
    .await?;
    Ok(rows.into_iter().map(|(user_id, at)| LastSignIn { user_id, at }).collect())
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditUser {
    pub user_id: String,
    pub username: Option<String>,
}

/// Distinct users in the log, for the filter dropdown (reference `AuditService.users`).
pub async fn users(db: &Db) -> AppResult<Vec<AuditUser>> {
    let rows: Vec<(String, Option<String>)> = sqlx::query_as(
        r#"SELECT "userId", MAX("username") AS "username" FROM "audit_logs"
           WHERE "userId" IS NOT NULL GROUP BY "userId" ORDER BY "username" ASC"#,
    )
    .fetch_all(db.read())
    .await?;
    Ok(rows.into_iter().map(|(user_id, username)| AuditUser { user_id, username }).collect())
}

// ─── Summariser: exact port of audit.interceptor.ts ─────────────────────────

fn verb(method: &str) -> &str {
    match method {
        "POST" => "Created",
        "PATCH" | "PUT" => "Updated",
        "DELETE" => "Deleted",
        other => other,
    }
}

fn singular(resource: &str) -> &str {
    match resource {
        "invoices" => "invoice",
        "bills" => "bill",
        "customers" => "customer",
        "vendors" => "vendor",
        "accounts" => "account",
        "expenses" => "expense",
        "payments" => "payment",
        "credit-notes" => "credit note",
        "debit-notes" => "debit note",
        "tax-codes" => "tax code",
        "banking" => "bank account",
        "users" => "user",
        "roles" => "role",
        "ledger" => "ledger entry",
        "employees" => "employee",
        other => other,
    }
}

fn article(noun: &str) -> &'static str {
    match noun.chars().next().map(|c| c.to_ascii_lowercase()) {
        Some('a' | 'e' | 'i' | 'o' | 'u') => "an",
        _ => "a",
    }
}

/// Human-readable action for a mutating request, word for word as the reference writes it.
pub fn summarize(method: &str, path: &str) -> String {
    let no_prefix = path.strip_prefix('/').unwrap_or(path);
    let no_prefix = no_prefix.strip_prefix("api/").unwrap_or(no_prefix);
    let clean = no_prefix.split('?').next().unwrap_or("").trim_end_matches('/');
    let seg: Vec<&str> = clean.split('/').collect();
    let resource = seg.first().copied().unwrap_or("");
    let last = seg.last().copied().unwrap_or("");
    let at = |i: usize| seg.get(i).copied();
    let noun = singular(resource);

    if resource == "system" {
        return match (at(1), last) {
            (Some("backup"), "run") => "Made a backup".into(),
            (Some("backup"), "password") => "Changed the backup password".into(),
            (Some("backup"), "settings") => "Changed the daily backup settings".into(),
            (Some("restore"), "verify") => "Checked a backup file".into(),
            (Some("restore"), _) if method == "DELETE" => "Cancelled a staged restore".into(),
            (Some("restore"), _) => "Staged a restore from a backup".into(),
            _ => "Changed system settings".into(),
        };
    }
    match last {
        "post" => return format!("Posted {} {noun}", article(noun)),
        "void" => return format!("Voided {} {noun}", article(noun)),
        "reverse" => return format!("Reversed {} {noun}", article(noun)),
        "seed" => return format!("Seeded {resource}"),
        "reset-password" => return "Reset a user's password".into(),
        "reset-2fa" => return "Reset a user's 2FA".into(),
        _ => {}
    }
    if resource == "settings" {
        return "Updated company settings".into();
    }
    if resource == "reconciliations" {
        return match (method, last) {
            (_, "complete") => "Completed a bank reconciliation",
            (_, "undo") => "Undid a bank reconciliation",
            (_, "lines") => "Saved a bank reconciliation in progress",
            ("DELETE", _) => "Discarded a bank reconciliation",
            _ => "Started a bank reconciliation",
        }
        .into();
    }
    // D37: import from file and bank transactions.
    if resource == "imports" {
        return match (method, at(1)) {
            ("POST", _) => "Imported records from a file",
            (_, Some("layouts")) if method == "DELETE" => "Deleted an import layout",
            (_, Some("layouts")) => "Saved an import layout",
            _ => "Undid an import",
        }
        .into();
    }
    if resource == "bank-lines" {
        return match last {
            "match" => "Matched a bank transaction",
            "payment" => "Recorded a payment from a bank transaction",
            "expense" => "Recorded an expense from a bank transaction",
            "ignore" => "Ignored a bank transaction",
            "restore" => "Restored an ignored bank transaction",
            _ => "Unmatched a bank transaction",
        }
        .into();
    }
    if resource == "books" && last == "lock" {
        return "Changed the books lock date".into();
    }

    if resource == "payroll" && at(1) == Some("runs") {
        match last {
            "post" => return "Posted a payroll run to the ledger".into(),
            "pay" => return "Paid a payroll run".into(),
            "remit" => return "Recorded a statutory remittance (EPF/ETF/PAYE)".into(),
            "void" => return "Voided a payroll run".into(),
            "slips-filed" => return "Filed salary slips into employees' documents".into(),
            _ => {}
        }
        if at(3) == Some("lines") {
            return match method {
                "POST" => "Added an employee to a payroll run".into(),
                "DELETE" => "Removed an employee from a payroll run".into(),
                _ => "Edited an employee's payroll line".into(),
            };
        }
        if method == "POST" {
            return "Created a payroll run".into();
        }
        if method == "DELETE" {
            return "Deleted a draft payroll run".into();
        }
    }
    if resource == "payroll" && at(1) == Some("rates") {
        return "Updated payroll rates".into();
    }
    if resource == "payroll" && at(1) == Some("company") {
        return "Updated payroll company details".into();
    }
    if resource == "payroll" && at(1) == Some("employees") {
        if at(3) == Some("bank") {
            return "Updated an employee's bank details".into();
        }
        if at(3) == Some("salaries") {
            return match method {
                "POST" => "Added a salary / increment".into(),
                "DELETE" => "Deleted a salary record".into(),
                _ => "Updated a salary record".into(),
            };
        }
    }
    if resource == "employees" && seg.len() >= 3 {
        if at(2) == Some("photo") {
            return if method == "DELETE" {
                "Removed an employee's photo".into()
            } else {
                "Updated an employee's photo".into()
            };
        }
        if at(2) == Some("documents") {
            if last == "replace" {
                return "Uploaded a new version of an employee document".into();
            }
            return match method {
                "POST" => "Uploaded an employee document".into(),
                "DELETE" => "Deleted an employee document".into(),
                _ => "Updated an employee document".into(),
            };
        }
    }
    format!("{} {} {noun}", verb(method), article(noun))
}

/// `detailFrom(body)`: the first truthy of invoiceNumber, billNumber, title, name, code
/// (plus `noteNumber`, D21, which no reference response carries).
pub fn detail_from(body: &Value) -> Option<String> {
    let obj = body.as_object()?;
    ["invoiceNumber", "billNumber", "noteNumber", "title", "name", "code"].iter().find_map(|k| match obj.get(*k) {
        Some(Value::String(s)) if !s.is_empty() => Some(s.clone()),
        Some(Value::Number(n)) if n.as_f64() != Some(0.0) => Some(n.to_string()),
        Some(Value::Bool(true)) => Some("true".into()),
        _ => None,
    })
}

#[cfg(test)]
mod tests;
