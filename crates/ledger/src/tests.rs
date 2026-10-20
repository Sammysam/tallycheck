use crate::accounts::{self, AccountType, CreateAccount, UpdateAccount};
use crate::entries::{self, EntryFilter, EntryStatus, NewEntry, NewLine};
use app_core::money::Money;
use app_core::{AppError, AppResult};
use database::Db;
use std::collections::HashMap;

struct Books {
    _dir: tempfile::TempDir,
    db: Db,
    ids: HashMap<&'static str, String>,
}

async fn books() -> AppResult<Books> {
    let dir = tempfile::tempdir().map_err(AppError::internal)?;
    let db = Db::open(&dir.path().join("t.db")).await?;
    let mut ids = HashMap::new();
    for (code, t) in [
        ("1000", AccountType::Asset),
        ("1100", AccountType::Asset),
        ("2000", AccountType::Liability),
        ("3000", AccountType::Equity),
        ("4000", AccountType::Revenue),
        ("6000", AccountType::Expense),
    ] {
        let a = accounts::create(
            &db,
            CreateAccount {
                code: code.into(),
                name: format!("Account {code}"),
                account_type: t,
                subtype: None,
                description: None,
                parent_id: None,
            },
        )
        .await?;
        ids.insert(code, a.id);
    }
    Ok(Books { _dir: dir, db, ids })
}

fn m(s: &str) -> Money {
    Money::parse_decimal(s).unwrap_or(Money::ZERO)
}

impl Books {
    fn line(&self, code: &str, debit: &str, credit: &str) -> NewLine {
        NewLine {
            account_id: self.ids.get(code).cloned().unwrap_or_default(),
            debit: m(debit),
            credit: m(credit),
            description: None,
        }
    }

    fn entry(&self, date: &str, lines: Vec<NewLine>) -> NewEntry {
        NewEntry {
            date: date.into(),
            description: "test".into(),
            reference: None,
            source_type: Some("MANUAL".into()),
            source_id: None,
            lines,
        }
    }

    async fn post(&self, e: NewEntry) -> AppResult<entries::JournalEntry> {
        let mut tx = self.db.begin().await?;
        let r = entries::post(&mut tx, e).await?;
        tx.commit().await?;
        Ok(r)
    }

    async fn balance(&self, code: &str) -> AppResult<Money> {
        let mut c = self.db.read().acquire().await?;
        Ok(entries::account_balance(&mut c, &self.ids[code], None, None).await?.balance)
    }
}

#[tokio::test]
async fn posts_balanced_entries_with_sequential_numbers() -> AppResult<()> {
    let b = books().await?;
    let e1 =
        b.post(b.entry("2026-04-01", vec![b.line("1000", "1000.50", "0"), b.line("3000", "0", "1000.50")])).await?;
    let e2 = b.post(b.entry("2026-04-02", vec![b.line("6000", "250", "0"), b.line("1000", "0", "250")])).await?;
    assert_eq!((e1.entry_number.as_str(), e2.entry_number.as_str()), ("JE-000001", "JE-000002"));
    assert_eq!(e1.status, EntryStatus::Posted);
    assert_eq!(e1.lines.len(), 2);
    assert!(e1.lines.iter().all(|l| l.account.is_none()), "post answers carry no account objects");
    let json = serde_json_value(&e1)?;
    assert_eq!(json["lines"][0]["debit"], "1000.50");
    assert_eq!(json["lines"][1]["credit"], "1000.50");
    assert_eq!(b.balance("1000").await?, m("750.50"));
    assert_eq!(b.balance("3000").await?, m("1000.50"));
    assert_eq!(b.balance("6000").await?, m("250"));
    Ok(())
}

fn serde_json_value<T: serde::Serialize>(v: &T) -> AppResult<serde_json::Value> {
    serde_json::to_value(v).map_err(AppError::internal)
}

