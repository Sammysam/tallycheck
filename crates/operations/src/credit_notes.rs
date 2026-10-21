//! Credit notes and debit notes (approved deviation D21, requested 2026-10-20). The reference
//! had neither; the only way to reduce an invoice was to void it whole.
//!
//! * AR, a customer credit note (`CN-000001`) against a POSTED invoice: debit a revenue
//!   account, credit 1100 Accounts Receivable.
//! * AP, a vendor debit note (`DN-000001`) against a POSTED bill: debit 2000 Accounts Payable,
//!   credit the expense (or asset) account the cost was booked to.
//!
//! A note never exceeds what is still owed (total - payments - earlier notes) and takes that
//! amount off the document, which becomes PAID once nothing is owed. Voiding a note reverses
//! its entry (dated today) and puts the amount back. Note, entry and document change in one
//! transaction. Refunding money already received is a separate step this does not cover.

use crate::invoices::{self, Scope};
use crate::{bills, money_str, opt, overflow};
use app_core::ids::new_id;
use app_core::money::Money;
use app_core::time::now_iso;
use app_core::{AppError, AppResult};
use database::{counters, Db};
use ledger::accounts::{self, AccountType};
use ledger::{entries, NewEntry, NewLine};
use serde_json::{json, Value};
use sqlx::sqlite::SqliteRow;
use sqlx::{Row, SqliteConnection};

pub const AR: &str = "AR";
pub const AP: &str = "AP";

#[derive(Debug, Clone)]
pub struct CreditNote {
    pub id: String,
    pub note_number: String,
    pub kind: String,
    pub invoice_id: Option<String>,
    pub bill_id: Option<String>,
    pub date: String,
    pub reason: String,
    pub amount: Money,
    pub account_id: String,
    pub status: String,
    pub journal_entry_id: Option<String>,
    pub created_by_id: Option<String>,
    /// D26: the amount includes tax; this is the tax inside it, reversed with the note.
    pub tax_code_id: Option<String>,
    pub tax_amount: Money,
    pub created_at: String,
    pub updated_at: String,
    /// The invoice or bill number, and the customer or vendor name, for lists.
    pub document_number: Option<String>,
    pub party_name: Option<String>,
    /// D30: the document's currency (None = home); `amount` is in it.
    pub currency: Option<String>,
}

impl CreditNote {
    pub fn to_json(&self) -> Value {
        json!({
            "id": self.id,
            "noteNumber": self.note_number,
            "type": self.kind,
            "invoiceId": opt(&self.invoice_id),
            "billId": opt(&self.bill_id),
            "documentNumber": opt(&self.document_number),
            "partyName": opt(&self.party_name),
            "currency": opt(&self.currency),
            "date": self.date,
            "reason": self.reason,
            "amount": money_str(self.amount),
            "accountId": self.account_id,
            "status": self.status,
            "journalEntryId": opt(&self.journal_entry_id),
            "createdById": opt(&self.created_by_id),
            "taxCodeId": opt(&self.tax_code_id),
            "taxAmount": money_str(self.tax_amount),
            "createdAt": self.created_at,
            "updatedAt": self.updated_at,
        })
    }
}

const SELECT: &str = r#"SELECT n."id", n."noteNumber", n."type", n."invoiceId", n."billId", n."date", n."reason", n."amount",
       n."accountId", n."status", n."journalEntryId", n."createdById", n."taxCodeId", n."taxAmount", n."createdAt", n."updatedAt",
       COALESCE(i."invoiceNumber", b."billNumber") AS "documentNumber",
       COALESCE(NULLIF(c."name", ''), i."billToName", v."name") AS "partyName",
       COALESCE(i."currency", b."currency") AS "currency"
  FROM "credit_notes" n
  LEFT JOIN "invoices" i ON i."id" = n."invoiceId"
  LEFT JOIN "customers" c ON c."id" = i."customerId"
  LEFT JOIN "bills" b ON b."id" = n."billId"
  LEFT JOIN "vendors" v ON v."id" = b."vendorId""#;

