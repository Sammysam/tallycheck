use super::*;
use database::Db;
use ledger::accounts::{self, CreateAccount};
use operations::bills::{self, BillLineInput, CreateBill};
use operations::expenses::{self, CreateExpense};
use operations::invoices::{self, CreateInvoice, LineInput, Scope};
use operations::parties::{self, PartyFields, PartyKind};
use operations::payments::{self, CreatePayment};
use std::collections::HashMap;

fn m(s: &str) -> Money {
    Money::parse_decimal(s).unwrap_or_default()
}

fn d(s: &str) -> NaiveDate {
    NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap_or_default()
}

struct Books {
    _dir: tempfile::TempDir,
    db: Db,
    acct: HashMap<&'static str, String>,
}

/// A small but complete month: capital, a sale (part paid), a voided sale, a bill (part paid),
/// cost of goods, an expense and a voided expense.
async fn books() -> AppResult<Books> {
    let dir = tempfile::tempdir().map_err(AppError::internal)?;
    let db = Db::open(&dir.path().join("t.db")).await?;
    sqlx::query(r#"INSERT INTO "users" ("id","username","passwordHash") VALUES ('admin','admin','x')"#)
        .execute(db.write())
        .await?;
    let mut acct = HashMap::new();
    for (code, t, sub) in [
        ("1000", AccountType::Asset, None),
        ("1100", AccountType::Asset, None),
        ("2000", AccountType::Liability, None),
        ("3000", AccountType::Equity, None),
        ("4000", AccountType::Revenue, None),
        ("5000", AccountType::Expense, Some("cogs")),
        ("6000", AccountType::Expense, Some("operating_expense")),
    ] {
        let a = accounts::create(
            &db,
            CreateAccount {
                code: code.into(),
                name: code.into(),
                account_type: t,
                subtype: sub.map(str::to_string),
                description: None,
                parent_id: None,
            },
        )
        .await?;
        acct.insert(code, a.id);
    }
    let b = Books { _dir: dir, db, acct };
    let admin = Scope { user_id: Some("admin".into()), sees_all: true };

    // Owner puts in 10,000.
    let mut tx = b.db.begin().await?;
    ledger::entries::post(
        &mut tx,
        ledger::NewEntry {
            date: "2026-04-01".into(),
            description: "Capital".into(),
            reference: None,
            source_type: None,
            source_id: None,
            lines: vec![
                ledger::NewLine {
                    account_id: b.acct["1000"].clone(),
                    debit: m("10000"),
                    credit: Money::ZERO,
                    description: None,
                },
                ledger::NewLine {
                    account_id: b.acct["3000"].clone(),
                    debit: Money::ZERO,
                    credit: m("10000"),
                    description: None,
                },
            ],
        },
    )
    .await?;
    tx.commit().await?;

    let cust =
        parties::create(&b.db, PartyKind::Customer, PartyFields { name: Some("Acme".into()), ..Default::default() })
            .await?
            .id;
    let sale = |amount: &str, due: &str| CreateInvoice {
        customer_id: Some(cust.clone()),
        date: "2026-04-05".into(),
        due_date: Some(due.into()),
        lines: vec![LineInput {
            description: "work".into(),
            quantity: None,
            unit_price: Some(m(amount)),
            account_id: Some(b.acct["4000"].clone()),
        }],
        ..Default::default()
    };
    let inv = invoices::create(&b.db, sale("3000", "2026-04-20"), Some("admin")).await?;
    invoices::post(&b.db, &inv.id, &admin).await?;
    let pay = |kind: &str, invoice: Option<&str>, bill: Option<&str>, amount: &str| CreatePayment {
        kind: kind.into(),
        invoice_id: invoice.map(str::to_string),
        bill_id: bill.map(str::to_string),
        customer_id: None,
        vendor_id: None,
        date: "2026-04-25".into(),
        amount: m(amount),
        payment_method: "CASH".into(),
        payment_account_id: b.acct["1000"].clone(),
        notes: None,
    };
    payments::create(&b.db, pay("AR", Some(&inv.id), None, "1000")).await?;
    let voided = invoices::create(&b.db, sale("999", "2026-04-30"), Some("admin")).await?;
    invoices::post(&b.db, &voided.id, &admin).await?;
    invoices::void(&b.db, &voided.id, &admin, "2026-04-28").await?;

    let vendor =
        parties::create(&b.db, PartyKind::Vendor, PartyFields { name: Some("Supplier".into()), ..Default::default() })
            .await?
            .id;
    let bill = bills::create(
        &b.db,
        CreateBill {
            vendor_id: vendor,
            date: "2026-04-06".into(),
            due_date: Some("2026-05-06".into()),
            notes: None,
            lines: vec![BillLineInput {
                description: "stock".into(),
                quantity: m("4"),
                unit_price: m("250"),
                account_id: b.acct["5000"].clone(),
            }],
        },
    )
    .await?;
    bills::post(&b.db, &bill.id).await?;
    payments::create(&b.db, pay("AP", None, Some(&bill.id), "400")).await?;

    let expense = |amount: &str| CreateExpense {
        date: "2026-04-10".into(),
        description: "rent".into(),
        amount: m(amount),
        expense_account_id: b.acct["6000"].clone(),
        payment_account_id: b.acct["1000"].clone(),
        receipt_url: None,
    };
    let e = expenses::create(&b.db, expense("500")).await?;
    expenses::post(&b.db, &e.id).await?;
    let e2 = expenses::create(&b.db, expense("77")).await?;
    expenses::post(&b.db, &e2.id).await?;
    expenses::void(&b.db, &e2.id, "2026-04-28").await?;
    Ok(b)
}

#[tokio::test]
async fn profit_and_loss_groups_like_the_reference_and_ignores_voids() -> AppResult<()> {
    let b = books().await?;
    let mut c = b.db.read().acquire().await?;
    let pl = profit_and_loss(&mut c, "2026-04-01", "2026-04-30").await?;
    assert_eq!(pl.total_revenue, m("3000"), "the voided 999 sale and its reversal cancel (D1)");
    assert_eq!(pl.total_cogs, m("1000"));
    assert_eq!(pl.gross_profit, m("2000"));
    assert_eq!(pl.total_operating_expenses, m("500"), "the voided 77 expense cancels");
    assert_eq!(pl.net_income, m("1500"));
    assert_eq!(pl.revenue.len(), 1);
    assert_eq!(pl.cogs[0].account.code, "5000");
    let json = pl.to_json();
    assert_eq!(json["netIncome"], serde_json::json!(1500));
    assert_eq!(json["revenue"][0]["account"]["code"], "4000");

    // A range before anything happened still lists every revenue/expense account at zero.
    let empty = profit_and_loss(&mut c, "2026-01-01", "2026-01-31").await?;
    assert_eq!((empty.revenue.len(), empty.net_income), (1, Money::ZERO));
    Ok(())
}

#[tokio::test]
async fn balance_sheet_always_balances() -> AppResult<()> {
    let b = books().await?;
    let mut c = b.db.read().acquire().await?;
    for as_of in ["2026-03-31", "2026-04-05", "2026-04-15", "2026-04-30", "2026-12-31"] {
        let bs = balance_sheet(&mut c, as_of).await?;
        assert_eq!(bs["balanced"], serde_json::json!(true), "{as_of}: {bs}");
    }
    let bs = balance_sheet(&mut c, "2026-04-30").await?;
    // Cash 10,000 + 1,000 received − 400 paid − 500 rent = 10,100; AR 2,000 still owed.
    assert_eq!(bs["totalAssets"], serde_json::json!(12100));
    assert_eq!(bs["totalLiabilities"], serde_json::json!(600));
    assert_eq!(bs["totalEquity"], serde_json::json!(10000));
    assert_eq!(bs["retainedEarnings"], serde_json::json!(1500));
    assert_eq!(bs["totalLiabilitiesAndEquity"], serde_json::json!(12100));
    Ok(())
}

#[tokio::test]
async fn aging_buckets_outstanding_amounts_by_days_past_due() -> AppResult<()> {
    let b = books().await?;
    let mut c = b.db.read().acquire().await?;
    let ar = ar_aging(&mut c, d("2026-05-01")).await?;
    // Due 2026-04-20, 11 days late, 2,000 of 3,000 still owed; the voided sale is not listed.
    assert_eq!(ar["days1_30"]["total"], serde_json::json!(2000));
    assert_eq!(ar["days1_30"]["items"][0]["name"], "Acme");
    assert_eq!(ar["grandTotal"], serde_json::json!(2000));
    assert_eq!(ar["current"]["items"].as_array().map(Vec::len), Some(0));

    let ap = ap_aging(&mut c, d("2026-05-01")).await?;
    // D13: the bill ages on what is still owed (1,000 − 400).
    assert_eq!(ap["current"]["total"], serde_json::json!(600));
    let late = ap_aging(&mut c, d("2026-08-10")).await?;
    assert_eq!(late["days90plus"]["total"], serde_json::json!(600));
    Ok(())
}

#[test]
fn bucket_edges_match_the_reference() {
    assert_eq!([0, 1, 30, 31, 60, 61, 90, 91, -5].map(bucket_of), [0, 1, 1, 2, 2, 3, 3, 4, 0]);
}

#[tokio::test]
async fn cash_flow_explains_every_movement_of_cash() -> AppResult<()> {
    let b = books().await?;
    // A van bought for cash and a bank loan, to fill the investing and financing sections.
    for (code, t, sub) in
        [("1500", AccountType::Asset, "fixed_asset"), ("2500", AccountType::Liability, "long_term_loan")]
    {
        let a = accounts::create(
            &b.db,
            CreateAccount {
                code: code.into(),
                name: code.into(),
                account_type: t,
                subtype: Some(sub.into()),
                description: None,
                parent_id: None,
            },
        )
        .await?;
        let cash_line = |debit: bool| ledger::NewLine {
            account_id: b.acct["1000"].clone(),
            debit: if debit { m("3000") } else { Money::ZERO },
            credit: if debit { Money::ZERO } else { m("3000") },
            description: None,
        };
        let other = |debit: bool| ledger::NewLine {
            account_id: a.id.clone(),
            debit: if debit { m("3000") } else { Money::ZERO },
            credit: if debit { Money::ZERO } else { m("3000") },
            description: None,
        };
        // The van: cash out. The loan: cash in.
        let lines =
            if code == "1500" { vec![other(true), cash_line(false)] } else { vec![cash_line(true), other(false)] };
        let mut tx = b.db.begin().await?;
        ledger::entries::post(
            &mut tx,
            ledger::NewEntry {
                date: "2026-04-15".into(),
                description: code.into(),
                reference: None,
                source_type: None,
                source_id: None,
                lines,
            },
        )
        .await?;
        tx.commit().await?;
    }
    let cash = vec![b.acct["1000"].clone()];
    let mut c = b.db.read().acquire().await?;
    let cf = cash_flow::cash_flow(&mut c, "2026-04-02", "2026-04-30", &cash).await?;
    // Capital (1 April) is before the period; then +1,000 from the customer, −400 to the
    // supplier, −500 rent (the voided 77 nets out), −3,000 van, +3,000 loan.
    assert_eq!(cf["openingCash"], serde_json::json!(10000));
    assert_eq!(cf["operating"]["total"], serde_json::json!(100));
    assert_eq!(cf["investing"]["total"], serde_json::json!(-3000));
    assert_eq!(cf["financing"]["total"], serde_json::json!(3000));
    assert_eq!(
        (cf["netChange"].clone(), cf["closingCash"].clone()),
        (serde_json::json!(100), serde_json::json!(10100))
    );
    let operating: Vec<(&str, f64)> = cf["operating"]["lines"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|l| (l["account"]["code"].as_str().unwrap_or(""), l["amount"].as_f64().unwrap_or(0.0)))
                .collect()
        })
        .unwrap_or_default();
    assert_eq!(operating, vec![("1100", 1000.0), ("2000", -400.0), ("6000", -500.0)]);

    // Over all dates the capital is a financing inflow and the cash ends where the ledger says.
    let all = cash_flow::cash_flow(&mut c, "", "", &cash).await?;
    assert_eq!(
        (all["openingCash"].clone(), all["financing"]["total"].clone()),
        (serde_json::json!(0), serde_json::json!(13000))
    );
    assert_eq!(all["closingCash"], serde_json::json!(10100));
    let not_cash = cash_flow::cash_flow(&mut c, "", "", &[b.acct["4000"].clone()]).await;
    assert!(matches!(not_cash, Err(AppError::BadRequest(_))));
    Ok(())
}