#[tokio::test]
async fn rejects_unbalanced_short_and_unknown_account_entries_without_side_effects() -> AppResult<()> {
    let b = books().await?;
    let r = b.post(b.entry("2026-04-01", vec![b.line("1000", "100.10", "0"), b.line("3000", "0", "100")])).await;
    assert!(matches!(r, Err(AppError::BadRequest(app_core::ErrorMessage::Text(ref s)))
        if s == "Journal entry is not balanced: debits 100.1 ≠ credits 100"));

    // Exactly balanced to the cent even where binary floating point would drift.
    b.post(b.entry(
        "2026-04-01",
        vec![b.line("1000", "0.10", "0"), b.line("1000", "0.20", "0"), b.line("3000", "0", "0.30")],
    ))
    .await?;

    let r = b.post(b.entry("2026-04-01", vec![b.line("1000", "0", "0")])).await;
    assert!(matches!(r, Err(AppError::BadRequest(_))));

    let mut bad = b.line("1000", "5", "0");
    bad.account_id = "7c9e6679-7425-40de-944b-e07fc1f90ae7".into();
    let r = b.post(b.entry("2026-04-01", vec![bad, b.line("3000", "0", "5")])).await;
    assert!(matches!(r, Err(AppError::NotFound(_))));

    // Only the one good entry exists, and its number was not skipped by the failures.
    let mut c = b.db.read().acquire().await?;
    let all = entries::list(&mut c, &EntryFilter::default()).await?;
    assert_eq!(all.len(), 1);
    assert_eq!(all[0].entry_number, "JE-000001");
    drop(c);
    let next = b.post(b.entry("2026-04-03", vec![b.line("1000", "1", "0"), b.line("3000", "0", "1")])).await?;
    assert_eq!(next.entry_number, "JE-000002");
    Ok(())
}

#[tokio::test]
async fn reversal_mirrors_the_entry_and_cancels_its_effect() -> AppResult<()> {
    let b = books().await?;
    b.post(b.entry("2026-04-01", vec![b.line("1000", "1000", "0"), b.line("3000", "0", "1000")])).await?;
    let exp = b.post(b.entry("2026-04-15", vec![b.line("6000", "250", "0"), b.line("1000", "0", "250")])).await?;

    let mut tx = b.db.begin().await?;
    let rev = entries::reverse(&mut tx, &exp.id, "2026-10-14").await?;
    tx.commit().await?;
    assert_eq!(rev.description, format!("Reversal of {}: test", exp.entry_number));
    assert_eq!(rev.reversal_of_id.as_deref(), Some(exp.id.as_str()));
    assert_eq!(rev.date, "2026-10-14");
    assert_eq!(rev.source_type.as_deref(), Some("MANUAL"));
    assert_eq!((rev.lines[0].debit, rev.lines[0].credit), (Money::ZERO, m("250")));
    assert_eq!((rev.lines[1].debit, rev.lines[1].credit), (m("250"), Money::ZERO));

    let mut c = b.db.read().acquire().await?;
    assert_eq!(entries::get(&mut c, &exp.id).await?.status, EntryStatus::Reversed);
    drop(c);

    // D1: the expense and its reversal cancel; cash is back to 1000 (the reference showed 1250).
    assert_eq!(b.balance("6000").await?, Money::ZERO);
    assert_eq!(b.balance("1000").await?, m("1000"));

    let mut tx = b.db.begin().await?;
    let again = entries::reverse(&mut tx, &exp.id, "2026-10-14").await;
    assert!(matches!(again, Err(AppError::BadRequest(app_core::ErrorMessage::Text(ref s)))
        if s == "This entry has already been reversed"));
    let missing = entries::reverse(&mut tx, "nope", "2026-10-14").await;
    assert!(matches!(missing, Err(AppError::NotFound(ref s)) if s == "Journal entry nope not found"));
    Ok(())
}