fn from_row(r: &SqliteRow) -> AppResult<CreditNote> {
    Ok(CreditNote {
        id: r.try_get("id")?,
        note_number: r.try_get("noteNumber")?,
        kind: r.try_get("type")?,
        invoice_id: r.try_get("invoiceId")?,
        bill_id: r.try_get("billId")?,
        date: r.try_get("date")?,
        reason: r.try_get("reason")?,
        amount: Money::from_cents(r.try_get("amount")?),
        account_id: r.try_get("accountId")?,
        status: r.try_get("status")?,
        journal_entry_id: r.try_get("journalEntryId")?,
        created_by_id: r.try_get("createdById")?,
        tax_code_id: r.try_get("taxCodeId")?,
        tax_amount: Money::from_cents(r.try_get("taxAmount")?),
        created_at: r.try_get("createdAt")?,
        updated_at: r.try_get("updatedAt")?,
        document_number: r.try_get("documentNumber")?,
        party_name: r.try_get("partyName")?,
        currency: r.try_get("currency")?,
    })
}

fn noun(kind: &str) -> &'static str {
    if kind == AR {
        "Credit note"
    } else {
        "Debit note"
    }
}

/// A note of `kind`, visible to `scope` (credit notes follow the invoice's visibility).
pub async fn find_one(conn: &mut SqliteConnection, kind: &str, id: &str, scope: &Scope) -> AppResult<CreditNote> {
    let row = sqlx::query(&format!(r#"{SELECT} WHERE n."id" = ?1 AND n."type" = ?2"#))
        .bind(id)
        .bind(kind)
        .fetch_optional(&mut *conn)
        .await?;
    let note = match row {
        Some(r) => from_row(&r)?,
        None => return Err(AppError::NotFound(format!("{} {id} not found", noun(kind)))),
    };
    if let Some(iid) = &note.invoice_id {
        // Someone who cannot see the invoice cannot see its credit notes either.
        invoices::find_one(conn, iid, scope).await?;
    }
    Ok(note)
}

/// Newest first. Credit notes are limited to the invoices `scope` can see.
pub async fn find_all(db: &Db, kind: &str, scope: &Scope) -> AppResult<Vec<CreditNote>> {
    let own_only = kind == AR && !scope.sees_all;
    let sql = format!(
        r#"{SELECT} WHERE n."type" = ?1 AND (?2 = 0 OR i."createdById" IS ?3) ORDER BY n."date" DESC, n.rowid DESC"#
    );
    let rows = sqlx::query(&sql).bind(kind).bind(own_only).bind(&scope.user_id).fetch_all(db.read()).await?;
    rows.iter().map(from_row).collect()
}

pub struct CreateNote {
    pub kind: String,
    /// The invoice (AR) or bill (AP) the note reduces.
    pub document_id: String,
    pub date: String,
    pub reason: String,
    pub amount: Money,
    pub account_id: String,
    /// D26: optional tax code; `amount` includes the tax.
    pub tax_code_id: Option<String>,
}

/// What the note is against: its number, date, status and what is still owed on it.
struct Target {
    number: String,
    date: String,
    status: String,
    total: Money,
    paid: Money,
    credited: Money,
    outstanding: Money,
    /// D30: the document's rate, credits so far as booked, and what is owed as booked.
    rate: i64,
    base_credited: Money,
    base_outstanding: Money,
}

async fn target(conn: &mut SqliteConnection, kind: &str, id: &str, scope: &Scope) -> AppResult<Target> {
    if kind == AR {
        let inv = invoices::find_one(conn, id, scope).await?;
        Ok(Target {
            outstanding: inv.outstanding()?,
            base_outstanding: inv.base_outstanding()?,
            number: inv.invoice_number,
            date: inv.date,
            status: inv.status,
            total: inv.total_amount,
            paid: inv.amount_paid,
            credited: inv.amount_credited,
            rate: inv.exchange_rate,
            base_credited: inv.base_amount_credited,
        })
    } else {
        let b = bills::find_one(conn, id).await?;
        Ok(Target {
            outstanding: b.outstanding()?,
            base_outstanding: b.base_outstanding()?,
            number: b.bill_number,
            date: b.date,
            status: b.status,
            total: b.total_amount,
            paid: b.amount_paid,
            credited: b.amount_credited,
            rate: b.exchange_rate,
            base_credited: b.base_amount_credited,
        })
    }
}

async fn set_credited(
    tx: &mut database::Tx,
    kind: &str,
    id: &str,
    t: &Target,
    credited: Money,
    base_credited: Money,
) -> AppResult<()> {
    let status = invoices::settled_status(&t.status, t.total, t.paid, credited)?;
    if kind == AR {
        invoices::set_credited(tx, id, credited, base_credited, status).await
    } else {
        bills::set_credited(tx, id, credited, base_credited, status).await
    }
}

pub async fn create(db: &Db, dto: CreateNote, scope: &Scope) -> AppResult<CreditNote> {
    let kind = dto.kind.as_str();
    let (doc, control_code, prefix, source_type) =
        if kind == AR { ("invoice", "1100", "CN", "CREDIT_NOTE") } else { ("bill", "2000", "DN", "DEBIT_NOTE") };
    if dto.amount <= Money::ZERO {
        return Err(AppError::bad_request("amount must be greater than zero"));
    }
    let mut tx = db.begin().await?;
    let t = target(&mut tx, kind, &dto.document_id, scope).await?;
    match t.status.as_str() {
        invoices::POSTED => {}
        invoices::PAID => {
            return Err(AppError::bad_request(format!(
            "This {doc} is fully settled — nothing is left to reduce. Void a payment first if the credit replaces it."
        )))
        }
        other => {
            return Err(AppError::bad_request(format!("Only a posted {doc} can be reduced — this one is {other}")))
        }
    }
    if dto.amount > t.outstanding {
        return Err(AppError::bad_request(format!(
            "{} exceeds the outstanding balance of {}",
            noun(kind),
            t.outstanding
        )));
    }
    if dto.date < t.date {
        return Err(AppError::bad_request(format!("{} cannot be dated before its {doc} ({})", noun(kind), t.date)));
    }
    let account = accounts::find_one(&mut tx, &dto.account_id).await?;
    let allowed = if kind == AR {
        account.account_type == AccountType::Revenue
    } else {
        matches!(account.account_type, AccountType::Expense | AccountType::Asset)
    };
    if !allowed || !account.is_active || account.code == control_code {
        return Err(AppError::bad_request(if kind == AR {
            "Choose an active revenue account for a credit note"
        } else {
            "Choose an active expense or asset account for a debit note"
        }));
    }
    // D33: a debit note reduces the bill's own payable account.
    let control = if kind == AR {
        accounts::find_by_code(&mut tx, control_code).await?
    } else {
        let payable: Option<String> = sqlx::query_scalar(r#"SELECT "payableAccountId" FROM "bills" WHERE "id" = ?1"#)
            .bind(&dto.document_id)
            .fetch_one(&mut *tx)
            .await?;
        crate::bills::payable_account(&mut tx, payable.as_deref()).await?
    };
    // D26: the tax inside the gross amount goes back to the code's tax account.
    let (parts, tax) = match &dto.tax_code_id {
        Some(code) => crate::tax::line_tax(&mut tx, code, dto.amount, true).await?,
        None => (Vec::new(), Money::ZERO),
    };
    // D30: in the home currency at the document's own rate, so no exchange difference arises;
    // a note that clears the document takes exactly what is left as booked.
    let base_amount =
        if dto.amount == t.outstanding { t.base_outstanding } else { crate::fx::to_base(dto.amount, t.rate)? };
    let mut base_parts = Vec::new();
    for p in &parts {
        base_parts.push(crate::tax::TaxPart { amount: crate::fx::to_base(p.amount, t.rate)?, ..p.clone() });
    }
    let base_tax = base_parts.iter().try_fold(Money::ZERO, |s, p| s.checked_add(p.amount)).ok_or_else(overflow)?;
    let net = base_amount.checked_sub(base_tax).ok_or_else(overflow)?;
    let number = counters::next_formatted(&mut tx, prefix, 6).await?;
    let id = new_id();
    let mut lines = if kind == AR {
        vec![
            NewLine { account_id: account.id.clone(), debit: net, credit: Money::ZERO, description: None },
            NewLine { account_id: control.id, debit: Money::ZERO, credit: base_amount, description: None },
        ]
    } else {
        vec![
            NewLine { account_id: control.id, debit: base_amount, credit: Money::ZERO, description: None },
            NewLine { account_id: account.id.clone(), debit: Money::ZERO, credit: net, description: None },
        ]
    };
    // D27: each part of the tax back to its own code's account.
    for (account_id, part) in crate::tax::by_account(&mut tx, base_parts, kind == AR).await? {
        let (debit, credit) = if kind == AR { (part, Money::ZERO) } else { (Money::ZERO, part) };
        lines.push(NewLine { account_id, debit, credit, description: Some("Tax".into()) });
    }
    let entry = entries::post(
        &mut tx,
        NewEntry {
            date: dto.date.clone(),
            description: format!("{} {number} for {}: {}", noun(kind), t.number, dto.reason),
            reference: Some(number.clone()),
            source_type: Some(source_type.into()),
            source_id: Some(id.clone()),
            lines,
        },
    )
    .await?;
    let credited = t.credited.checked_add(dto.amount).ok_or_else(overflow)?;
    let base_credited = t.base_credited.checked_add(base_amount).ok_or_else(overflow)?;
    set_credited(&mut tx, kind, &dto.document_id, &t, credited, base_credited).await?;

    let now = now_iso();
    let (invoice_id, bill_id) =
        if kind == AR { (Some(dto.document_id.clone()), None) } else { (None, Some(dto.document_id.clone())) };
    sqlx::query(
        r#"INSERT INTO "credit_notes" ("id","noteNumber","type","invoiceId","billId","date","reason","amount","accountId",
             "status","journalEntryId","createdById","createdAt","updatedAt","taxCodeId","taxAmount","baseAmount")
           VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,'POSTED',?10,?11,?12,?12,?13,?14,?15)"#,
    )
    .bind(&id)
    .bind(&number)
    .bind(kind)
    .bind(invoice_id)
    .bind(bill_id)
    .bind(&dto.date)
    .bind(&dto.reason)
    .bind(dto.amount.cents())
    .bind(&account.id)
    .bind(&entry.id)
    .bind(&scope.user_id)
    .bind(&now)
    .bind(&dto.tax_code_id)
    .bind(tax.cents())
    .bind(base_amount.cents())
    .execute(&mut *tx)
    .await?;
    crate::tax::save_parts(&mut tx, "CREDIT_NOTE", &id, &parts).await?;
    let note = find_one(&mut tx, kind, &id, scope).await?;
    tx.commit().await?;
    Ok(note)
}

/// Reverses the note's entry (dated `today`) and puts the amount back on the document.
pub async fn void(db: &Db, kind: &str, id: &str, scope: &Scope, today: &str) -> AppResult<CreditNote> {
    let mut tx = db.begin().await?;
    let note = find_one(&mut tx, kind, id, scope).await?;
    if note.status == "VOID" {
        return Err(AppError::bad_request(format!("{} is already voided", noun(kind))));
    }
    if let Some(je) = &note.journal_entry_id {
        entries::reverse(&mut tx, je, today).await?;
    }
    let doc_id = note.invoice_id.as_deref().or(note.bill_id.as_deref()).unwrap_or_default().to_string();
    let t = target(&mut tx, kind, &doc_id, scope).await?;
    let credited = t.credited.checked_sub(note.amount).ok_or_else(overflow)?.max(Money::ZERO);
    let booked: i64 = sqlx::query_scalar(r#"SELECT "baseAmount" FROM "credit_notes" WHERE "id" = ?1"#)
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
    let base_credited = t.base_credited.checked_sub(Money::from_cents(booked)).ok_or_else(overflow)?.max(Money::ZERO);
    set_credited(&mut tx, kind, &doc_id, &t, credited, base_credited).await?;
    sqlx::query(r#"UPDATE "credit_notes" SET "status" = 'VOID', "updatedAt" = ?1 WHERE "id" = ?2"#)
        .bind(now_iso())
        .bind(id)
        .execute(&mut *tx)
        .await?;
    let out = find_one(&mut tx, kind, id, scope).await?;
    tx.commit().await?;
    Ok(out)
}
