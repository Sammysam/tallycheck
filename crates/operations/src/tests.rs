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
                currency: None,
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
                    tax_code_id: None,
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
            withholding_tax_code_id: None,
            withholding_amount: Money::ZERO,
            exchange_rate: None,
            payable_account_id: None,
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
        tax_code_id: None,
    };
    let bill = bills::create(
        &s.db,
        CreateBill {
            vendor_id: s.vendor.clone(),
            date: "2026-04-03".into(),
            due_date: None,
            notes: None,
            prices_include_tax: false,
            currency: None,
            exchange_rate: None,
            payable_account_id: None,
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
    let part = bills::find_one(&mut c, &bill.id).await?;
    drop(c);
    assert_eq!((part.status.as_str(), part.amount_paid), ("POSTED", m("10")), "D13: part-payments accumulate");
    let v = bills::void(&s.db, &bill.id, TODAY).await;
    assert_eq!(bad_request(&v).as_deref(), Some("Cannot void a part-paid bill — reverse the payments first"), "D15");
    let too_much = payments::create(&s.db, s.pay("AP", None, Some(&bill.id), "57.5")).await;
    assert_eq!(bad_request(&too_much).as_deref(), Some("Payment exceeds the outstanding balance of 47.50"));
    payments::create(&s.db, s.pay("AP", None, Some(&bill.id), "47.5")).await?;
    let mut c = s.db.read().acquire().await?;
    assert_eq!(bills::find_one(&mut c, &bill.id).await?.status, "PAID", "D13: paid once nothing is outstanding");
    drop(c);
    assert_eq!(s.balance("2000").await?, Money::ZERO);
    let v = bills::void(&s.db, &bill.id, TODAY).await;
    assert_eq!(bad_request(&v).as_deref(), Some("Cannot void a paid bill — reverse the payment first"));
    let again = payments::create(&s.db, s.pay("AP", None, Some(&bill.id), "1")).await;
    assert_eq!(bad_request(&again).as_deref(), Some("Bill is already fully paid"));

    let draft = bills::create(
        &s.db,
        CreateBill {
            vendor_id: s.vendor.clone(),
            date: "2026-04-07".into(),
            due_date: None,
            notes: None,
            prices_include_tax: false,
            currency: None,
            exchange_rate: None,
            payable_account_id: None,
            lines: vec![line("Unposted", "1", "9")],
        },
    )
    .await?;
    let r = payments::create(&s.db, s.pay("AP", None, Some(&draft.id), "9")).await;
    assert_eq!(bad_request(&r).as_deref(), Some("Post the bill before recording a payment against it"), "D14");

    let e = expenses::create(
        &s.db,
        CreateExpense {
            date: "2026-04-08".into(),
            description: "Taxi".into(),
            amount: m("1500.75"),
            expense_account_id: exp.clone(),
            payment_account_id: s.acct["1000"].clone(),
            receipt_url: None,
            tax_code_id: None,
        },
    )
    .await?;
    assert_eq!(e.expense_number, "EXP-000001");
    expenses::post(&s.db, &e.id).await?;
    let again = expenses::post(&s.db, &e.id).await;
    assert_eq!(bad_request(&again).as_deref(), Some("Expense is already POSTED"));
    expenses::void(&s.db, &e.id, TODAY).await?;
    // Expense posted and voided: only the posted bill remains on 6000.
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

async fn invoice_state(s: &Shop, id: &str) -> AppResult<(String, Money)> {
    let mut c = s.db.read().acquire().await?;
    let inv = invoices::find_one(&mut c, id, &ADMIN()).await?;
    Ok((inv.status, inv.amount_paid))
}

#[tokio::test]
async fn voiding_a_payment_reverses_it_and_reopens_the_invoice_and_bill() -> AppResult<()> {
    let s = shop().await?;
    let inv = invoices::create(&s.db, s.invoice(vec![("1", "1000", true)]), Some("admin")).await?;
    invoices::post(&s.db, &inv.id, &ADMIN()).await?;
    let first = payments::create(&s.db, s.pay("AR", Some(&inv.id), None, "400")).await?;
    let second = payments::create(&s.db, s.pay("AR", Some(&inv.id), None, "600")).await?;
    assert_eq!(invoice_state(&s, &inv.id).await?, ("PAID".into(), m("1000")));
    assert_eq!(first.status, "POSTED");

    let voided = payments::void(&s.db, &first.id, TODAY).await?;
    assert_eq!(voided.status, "VOID");
    assert_eq!(invoice_state(&s, &inv.id).await?, ("POSTED".into(), m("600")), "PAID returns to POSTED");
    assert_eq!(s.balance("1100").await?, m("400"), "the voided amount is owed again");
    assert_eq!(s.balance("1000").await?, m("600"));
    let again = payments::void(&s.db, &first.id, TODAY).await;
    assert_eq!(bad_request(&again).as_deref(), Some("Payment is already voided"));

    let blocked = invoices::void(&s.db, &inv.id, &ADMIN(), TODAY).await;
    assert!(bad_request(&blocked).is_some(), "still part-paid by the second payment");
    payments::void(&s.db, &second.id, TODAY).await?;
    assert_eq!(invoice_state(&s, &inv.id).await?, ("POSTED".into(), Money::ZERO));
    invoices::void(&s.db, &inv.id, &ADMIN(), TODAY).await?;
    assert_eq!(s.balance("1100").await?, Money::ZERO);
    assert_eq!(s.balance("1000").await?, Money::ZERO);

    let bill = bills::create(
        &s.db,
        CreateBill {
            vendor_id: s.vendor.clone(),
            date: "2026-04-03".into(),
            due_date: None,
            notes: None,
            prices_include_tax: false,
            currency: None,
            exchange_rate: None,
            payable_account_id: None,
            lines: vec![BillLineInput {
                description: "Paper".into(),
                quantity: m("1"),
                unit_price: m("57.5"),
                account_id: s.acct["6000"].clone(),
                tax_code_id: None,
            }],
        },
    )
    .await?;
    bills::post(&s.db, &bill.id).await?;
    let paid = payments::create(&s.db, s.pay("AP", None, Some(&bill.id), "57.5")).await?;
    payments::void(&s.db, &paid.id, TODAY).await?;
    let mut c = s.db.read().acquire().await?;
    let reopened = bills::find_one(&mut c, &bill.id).await?;
    drop(c);
    assert_eq!((reopened.status.as_str(), reopened.amount_paid), ("POSTED", Money::ZERO));
    assert_eq!(s.balance("2000").await?, m("57.5"));
    bills::void(&s.db, &bill.id, TODAY).await?;
    Ok(())
}

#[tokio::test]
async fn nothing_posts_on_or_before_the_books_lock_date() -> AppResult<()> {
    let s = shop().await?;
    let inv = invoices::create(&s.db, s.invoice(vec![("1", "100", true)]), Some("admin")).await?;
    invoices::post(&s.db, &inv.id, &ADMIN()).await?;
    let pay = payments::create(&s.db, s.pay("AR", Some(&inv.id), None, "40")).await?;

    let lock = |d: Option<&'static str>| {
        let db = s.db.clone();
        async move {
            let mut tx = db.begin().await?;
            ledger::entries::set_locked_through(&mut tx, d).await?;
            tx.commit().await?;
            AppResult::Ok(())
        }
    };
    lock(Some("2026-04-30")).await?;
    let locked = "The books are locked up to and including 2026-04-30. Use a later date, or ask an administrator to change the lock date.";

    // On the lock date itself and before it: refused, and nothing is half-written.
    let mut on_lock_day = s.pay("AR", Some(&inv.id), None, "10");
    on_lock_day.date = "2026-04-30".into();
    assert_eq!(bad_request(&payments::create(&s.db, on_lock_day).await).as_deref(), Some(locked));
    assert_eq!(invoice_state(&s, &inv.id).await?, ("POSTED".into(), m("40")));
    let draft = invoices::create(&s.db, s.invoice(vec![("1", "5", true)]), Some("admin")).await?;
    assert_eq!(bad_request(&invoices::post(&s.db, &draft.id, &ADMIN()).await).as_deref(), Some(locked));

    // After it: fine. Corrections of locked documents are dated today, in the open period.
    let mut may = s.pay("AR", Some(&inv.id), None, "10");
    may.date = "2026-05-01".into();
    payments::create(&s.db, may).await?;
    payments::void(&s.db, &pay.id, TODAY).await?;
    assert_eq!(invoice_state(&s, &inv.id).await?, ("POSTED".into(), m("10")));

    lock(None).await?;
    invoices::post(&s.db, &draft.id, &ADMIN()).await?;
    Ok(())
}

fn note(kind: &str, doc: &str, amount: &str, account: &str) -> crate::credit_notes::CreateNote {
    crate::credit_notes::CreateNote {
        kind: kind.into(),
        document_id: doc.into(),
        date: "2026-04-20".into(),
        reason: "Discount agreed".into(),
        amount: m(amount),
        account_id: account.into(),
        tax_code_id: None,
    }
}

#[tokio::test]
async fn credit_notes_reduce_what_the_customer_owes_and_can_be_voided() -> AppResult<()> {
    use crate::credit_notes;
    let s = shop().await?;
    let revenue = s.acct["4000"].clone();
    let inv = invoices::create(&s.db, s.invoice(vec![("1", "1000", true)]), Some("admin")).await?;

    let draft = credit_notes::create(&s.db, note("AR", &inv.id, "100", &revenue), &ADMIN()).await;
    assert_eq!(bad_request(&draft).as_deref(), Some("Only a posted invoice can be reduced — this one is DRAFT"));
    invoices::post(&s.db, &inv.id, &ADMIN()).await?;
    payments::create(&s.db, s.pay("AR", Some(&inv.id), None, "600")).await?;

    let too_much = credit_notes::create(&s.db, note("AR", &inv.id, "400.01", &revenue), &ADMIN()).await;
    assert_eq!(bad_request(&too_much).as_deref(), Some("Credit note exceeds the outstanding balance of 400.00"));
    let wrong_account = credit_notes::create(&s.db, note("AR", &inv.id, "50", &s.acct["1000"]), &ADMIN()).await;
    assert_eq!(bad_request(&wrong_account).as_deref(), Some("Choose an active revenue account for a credit note"));
    let mut early = note("AR", &inv.id, "50", &revenue);
    early.date = "2026-03-31".into();
    let early = credit_notes::create(&s.db, early, &ADMIN()).await;
    assert_eq!(bad_request(&early).as_deref(), Some("Credit note cannot be dated before its invoice (2026-04-01)"));

    let cn = credit_notes::create(&s.db, note("AR", &inv.id, "150", &revenue), &ADMIN()).await?;
    assert_eq!((cn.note_number.as_str(), cn.status.as_str()), ("CN-000001", "POSTED"));
    assert_eq!(cn.document_number.as_deref(), Some(inv.invoice_number.as_str()));
    assert_eq!(s.balance("1100").await?, m("250"), "1000 - 600 paid - 150 credited");
    assert_eq!(s.balance("4000").await?, m("850"), "revenue is reduced by the credit");
    let pay_more = payments::create(&s.db, s.pay("AR", Some(&inv.id), None, "250.01")).await;
    assert_eq!(bad_request(&pay_more).as_deref(), Some("Payment exceeds the outstanding balance of 250.00"));

    // The rest as a second credit note settles the invoice.
    let rest = credit_notes::create(&s.db, note("AR", &inv.id, "250", &revenue), &ADMIN()).await?;
    let mut c = s.db.read().acquire().await?;
    let settled = invoices::find_one(&mut c, &inv.id, &ADMIN()).await?;
    drop(c);
    assert_eq!((settled.status.as_str(), settled.amount_credited), ("PAID", m("400")));
    let none_left = credit_notes::create(&s.db, note("AR", &inv.id, "1", &revenue), &ADMIN()).await;
    assert!(bad_request(&none_left).unwrap_or_default().starts_with("This invoice is fully settled"));

    // Voiding a note puts its amount back and reopens the invoice.
    let voided = credit_notes::void(&s.db, "AR", &rest.id, &ADMIN(), TODAY).await?;
    assert_eq!(voided.status, "VOID");
    let mut c = s.db.read().acquire().await?;
    let reopened = invoices::find_one(&mut c, &inv.id, &ADMIN()).await?;
    drop(c);
    assert_eq!((reopened.status.as_str(), reopened.amount_credited), ("POSTED", m("150")));
    assert_eq!(s.balance("1100").await?, m("250"));
    let twice = credit_notes::void(&s.db, "AR", &rest.id, &ADMIN(), TODAY).await;
    assert_eq!(bad_request(&twice).as_deref(), Some("Credit note is already voided"));
    let blocked = invoices::void(&s.db, &inv.id, &ADMIN(), TODAY).await;
    assert!(bad_request(&blocked).is_some(), "an invoice with payments or credit notes is not voided");
    assert_eq!(credit_notes::find_all(&s.db, "AR", &ADMIN()).await?.len(), 2);
    Ok(())
}

#[tokio::test]
async fn debit_notes_reduce_what_is_owed_to_the_vendor() -> AppResult<()> {
    use crate::credit_notes;
    let s = shop().await?;
    let expense = s.acct["6000"].clone();
    let bill = bills::create(
        &s.db,
        CreateBill {
            vendor_id: s.vendor.clone(),
            date: "2026-04-03".into(),
            due_date: None,
            notes: None,
            prices_include_tax: false,
            currency: None,
            exchange_rate: None,
            payable_account_id: None,
            lines: vec![BillLineInput {
                description: "Paper".into(),
                quantity: m("1"),
                unit_price: m("300"),
                account_id: expense.clone(),
                tax_code_id: None,
            }],
        },
    )
    .await?;
    bills::post(&s.db, &bill.id).await?;
    let wrong = credit_notes::create(&s.db, note("AP", &bill.id, "10", &s.acct["4000"]), &ADMIN()).await;
    assert_eq!(bad_request(&wrong).as_deref(), Some("Choose an active expense or asset account for a debit note"));
    let dn = credit_notes::create(&s.db, note("AP", &bill.id, "100", &expense), &ADMIN()).await?;
    assert_eq!(dn.note_number, "DN-000001");
    assert_eq!(s.balance("2000").await?, m("200"));
    assert_eq!(s.balance("6000").await?, m("200"));
    payments::create(&s.db, s.pay("AP", None, Some(&bill.id), "200")).await?;
    let mut c = s.db.read().acquire().await?;
    let paid = bills::find_one(&mut c, &bill.id).await?;
    drop(c);
    assert_eq!(paid.status, "PAID", "payment plus debit note settle the bill");
    let v = bills::void(&s.db, &bill.id, TODAY).await;
    assert!(bad_request(&v).is_some());
    Ok(())
}

#[tokio::test]
async fn customer_statement_runs_invoices_payments_and_credits() -> AppResult<()> {
    let s = shop().await?;
    let revenue = s.acct["4000"].clone();
    let main = invoices::create(&s.db, s.invoice(vec![("1", "1000", true)]), Some("admin")).await?;
    invoices::post(&s.db, &main.id, &ADMIN()).await?;
    let _draft = invoices::create(&s.db, s.invoice(vec![("1", "500", true)]), Some("admin")).await?;
    let voided = invoices::create(&s.db, s.invoice(vec![("1", "200", true)]), Some("admin")).await?;
    invoices::post(&s.db, &voided.id, &ADMIN()).await?;
    invoices::void(&s.db, &voided.id, &ADMIN(), TODAY).await?;
    crate::credit_notes::create(&s.db, note("AR", &main.id, "100", &revenue), &ADMIN()).await?;
    payments::create(&s.db, s.pay("AR", Some(&main.id), None, "400")).await?;
    let mistake = payments::create(&s.db, s.pay("AR", Some(&main.id), None, "1")).await?;
    payments::void(&s.db, &mistake.id, TODAY).await?;

    let mut c = s.db.read().acquire().await?;
    let all = crate::statements::customer_statement(&mut c, &s.customer, None, None, &ADMIN()).await?;
    let kinds: Vec<(&str, f64)> = all["lines"]
        .as_array()
        .map(|a| a.iter().map(|l| (l["type"].as_str().unwrap_or(""), l["balance"].as_f64().unwrap_or(0.0))).collect())
        .unwrap_or_default();
    assert_eq!(
        kinds,
        vec![("INVOICE", 1000.0), ("CREDIT_NOTE", 900.0), ("PAYMENT", 500.0)],
        "drafts and voids never count"
    );
    let open = invoices::find_one(&mut c, &main.id, &ADMIN()).await?.outstanding()?;
    assert_eq!(all["closingBalance"], open.to_json_number(), "closing balance = what is still owed");

    let late = crate::statements::customer_statement(&mut c, &s.customer, Some("2026-04-21"), None, &ADMIN()).await?;
    assert_eq!((late["openingBalance"].as_f64(), late["lines"].as_array().map(Vec::len)), (Some(900.0), Some(1)));
    let early = crate::statements::customer_statement(&mut c, &s.customer, None, Some("2026-04-19"), &ADMIN()).await?;
    assert_eq!(early["closingBalance"].as_f64(), Some(1000.0));
    let stranger = Scope { user_id: Some("someone-else".into()), sees_all: false };
    let none = crate::statements::customer_statement(&mut c, &s.customer, None, None, &stranger).await?;
    assert_eq!(none["lines"].as_array().map(Vec::len), Some(0), "only invoices the user may see");
    Ok(())
}

#[tokio::test]
async fn taxes_split_every_document_and_post_to_the_tax_accounts() -> AppResult<()> {
    use crate::tax::{self, TaxCodeInput};
    let s = shop().await?;
    let account = |code: &str, t: AccountType| CreateAccount {
        code: code.into(),
        name: code.into(),
        account_type: t,
        subtype: None,
        description: None,
        parent_id: None,
        currency: None,
    };
    let payable = accounts::create(&s.db, account("2300", AccountType::Liability)).await?.id;
    let receivable = accounts::create(&s.db, account("1400", AccountType::Asset)).await?.id;
    let bal = |id: String| {
        let db = s.db.clone();
        async move {
            let mut c = db.read().acquire().await?;
            AppResult::Ok(ledger::entries::account_balance(&mut c, &id, None, None).await?.balance)
        }
    };
    let code = |name: &str, rate: i64| TaxCodeInput {
        name: Some(name.into()),
        kind: None,
        components: None,
        rate: Some(rate),
        description: None,
        sales_account_id: Some(payable.clone()),
        purchase_account_id: Some(receivable.clone()),
        is_active: None,
    };
    let wrong = tax::create(&s.db, TaxCodeInput { sales_account_id: Some(receivable.clone()), ..code("Bad", 1) }).await;
    assert!(bad_request(&wrong).is_some(), "the sales tax account must be a liability");
    let vat = tax::create(&s.db, code("VAT 18%", 180_000)).await?;
    assert!(matches!(tax::create(&s.db, code("vat 18%", 1)).await, Err(AppError::Conflict(_))));

    // Prices excluding tax: 2 × 100 + 18% = 236.
    let mut ex = s.invoice(vec![("2", "100", true)]);
    ex.lines[0].tax_code_id = Some(vat.id.clone());
    let ex = invoices::create(&s.db, ex, Some("admin")).await?;
    assert_eq!((ex.total_amount, ex.tax_amount), (m("236"), m("36")));
    invoices::post(&s.db, &ex.id, &ADMIN()).await?;
    // Prices including tax: 118 holds 18 of tax.
    let mut inc = s.invoice(vec![("1", "118", true)]);
    inc.lines[0].tax_code_id = Some(vat.id.clone());
    inc.prices_include_tax = true;
    let inc = invoices::create(&s.db, inc, Some("admin")).await?;
    assert_eq!((inc.total_amount, inc.tax_amount), (m("118"), m("18")));
    invoices::post(&s.db, &inc.id, &ADMIN()).await?;
    assert_eq!(s.balance("1100").await?, m("354"));
    assert_eq!(s.balance("4000").await?, m("300"), "revenue never includes tax");
    assert_eq!(bal(payable.clone()).await?, m("54"));

    // A credit note of 59 gross takes 9 of tax back.
    let mut cn = note("AR", &ex.id, "59", &s.acct["4000"]);
    cn.tax_code_id = Some(vat.id.clone());
    let cn = crate::credit_notes::create(&s.db, cn, &ADMIN()).await?;
    assert_eq!(cn.tax_amount, m("9"));
    assert_eq!((s.balance("4000").await?, bal(payable.clone()).await?), (m("250"), m("45")));

    // Purchases: a bill excluding tax and an expense paid including tax.
    let bill = bills::create(
        &s.db,
        CreateBill {
            vendor_id: s.vendor.clone(),
            date: "2026-04-03".into(),
            due_date: None,
            notes: None,
            prices_include_tax: false,
            currency: None,
            exchange_rate: None,
            payable_account_id: None,
            lines: vec![BillLineInput {
                description: "Paper".into(),
                quantity: m("1"),
                unit_price: m("57.5"),
                account_id: s.acct["6000"].clone(),
                tax_code_id: Some(vat.id.clone()),
            }],
        },
    )
    .await?;
    assert_eq!((bill.total_amount, bill.tax_amount), (m("67.85"), m("10.35")));
    bills::post(&s.db, &bill.id).await?;
    let exp = expenses::create(
        &s.db,
        CreateExpense {
            date: "2026-04-08".into(),
            description: "Taxi".into(),
            amount: m("118"),
            expense_account_id: s.acct["6000"].clone(),
            payment_account_id: s.acct["1000"].clone(),
            receipt_url: None,
            tax_code_id: Some(vat.id.clone()),
        },
    )
    .await?;
    expenses::post(&s.db, &exp.id).await?;
    assert_eq!(s.balance("6000").await?, m("157.5"), "expenses never include reclaimable tax");
    assert_eq!(bal(receivable.clone()).await?, m("28.35"));
    assert_eq!(s.balance("2000").await?, m("67.85"));

    // A code in use cannot be deleted, only switched off; a switched-off code is refused.
    assert!(bad_request(&tax::remove(&s.db, &vat.id).await).is_some());
    tax::update(&s.db, &vat.id, TaxCodeInput { is_active: Some(false), ..code("VAT 18%", 180_000) }).await?;
    let mut late = s.invoice(vec![("1", "10", true)]);
    late.lines[0].tax_code_id = Some(vat.id.clone());
    let refused = invoices::create(&s.db, late, Some("admin")).await;
    assert_eq!(bad_request(&refused).as_deref(), Some("The tax code \"VAT 18%\" is no longer in use"));
    let unused = tax::create(&s.db, code("Zero-rated", 0)).await?;
    tax::remove(&s.db, &unused.id).await?;
    Ok(())
}