#[tokio::test]
async fn trial_balance_and_ranges() -> AppResult<()> {
    let b = books().await?;
    b.post(b.entry("2026-04-01", vec![b.line("1000", "1000", "0"), b.line("3000", "0", "1000")])).await?;
    b.post(b.entry(
        "2026-04-20",
        vec![b.line("1000", "100", "0"), b.line("1100", "50.25", "0"), b.line("4000", "0", "150.25")],
    ))
    .await?;
    let mut c = b.db.read().acquire().await?;
    let tb = entries::trial_balance(&mut c, None).await?;
    let codes: Vec<&str> = tb.iter().map(|r| r.account.code.as_str()).collect();
    assert_eq!(codes, ["1000", "1100", "3000", "4000"]);
    let (d, cr) = tb.iter().fold((Money::ZERO, Money::ZERO), |(d, c), r| {
        (d.checked_add(r.debit).unwrap_or(d), c.checked_add(r.credit).unwrap_or(c))
    });
    assert_eq!(d, cr);
    let tb_early = entries::trial_balance(&mut c, Some("2026-04-10")).await?;
    assert_eq!(tb_early.len(), 2);
    let range = entries::account_balance(&mut c, &b.ids["1000"], Some("2026-04-10"), Some("2026-04-30")).await?;
    assert_eq!(range.balance, m("100"));
    let unknown = entries::account_balance(&mut c, "nope", None, None).await?;
    assert_eq!((unknown.debit, unknown.credit, unknown.balance), (Money::ZERO, Money::ZERO, Money::ZERO));
    let json = serde_json_value(&tb[0])?;
    assert_eq!(json["debit"], serde_json::json!(1100));
    assert_eq!(json["account"]["code"], "1000");
    Ok(())
}

