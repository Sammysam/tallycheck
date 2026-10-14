use crate::bills::{self, BillLineInput, CreateBill};
use crate::expenses::{self, CreateExpense};
use crate::invoices::{self, CreateInvoice, InvoicePatch, LineInput, Scope, View};
use crate::parties::{self, PartyFields, PartyKind};
use crate::payments::{self, CreatePayment};
use app_core::money::Money;
use app_core::{AppError, AppResult, ErrorMessage};
use database::Db;
use ledger::accounts::{self, AccountType, CreateAccount};
use std::collections::HashMap;

struct Shop {
    _dir: tempfile::TempDir,
    db: Db,
    acct: HashMap<&'static str, String>,
    customer: String,
    vendor: String,
}

const ADMIN: fn() -> Scope = || Scope { user_id: Some("admin".into()), sees_all: true };
const TODAY: &str = "2026-10-14";

fn m(s: &str) -> Money {
    Money::parse_decimal(s).unwrap_or_default()
}

async fn shop() -> AppResult<Shop> {
    let dir = tempfile::tempdir().map_err(AppError::internal)?;
    let db = Db::open(&dir.path().join("t.db")).await?;
    let mut acct = HashMap::new();
    for (code, t) in [
        ("1000", AccountType::Asset),
        ("1100", AccountType::Asset),
        ("2000", AccountType::Liability),
        ("4000", AccountType::Revenue),
        ("6000", AccountType::Expense),
    ] {
        let a = accounts::create(
            &db,
            CreateAccount {
                code: code.into(),
                name: code.into(),
                account_type: t,
                subtype: None,
                description: None,
                parent_id: None,
            },
        )
        .await?;
        acct.insert(code, a.id);
    }
    // Invoices record who raised them (foreign key to users), so the test users must exist.
    for u in ["admin", "sales1"] {
        sqlx::query(r#"INSERT INTO "users" ("id","username","passwordHash") VALUES (?1, ?1, 'x')"#)
            .bind(u)
            .execute(db.write())
            .await?;
    }
    let customer =
        parties::create(&db, PartyKind::Customer, PartyFields { name: Some("Acme".into()), ..Default::default() })
            .await?
            .id;
    let vendor =
        parties::create(&db, PartyKind::Vendor, PartyFields { name: Some("Paper Co".into()), ..Default::default() })
            .await?
            .id;
    Ok(Shop { _dir: dir, db, acct, customer, vendor })
}

impl Shop {
    async fn balance(&self, code: &str) -> AppResult<Money> {
        let mut c = self.db.read().acquire().await?;
        Ok(ledger::entries::account_balance(&mut c, &self.acct[code], None, None).await?.balance)
    }

    fn invoice(&self, lines: Vec<(&str, &str, bool)>) -> CreateInvoice {
        CreateInvoice {
            customer_id: Some(self.customer.clone()),
            date: "2026-04-01".into(),
            payment_terms: Some("NET_30".into()),
            lines: lines
                .into_iter()
                .map(|(q, p, with_account)| LineInput {
                    description: "work".into(),
                    quantity: Some(m(q)),
                    unit_price: Some(m(p)),
                    account_id: with_account.then(|| self.acct["4000"].clone()),
                })
                .collect(),
            ..Default::default()
        }
    }

    fn pay(&self, kind: &str, invoice: Option<&str>, bill: Option<&str>, amount: &str) -> CreatePayment {
        CreatePayment {
            kind: kind.into(),
            invoice_id: invoice.map(str::to_string),
            bill_id: bill.map(str::to_string),
            customer_id: None,
            vendor_id: None,
            date: "2026-04-25".into(),
            amount: m(amount),
            payment_method: "BANK_TRANSFER".into(),
            payment_account_id: self.acct["1000"].clone(),
            notes: None,
        }
    }
}

fn bad_request(r: &AppResult<impl std::fmt::Debug>) -> Option<String> {
    match r {
        Err(AppError::BadRequest(ErrorMessage::Text(s))) => Some(s.clone()),
        _ => None,
    }
}

#[tokio::test]
async fn invoice_lifecycle_posts_pays_and_balances() -> AppResult<()> {
    let s = shop().await?;
    let inv =
        invoices::create(&s.db, s.invoice(vec![("2.5", "1000.25", true), ("1", "0", false)]), Some("admin")).await?;
    assert_eq!(inv.invoice_number, "INV-9000");
    assert_eq!(inv.total_amount, m("2500.63"));
    assert_eq!(inv.due_date.as_deref(), Some("2026-05-01"));
    let created = inv.to_json(View::Created);
    assert!(created.get("customer").is_none());
    assert_eq!(created["lines"][0]["unitPrice"], serde_json::json!(1000.25));
    assert_eq!(created["lines"][0]["quantity"], "2.50");

    let blank = invoices::post(&s.db, &inv.id, &ADMIN()).await;
    assert_eq!(
        bad_request(&blank).as_deref(),
        Some("Choose a revenue account for every line before posting — 1 still blank")
    );
    assert_eq!(s.balance("1100").await?, Money::ZERO, "a refused post leaves no trace");

    let patch = InvoicePatch {
        lines: Some(Some(s.invoice(vec![("2.5", "1000.25", true), ("12", "15.5", true)]).lines)),
        ..Default::default()
    };
    let (inv, view) = invoices::update(&s.db, &inv.id, patch, &ADMIN()).await?;
    assert_eq!(view, View::UpdatedLines);
    assert_eq!(inv.total_amount, m("2686.63"));

    let posted = invoices::post(&s.db, &inv.id, &ADMIN()).await?;
    assert_eq!(posted.status, "POSTED");
    assert_eq!(s.balance("1100").await?, m("2686.63"));
    assert_eq!(s.balance("4000").await?, m("2686.63"));
    // Retrying the post is refused, so it cannot double-post.
    let again = invoices::post(&s.db, &inv.id, &ADMIN()).await;
    assert_eq!(bad_request(&again).as_deref(), Some("Invoice is already POSTED"));
    assert_eq!(s.balance("1100").await?, m("2686.63"));

    payments::create(&s.db, s.pay("AR", Some(&inv.id), None, "1000")).await?;
    let too_much = payments::create(&s.db, s.pay("AR", Some(&inv.id), None, "99999")).await;
    assert_eq!(bad_request(&too_much).as_deref(), Some("Payment exceeds the outstanding balance of 1686.63"));
    payments::create(&s.db, s.pay("AR", Some(&inv.id), None, "1686.63")).await?;
    let mut c = s.db.read().acquire().await?;
    let paid = invoices::find_one(&mut c, &inv.id, &ADMIN()).await?;
    drop(c);
    assert_eq!((paid.status.as_str(), paid.amount_paid), ("PAID", m("2686.63")));
    assert_eq!(s.balance("1100").await?, Money::ZERO);
    assert_eq!(s.balance("1000").await?, m("2686.63"));
    let dup = payments::create(&s.db, s.pay("AR", Some(&inv.id), None, "1")).await;
    assert_eq!(bad_request(&dup).as_deref(), Some("Invoice is already fully paid"));
    Ok(())
}

#[tokio::test]
async fn void_and_unpost_reverse_the_entry_and_ar_returns_to_zero() -> AppResult<()> {
    let s = shop().await?;
    let inv = invoices::create(&s.db, s.invoice(vec![("1", "500", true)]), Some("admin")).await?;
    invoices::post(&s.db, &inv.id, &ADMIN()).await?;
    let draft = invoices::unpost(&s.db, &inv.id, &ADMIN(), TODAY).await?;
    assert_eq!((draft.status.as_str(), draft.journal_entry_id.as_deref()), ("DRAFT", None));
    assert_eq!(s.balance("1100").await?, Money::ZERO, "D1: original and reversal cancel");
    invoices::post(&s.db, &inv.id, &ADMIN()).await?;
    let voided = invoices::change_status(&s.db, &inv.id, "VOID", &ADMIN(), TODAY).await?;
    assert_eq!(voided.status, "VOID");
    assert!(voided.journal_entry_id.is_some(), "the voided invoice keeps its entry id, as the reference does");
    assert_eq!(s.balance("1100").await?, Money::ZERO);
    assert_eq!(s.balance("4000").await?, Money::ZERO);
    let del = invoices::remove(&s.db, &inv.id, &ADMIN()).await;
    assert_eq!(bad_request(&del).as_deref(), Some("A voided invoice is kept for the record and cannot be deleted"));
    let pay = payments::create(&s.db, s.pay("AR", Some(&inv.id), None, "1")).await;
    assert_eq!(bad_request(&pay).as_deref(), Some("Cannot pay a voided invoice"));

    let copy = invoices::duplicate(&s.db, &inv.id, &ADMIN(), TODAY).await?;
    assert_eq!((copy.status.as_str(), copy.date.as_str()), ("DRAFT", TODAY));
    assert_eq!(copy.bill_to_name.as_deref(), Some("Acme"), "falls back to the customer name");
    invoices::remove(&s.db, &copy.id, &ADMIN()).await?;
    Ok(())
}

#[tokio::test]
async fn missing_control_account_rolls_back_everything() -> AppResult<()> {
    let s = shop().await?;
    let inv = invoices::create(&s.db, s.invoice(vec![("1", "100", true)]), Some("admin")).await?;
    // Rename 1100 away so posting cannot find Accounts Receivable.
    sqlx::query(r#"UPDATE "accounts" SET "code" = '1199' WHERE "code" = '1100'"#).execute(s.db.write()).await?;
    let r = invoices::post(&s.db, &inv.id, &ADMIN()).await;
    assert!(matches!(r, Err(AppError::NotFound(ref m)) if m == "Account code 1100 not found"));
    let mut c = s.db.read().acquire().await?;
    let still = invoices::find_one(&mut c, &inv.id, &ADMIN()).await?;
    assert_eq!(still.status, "DRAFT");
    let entries: i64 = sqlx::query_scalar(r#"SELECT COUNT(*) FROM "journal_entries""#).fetch_one(&mut *c).await?;
    assert_eq!(entries, 0);
    Ok(())
}

#[tokio::test]
async fn bills_expenses_and_ap_payments_follow_the_reference() -> AppResult<()> {
    let s = shop().await?;
    let exp = s.acct["6000"].clone();
    let line = |d: &str, q: &str, p: &str| BillLineInput {
        description: d.into(),
        quantity: m(q),
        unit_price: m(p),
        account_id: exp.clone(),
    };
    let bill = bills::create(
        &s.db,
        CreateBill {
            vendor_id: s.vendor.clone(),
            date: "2026-04-03".into(),
            due_date: None,
            notes: None,
            lines: vec![line("Paper", "3", "12.5"), line("Ink", "1", "20")],
        },
    )
    .await?;
    assert_eq!((bill.bill_number.as_str(), bill.total_amount), ("BILL-000001", m("57.5")));
    bills::post(&s.db, &bill.id).await?;
    assert_eq!(s.balance("2000").await?, m("57.5"));
    assert_eq!(s.balance("6000").await?, m("57.5"));

    payments::create(&s.db, s.pay("AP", None, Some(&bill.id), "10")).await?;
    let mut c = s.db.read().acquire().await?;
    assert_eq!(bills::find_one(&mut c, &bill.id).await?.status, "POSTED", "partial AP payments do not accumulate");
    drop(c);
    payments::create(&s.db, s.pay("AP", None, Some(&bill.id), "57.5")).await?;
    let mut c = s.db.read().acquire().await?;
    assert_eq!(bills::find_one(&mut c, &bill.id).await?.status, "PAID");
    drop(c);
    let v = bills::void(&s.db, &bill.id, TODAY).await;
    assert_eq!(bad_request(&v).as_deref(), Some("Cannot void a paid bill — reverse the payment first"));

    let e = expenses::create(
        &s.db,
        CreateExpense {
            date: "2026-04-08".into(),
            description: "Taxi".into(),
            amount: m("1500.75"),
            expense_account_id: exp.clone(),
            payment_account_id: s.acct["1000"].clone(),
            receipt_url: None,
        },
    )
    .await?;
    assert_eq!(e.expense_number, "EXP-000001");
    expenses::post(&s.db, &e.id).await?;
    let again = expenses::post(&s.db, &e.id).await;
    assert_eq!(bad_request(&again).as_deref(), Some("Expense is already POSTED"));
    expenses::void(&s.db, &e.id, TODAY).await?;
    // Expense posted and voided: only the bill remains on 6000.
    assert_eq!(s.balance("6000").await?, m("57.5"));
    Ok(())
}

#[tokio::test]
async fn users_without_invoices_all_only_see_their_own() -> AppResult<()> {
    let s = shop().await?;
    let admin_inv = invoices::create(&s.db, s.invoice(vec![("1", "1", true)]), Some("admin")).await?;
    let sales = Scope { user_id: Some("sales1".into()), sees_all: false };
    let mut own = s.invoice(vec![("1", "75", true)]);
    own.customer_id = None;
    own.bill_to_name = Some("Lead".into());
    invoices::create(&s.db, own, Some("sales1")).await?;

    let q = invoices::InvoiceQuery { page: 1, limit: 20, ..Default::default() };
    let (mine, total) = invoices::find_all(&s.db, &q, &sales).await?;
    assert_eq!((mine.len(), total), (1, 1));
    let (all, total) = invoices::find_all(&s.db, &q, &ADMIN()).await?;
    assert_eq!((all.len(), total), (2, 2));
    let mut c = s.db.read().acquire().await?;
    let hidden = invoices::find_one(&mut c, &admin_inv.id, &sales).await;
    assert!(matches!(hidden, Err(AppError::NotFound(_))), "hidden behind a 404, not a 403");
    Ok(())
}

#[tokio::test]
async fn search_matches_number_po_bill_to_and_customer_name() -> AppResult<()> {
    let s = shop().await?;
    let mut walk = s.invoice(vec![("1", "1", true)]);
    walk.customer_id = None;
    walk.bill_to_name = Some("Walk-in Client".into());
    walk.po_number = Some("PO-77".into());
    invoices::create(&s.db, walk, Some("admin")).await?;
    invoices::create(&s.db, s.invoice(vec![("1", "1", true)]), Some("admin")).await?;
    let find = |q: &str| invoices::InvoiceQuery { page: 1, limit: 20, q: Some(q.into()), ..Default::default() };
    assert_eq!(invoices::find_all(&s.db, &find("walk"), &ADMIN()).await?.1, 1);
    assert_eq!(invoices::find_all(&s.db, &find("ACME"), &ADMIN()).await?.1, 1);
    assert_eq!(invoices::find_all(&s.db, &find("po-77"), &ADMIN()).await?.1, 1);
    assert_eq!(invoices::find_all(&s.db, &find("INV-900"), &ADMIN()).await?.1, 2);
    Ok(())
}

#[tokio::test]
async fn line_amounts_use_exact_half_up_rounding() -> AppResult<()> {
    let s = shop().await?;
    let inv = invoices::create(&s.db, s.invoice(vec![("0.29", "0.5", true)]), Some("admin")).await?;
    assert_eq!(inv.lines[0].amount, m("0.15"), "D4: the reference's float rounding gives 0.14");
    Ok(())
}

#[tokio::test]
async fn customers_in_use_cannot_be_deleted() -> AppResult<()> {
    let s = shop().await?;
    invoices::create(&s.db, s.invoice(vec![("1", "1", true)]), Some("admin")).await?;
    let r = parties::remove(&s.db, PartyKind::Customer, &s.customer).await;
    assert_eq!(
        bad_request(&r).as_deref(),
        Some("Cannot delete this customer because it is linked to existing invoices. Void or remove those invoices first.")
    );
    Ok(())
}