/// Many random balanced postings and reversals: the books always balance and every account's
/// balance equals the sum of its own lines.
#[tokio::test]
async fn books_always_balance_under_random_postings() -> AppResult<()> {
    let b = books().await?;
    let codes = ["1000", "1100", "2000", "3000", "4000", "6000"];
    let mut seed: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut rand = move |n: u64| {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed % n
    };
    let mut posted = Vec::new();
    for i in 0..120u64 {
        let k = 2 + rand(3) as usize;
        let mut lines = Vec::new();
        let mut debit_total = 0i64;
        for _ in 0..k - 1 {
            let cents = 1 + rand(1_000_000) as i64;
            debit_total += cents;
            lines.push(NewLine {
                account_id: b.ids[codes[rand(6) as usize]].clone(),
                debit: Money::from_cents(cents),
                credit: Money::ZERO,
                description: None,
            });
        }
        lines.push(NewLine {
            account_id: b.ids[codes[rand(6) as usize]].clone(),
            debit: Money::ZERO,
            credit: Money::from_cents(debit_total),
            description: None,
        });
        let e = b.post(b.entry(&format!("2026-04-{:02}", 1 + i % 28), lines)).await?;
        posted.push(e.id);
        if rand(4) == 0 {
            let id = posted[rand(posted.len() as u64) as usize].clone();
            let mut tx = b.db.begin().await?;
            match entries::reverse(&mut tx, &id, "2026-05-01").await {
                Ok(_) => tx.commit().await?,
                Err(AppError::BadRequest(_)) => tx.rollback().await?,
                Err(e) => return Err(e),
            }
        }
    }
    let mut c = b.db.read().acquire().await?;
    let tb = entries::trial_balance(&mut c, None).await?;
    let sum = |f: fn(&entries::TrialBalanceRow) -> Money| tb.iter().map(f).map(Money::cents).sum::<i64>();
    assert_eq!(sum(|r| r.debit), sum(|r| r.credit));
    let net: i64 = tb
        .iter()
        .map(|r| if r.account.account_type.is_debit_normal() { r.balance.cents() } else { -r.balance.cents() })
        .sum();
    assert_eq!(net, 0, "debit-normal balances equal credit-normal balances");
    let reversed: i64 = sqlx::query_scalar(r#"SELECT COUNT(*) FROM "journal_entries" WHERE "status"='REVERSED'"#)
        .fetch_one(&mut *c)
        .await?;
    let reversals: i64 =
        sqlx::query_scalar(r#"SELECT COUNT(*) FROM "journal_entries" WHERE "reversalOfId" IS NOT NULL"#)
            .fetch_one(&mut *c)
            .await?;
    assert_eq!(reversed, reversals);
    Ok(())
}

#[tokio::test]
async fn account_rules_match_reference() -> AppResult<()> {
    let b = books().await?;
    let dup = accounts::create(
        &b.db,
        CreateAccount {
            code: "1000".into(),
            name: "Again".into(),
            account_type: AccountType::Asset,
            subtype: None,
            description: None,
            parent_id: None,
        },
    )
    .await;
    assert!(matches!(dup, Err(AppError::Conflict(ref s)) if s == "Account code 1000 already exists"));

    let conflict =
        accounts::update(&b.db, &b.ids["1100"], UpdateAccount { code: Some("1000".into()), ..Default::default() })
            .await;
    assert!(matches!(conflict, Err(AppError::Conflict(_))));
    let unchanged = accounts::update(&b.db, &b.ids["1100"], UpdateAccount::default()).await?;
    let renamed =
        accounts::update(&b.db, &b.ids["1100"], UpdateAccount { name: Some("Debtors".into()), ..Default::default() })
            .await?;
    assert_eq!(renamed.name, "Debtors");
    assert!(renamed.updated_at >= unchanged.updated_at);

    b.post(b.entry("2026-04-01", vec![b.line("1000", "1", "0"), b.line("3000", "0", "1")])).await?;
    let used = accounts::remove(&b.db, &b.ids["1000"]).await;
    assert!(matches!(used, Err(AppError::BadRequest(_))));
    accounts::remove(&b.db, &b.ids["2000"]).await?;
    let gone = accounts::remove(&b.db, &b.ids["2000"]).await;
    assert!(matches!(gone, Err(AppError::NotFound(_))));

    // D10: seeding adds only the missing default codes: 1200, 2000 (deleted above), 2100 and 5000
    // (1000/1100/3000/4000/6000 exist; 6100 comes from the payroll migration).
    assert_eq!(accounts::seed(&b.db).await?, 4);
    assert_eq!(accounts::seed(&b.db).await?, 0);
    Ok(())
}

#[tokio::test]
async fn seed_fills_an_empty_chart() -> AppResult<()> {
    let dir = tempfile::tempdir().map_err(AppError::internal)?;
    let db = Db::open(&dir.path().join("t.db")).await?;
    sqlx::query(r#"DELETE FROM "accounts""#).execute(db.write()).await?;
    assert_eq!(accounts::seed(&db).await?, 10);
    let all = accounts::find_all(&db).await?;
    assert_eq!(all.len(), 10);
    assert_eq!(all[0].code, "1000");
    assert_eq!(accounts::seed(&db).await?, 0);
    Ok(())
}

#[tokio::test]
async fn bank_reconciliation_matches_the_statement_and_carries_outstanding_lines() -> AppResult<()> {
    use crate::reconciliation as rec;
    let b = books().await?;
    let cash = b.ids["1000"].clone();
    let capital = b.post(b.entry("2026-04-01", vec![b.line("1000", "1000", "0"), b.line("3000", "0", "1000")])).await?;
    let cheque = b.post(b.entry("2026-04-05", vec![b.line("6000", "250", "0"), b.line("1000", "0", "250")])).await?;
    let deposit = b.post(b.entry("2026-04-20", vec![b.line("1000", "300", "0"), b.line("4000", "0", "300")])).await?;
    let may = b.post(b.entry("2026-05-03", vec![b.line("6000", "100", "0"), b.line("1000", "0", "100")])).await?;
    // A receipt voided before it reached the bank: entry and reversal cancel and stay off the list.
    let voided = b.post(b.entry("2026-04-10", vec![b.line("1000", "50", "0"), b.line("4000", "0", "50")])).await?;
    let mut tx = b.db.begin().await?;
    entries::reverse(&mut tx, &voided.id, "2026-04-12").await?;
    tx.commit().await?;
    let cash_line = |e: &entries::JournalEntry| {
        e.lines.iter().find(|l| l.account_id == cash).map(|l| l.id.clone()).unwrap_or_default()
    };

    let april = rec::start(&b.db, &cash, "2026-04-30", m("750"), None).await?;
    let ids: Vec<&str> = april.lines.iter().map(|l| l.entry_id.as_str()).collect();
    assert_eq!(
        ids,
        vec![capital.id.as_str(), cheque.id.as_str(), deposit.id.as_str()],
        "May and the voided pair are not listed"
    );
    assert_eq!((april.cleared_balance, april.difference), (Money::ZERO, m("750")));
    let id = april.rec.id.clone();

    let early = rec::complete(&b.db, &id, None).await;
    assert!(matches!(early, Err(AppError::BadRequest(_))), "not balanced yet");
    let wrong = rec::set_cleared(&b.db, &id, &[cash_line(&may)]).await;
    assert!(matches!(wrong, Err(AppError::BadRequest(_))), "a May line is after the statement date");
    let twice = rec::start(&b.db, &cash, "2026-04-30", m("750"), None).await;
    assert!(matches!(twice, Err(AppError::BadRequest(_))), "one reconciliation in progress per account");

    // The deposit is in transit: tick only what the statement shows.
    let ws = rec::set_cleared(&b.db, &id, &[cash_line(&capital), cash_line(&cheque)]).await?;
    assert_eq!((ws.cleared_balance, ws.difference), (m("750"), Money::ZERO));
    let done = rec::complete(&b.db, &id, None).await?;
    assert_eq!(done.rec.status, rec::COMPLETED);
    assert_eq!(done.lines.len(), 2, "a completed reconciliation shows what it cleared");

    let stale = rec::start(&b.db, &cash, "2026-04-30", m("0"), None).await;
    assert!(matches!(stale, Err(AppError::BadRequest(_))), "statement dates move forward");
    let may_rec = rec::start(&b.db, &cash, "2026-05-31", m("950"), None).await?;
    assert_eq!(may_rec.rec.opening_balance, m("750"));
    let left: Vec<&str> = may_rec.lines.iter().map(|l| l.entry_id.as_str()).collect();
    assert_eq!(left, vec![deposit.id.as_str(), may.id.as_str()], "the April deposit comes up again");
    let may_id = may_rec.rec.id.clone();
    rec::set_cleared(&b.db, &may_id, &[cash_line(&deposit), cash_line(&may)]).await?;
    rec::complete(&b.db, &may_id, None).await?;

    let not_latest = rec::undo(&b.db, &id).await;
    assert!(matches!(not_latest, Err(AppError::BadRequest(_))), "only the latest can be undone");
    let reopened = rec::undo(&b.db, &may_id).await?;
    assert_eq!((reopened.rec.status.as_str(), reopened.difference), (rec::DRAFT, Money::ZERO), "ticks are kept");
    rec::discard(&b.db, &may_id).await?;
    let again = rec::start(&b.db, &cash, "2026-05-31", m("950"), None).await?;
    assert!(again.lines.iter().all(|l| !l.cleared), "discarding frees the lines");
    let mut c = b.db.read().acquire().await?;
    assert_eq!(rec::list(&mut c, &cash).await?.len(), 2);
    Ok(())
}

#[tokio::test]
async fn account_activity_runs_the_balance_and_matches_the_trial_balance() -> AppResult<()> {
    let b = books().await?;
    let cash = b.ids["1000"].clone();
    b.post(b.entry("2026-03-31", vec![b.line("1000", "1000", "0"), b.line("3000", "0", "1000")])).await?;
    b.post(b.entry("2026-04-05", vec![b.line("6000", "250.25", "0"), b.line("1000", "0", "250.25")])).await?;
    let sale = b.post(b.entry("2026-04-20", vec![b.line("1000", "300", "0"), b.line("4000", "0", "300")])).await?;
    let mut tx = b.db.begin().await?;
    entries::reverse(&mut tx, &sale.id, "2026-04-21").await?;
    tx.commit().await?;

    let mut c = b.db.read().acquire().await?;
    let april = crate::activity::account_activity(&mut c, &cash, Some("2026-04-01"), Some("2026-04-30")).await?;
    assert_eq!(april.opening, m("1000"));
    let running: Vec<Money> = april.lines.iter().map(|l| l.balance).collect();
    assert_eq!(running, vec![m("749.75"), m("1049.75"), m("749.75")], "a reversal counts too (D1)");
    assert_eq!((april.total_debit, april.total_credit, april.closing), (m("300"), m("550.25"), m("749.75")));
    let all = crate::activity::account_activity(&mut c, &cash, None, None).await?;
    assert_eq!(all.closing, entries::account_balance(&mut c, &cash, None, None).await?.balance);
    // Credit-normal accounts run the other way round.
    let revenue = crate::activity::account_activity(&mut c, &b.ids["4000"], None, None).await?;
    assert_eq!(revenue.lines.first().map(|l| l.balance), Some(m("300")));
    assert_eq!(revenue.closing, Money::ZERO);
    Ok(())
}
