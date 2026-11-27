//! D59: moving books in from another accounting system (QuickBooks, Xero, Tally, …) on a cut-over
//! date. The screen reads the old system's exports and sends the mapped rows here as text; one
//! conversion is one transaction, and a preview runs the same code and rolls back.
//!
//! * Accounts: each old account is matched to a TallyCheck account or created.
//! * Customers / vendors: new names are created; names already in the list are used as they are.
//! * Unpaid invoices and bills each become a POSTED document against 3900 Opening Balance Equity
//!   (Dr 1100 / Cr 3900 and Dr 3900 / Cr 2000), dated as in the old system, so they can be paid,
//!   chased and aged here. A balance list (one amount per customer / vendor, no number or date)
//!   works too: each amount is one document dated the cut-over day.
//! * The trial balance on the cut-over date becomes one opening entry (source CONVERSION). Its
//!   receivables (1100) and payables (2000) lines go to 3900 instead, so 3900 nets to zero when
//!   the open documents add up to the trial balance; a difference must be accepted explicitly.
//!
//! Anything wrong anywhere and nothing is saved. Reversing a conversion voids its documents and
//! reverses its opening entry (dated today); accounts, customers and vendors it created stay.

use crate::bills::{self, BillLineInput, CreateBill};
use crate::imports::{parse_amount, parse_date, DateOrder};
use crate::invoices::{self, CreateInvoice, LineInput, Scope};
use crate::parties::{self, PartyFields, PartyKind};
use app_core::ids::new_id;
use app_core::money::Money;
use app_core::time::now_iso;
use app_core::{AppError, AppResult};
use database::{Db, Tx};
use ledger::accounts::{self, AccountType, CreateAccount};
use ledger::{entries, NewEntry, NewLine};
use serde::Serialize;
use serde_json::{json, Value};
use sqlx::Row;
use std::collections::HashMap;

pub const OPENING_CODE: &str = "3900";
pub const OPENING_NAME: &str = "Opening Balance Equity";
pub const SOURCE_TYPE: &str = "CONVERSION";
const RECEIVABLE_CODE: &str = "1100";
const PAYABLE_CODE: &str = "2000";
pub const MAX_ROWS: usize = 10_000;

pub struct AccountRow {
    /// The old system's account as written in its files (number and / or name).
    pub key: String,
    /// An existing TallyCheck account to use; otherwise one is created from code / name / type.
    pub account_id: Option<String>,
    pub code: String,
    pub name: String,
    pub account_type: Option<AccountType>,
    /// M3: the subtype of an account that is created (e.g. `fixed_asset`, `long_term_liability`),
    /// so the cash flow report classes it correctly.
    pub subtype: Option<String>,
}

pub struct PartyRow {
    pub name: String,
    pub email: String,
    pub phone: String,
    pub address: String,
    pub tax_id: String,
}

pub struct BalanceRow {
    pub account: String,
    pub debit: String,
    pub credit: String,
}

pub struct OpenDocument {
    pub number: String,
    pub party: String,
    pub date: String,
    pub due_date: String,
    /// What is still unpaid on it.
    pub amount: String,
}

pub struct Request {
    pub source: String,
    pub cutover_date: String,
    pub date_order: DateOrder,
    pub accounts: Vec<AccountRow>,
    pub customers: Vec<PartyRow>,
    pub vendors: Vec<PartyRow>,
    pub balances: Vec<BalanceRow>,
    pub invoices: Vec<OpenDocument>,
    pub bills: Vec<OpenDocument>,
    /// Save even though the open invoices / bills don't add up to the trial balance's
    /// receivables / payables (the difference stays in 3900).
    pub accept_differences: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Issue {
    /// accounts, customers, vendors, balances, invoices, bills or general.
    pub section: &'static str,
    /// 1-based row in that section (0 = the whole section).
    pub row: usize,
    pub label: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Totals {
    pub accounts_created: usize,
    pub accounts_matched: usize,
    pub customers_created: usize,
    pub customers_existing: usize,
    pub vendors_created: usize,
    pub vendors_existing: usize,
    pub invoices: usize,
    pub bills: usize,
    pub opening_lines: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Check {
    pub debits: Value,
    pub credits: Value,
    /// Receivables in the trial balance and the open invoices' total.
    pub receivables_balance: Value,
    pub open_invoices: Value,
    pub payables_balance: Value,
    pub open_bills: Value,
    /// What would be left in 3900 Opening Balance Equity (0 when everything agrees).
    pub left_in_opening_equity: Value,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Outcome {
    /// The conversion's id once saved.
    pub id: Option<String>,
    pub name: String,
    pub totals: Totals,
    pub check: Check,
    pub issues: Vec<Issue>,
    pub can_save: bool,
}

fn norm(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

fn money_text(m: Money) -> String {
    m.to_db_string()
}

fn add(a: Money, b: Money) -> AppResult<Money> {
    a.checked_add(b).ok_or_else(crate::overflow)
}

fn sub(a: Money, b: Money) -> AppResult<Money> {
    a.checked_sub(b).ok_or_else(crate::overflow)
}

struct Issues(Vec<Issue>);

impl Issues {
    fn push(&mut self, section: &'static str, row: usize, label: impl Into<String>, message: impl Into<String>) {
        self.0.push(Issue { section, row, label: label.into(), message: message.into() });
    }
}

async fn savepoint(tx: &mut Tx, sql: &'static str) -> AppResult<()> {
    sqlx::query(sql).execute(&mut **tx).await?;
    Ok(())
}

macro_rules! in_savepoint {
    ($tx:expr, $body:expr) => {{
        savepoint($tx, "SAVEPOINT conversion_row").await?;
        let r = $body;
        if r.is_ok() {
            savepoint($tx, "RELEASE conversion_row").await?;
        } else {
            savepoint($tx, "ROLLBACK TO conversion_row").await?;
            savepoint($tx, "RELEASE conversion_row").await?;
        }
        r
    }};
}

async fn remember(tx: &mut Tx, conversion_id: &str, record_type: &str, record_id: &str, key: &str) -> AppResult<()> {
    sqlx::query(
        r#"INSERT INTO "conversion_items" ("id","conversionId","recordType","recordId","externalKey") VALUES (?1,?2,?3,?4,?5)"#,
    )
    .bind(new_id())
    .bind(conversion_id)
    .bind(record_type)
    .bind(record_id)
    .bind(key)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// 3900 Opening Balance Equity, added the first time it is needed.
pub async fn opening_account(tx: &mut Tx) -> AppResult<accounts::Account> {
    if let Ok(a) = accounts::find_by_code(tx, OPENING_CODE).await {
        if a.account_type != AccountType::Equity {
            return Err(AppError::bad_request(format!(
                "Account {OPENING_CODE} is already \"{}\", which is not an equity account. Give it another code, \
                 so {OPENING_CODE} can be {OPENING_NAME}.",
                a.name
            )));
        }
        return Ok(a);
    }
    accounts::insert_chart(tx, &[(OPENING_CODE, OPENING_NAME, AccountType::Equity, "equity")]).await?;
    accounts::find_by_code(tx, OPENING_CODE).await
}

/// The conversion still in force, if any (one at a time).
async fn active(conn: &mut sqlx::SqliteConnection) -> AppResult<Option<(String, String)>> {
    Ok(sqlx::query_as(r#"SELECT "source", "cutoverDate" FROM "conversions" WHERE "status" = 'ACTIVE' LIMIT 1"#)
        .fetch_optional(conn)
        .await?)
}

/// Existing customers / vendors by normalised name.
async fn party_names(tx: &mut Tx, kind: PartyKind) -> AppResult<HashMap<String, String>> {
    let rows = sqlx::query(sqlx::AssertSqlSafe(format!(r#"SELECT "id", "name" FROM "{}""#, kind.table())))
        .fetch_all(&mut **tx)
        .await?;
    Ok(rows.iter().map(|r| (norm(&r.get::<String, _>("name")), r.get::<String, _>("id"))).collect())
}

fn amount_of(text: &str) -> Result<Money, String> {
    Ok(parse_amount(text)?.unwrap_or(Money::ZERO))
}

pub async fn run(db: &Db, req: Request, created_by: Option<&str>, commit: bool) -> AppResult<Outcome> {
    let source = req.source.trim().to_string();
    if source.is_empty() {
        return Err(AppError::bad_request("Name the system the books come from (for example QuickBooks Online)"));
    }
    let Some(cutover) = parse_date(&req.cutover_date, DateOrder::Ymd) else {
        return Err(AppError::bad_request("Enter the cut-over date: the last day the old system's figures cover"));
    };
    if cutover > app_core::time::today_local().to_string() {
        return Err(AppError::bad_request("The cut-over date can't be in the future"));
    }
    let rows = req.accounts.len()
        + req.customers.len()
        + req.vendors.len()
        + req.balances.len()
        + req.invoices.len()
        + req.bills.len();
    if rows == 0 {
        return Err(AppError::bad_request("There is nothing to bring in yet"));
    }
    if rows > MAX_ROWS {
        return Err(AppError::bad_request(format!("Bring in at most {MAX_ROWS} rows at a time")));
    }

    let mut tx = db.begin().await?;
    if let Some((src, date)) = active(&mut tx).await? {
        return Err(AppError::bad_request(format!(
            "Books were already moved in from {src} (cut-over {date}). Reverse that conversion first to do it again."
        )));
    }
    let id = new_id();
    let now = now_iso();
    sqlx::query(
        r#"INSERT INTO "conversions" ("id","source","cutoverDate","createdById","createdAt","updatedAt") VALUES (?1,?2,?3,?4,?5,?5)"#,
    )
    .bind(&id)
    .bind(&source)
    .bind(&cutover)
    .bind(created_by)
    .bind(&now)
    .execute(&mut *tx)
    .await?;
    let opening = opening_account(&mut tx).await?;
    let mut issues = Issues(Vec::new());
    let mut totals = Totals {
        accounts_created: 0,
        accounts_matched: 0,
        customers_created: 0,
        customers_existing: 0,
        vendors_created: 0,
        vendors_existing: 0,
        invoices: 0,
        bills: 0,
        opening_lines: 0,
    };

    // ── Accounts ────────────────────────────────────────────────────────────
    let mut account_ids: HashMap<String, String> = HashMap::new();
    for (i, a) in req.accounts.iter().enumerate() {
        let n = i + 1;
        let key = norm(&a.key);
        if key.is_empty() {
            issues.push("accounts", n, "", "No account name or number");
            continue;
        }
        if account_ids.contains_key(&key) {
            issues.push("accounts", n, &a.key, "This account is listed twice");
            continue;
        }
        if let Some(existing) = a.account_id.as_deref().filter(|s| !s.is_empty()) {
            match accounts::get(&mut tx, existing).await? {
                Some(acc) if acc.is_active => {
                    account_ids.insert(key, acc.id);
                    totals.accounts_matched += 1;
                }
                Some(acc) => issues.push("accounts", n, &a.key, format!("{} {} is inactive", acc.code, acc.name)),
                None => issues.push("accounts", n, &a.key, "The chosen TallyCheck account no longer exists"),
            }
            continue;
        }
        let (code, name) = (a.code.trim(), a.name.trim());
        let Some(kind) = a.account_type else {
            issues.push("accounts", n, &a.key, "Choose the account type (asset, liability, equity, income or expense)");
            continue;
        };
        if code.is_empty() || name.is_empty() {
            issues.push("accounts", n, &a.key, "A new account needs a code and a name");
            continue;
        }
        if let Ok(acc) = accounts::find_by_code(&mut tx, code).await {
            if norm(&acc.name) == norm(name) && acc.account_type == kind {
                account_ids.insert(key, acc.id);
                totals.accounts_matched += 1;
            } else {
                issues.push(
                    "accounts",
                    n,
                    &a.key,
                    format!(
                        "Code {code} is already \"{}\". Choose that account or give this one another code.",
                        acc.name
                    ),
                );
            }
            continue;
        }
        let dto = CreateAccount {
            code: code.to_string(),
            name: name.to_string(),
            account_type: kind,
            subtype: a.subtype.clone(),
            description: Some(format!("Brought in from {source}")),
            parent_id: None,
            currency: None,
        };
        let created = accounts::create_in(&mut tx, dto).await?;
        remember(&mut tx, &id, "ACCOUNT", &created.id, &a.key).await?;
        account_ids.insert(key, created.id);
        totals.accounts_created += 1;
    }

    // ── Customers and vendors ───────────────────────────────────────────────
    let mut customers = party_names(&mut tx, PartyKind::Customer).await?;
    let mut vendors = party_names(&mut tx, PartyKind::Vendor).await?;
    for (kind, list, section) in
        [(PartyKind::Customer, &req.customers, "customers"), (PartyKind::Vendor, &req.vendors, "vendors")]
    {
        let names = if kind == PartyKind::Customer { &mut customers } else { &mut vendors };
        for (i, p) in list.iter().enumerate() {
            let name = p.name.trim();
            let blank = |s: &str| (!s.trim().is_empty()).then(|| s.trim().to_string());
            if name.is_empty() {
                if [&p.email, &p.phone, &p.address, &p.tax_id].iter().any(|s| !s.trim().is_empty()) {
                    issues.push(section, i + 1, "", "No name");
                }
                continue;
            }
            if names.contains_key(&norm(name)) {
                if kind == PartyKind::Customer {
                    totals.customers_existing += 1;
                } else {
                    totals.vendors_existing += 1;
                }
                continue;
            }
            let fields = PartyFields {
                name: Some(name.to_string()),
                email: Some(blank(&p.email)),
                phone: Some(blank(&p.phone)),
                address: Some(blank(&p.address)),
                tax_id: Some(blank(&p.tax_id)),
            };
            let party = parties::create_in(&mut tx, kind, fields).await?;
            let record = if kind == PartyKind::Customer { "CUSTOMER" } else { "VENDOR" };
            remember(&mut tx, &id, record, &party.id, name).await?;
            names.insert(norm(name), party.id);
            if kind == PartyKind::Customer {
                totals.customers_created += 1;
            } else {
                totals.vendors_created += 1;
            }
        }
    }

    // ── Trial balance → opening lines (by account) ──────────────────────────
    let receivable_id = accounts::find_by_code(&mut tx, RECEIVABLE_CODE).await.ok().map(|a| a.id);
    let payable_id = accounts::find_by_code(&mut tx, PAYABLE_CODE).await.ok().map(|a| a.id);
    let (mut debits, mut credits) = (Money::ZERO, Money::ZERO);
    let mut by_account: Vec<(String, Money)> = Vec::new(); // account → debit − credit
    for (i, b) in req.balances.iter().enumerate() {
        let n = i + 1;
        let (dr, cr) = match (amount_of(&b.debit), amount_of(&b.credit)) {
            (Err(e), _) | (_, Err(e)) => {
                issues.push("balances", n, &b.account, e);
                continue;
            }
            (Ok(d), Ok(c)) => (d, c),
        };
        let net = sub(dr, cr)?;
        if net == Money::ZERO {
            continue;
        }
        let Some(account_id) = account_ids.get(&norm(&b.account)).cloned() else {
            issues.push(
                "balances",
                n,
                &b.account,
                if b.account.trim().is_empty() {
                    "No account".to_string()
                } else {
                    "This account is not in the account list; add it there and match it".to_string()
                },
            );
            continue;
        };
        if net > Money::ZERO {
            debits = add(debits, net)?;
        } else {
            credits = sub(credits, net)?;
        }
        match by_account.iter_mut().find(|(a, _)| *a == account_id) {
            Some((_, total)) => *total = add(*total, net)?,
            None => by_account.push((account_id, net)),
        }
    }
    if debits != credits {
        issues.push(
            "balances",
            0,
            "",
            format!(
                "The trial balance doesn't balance: debits {}, credits {}, difference {}",
                money_text(debits),
                money_text(credits),
                money_text(sub(debits, credits)?)
            ),
        );
    }
    let take = |by_account: &[(String, Money)], id: &Option<String>| -> Money {
        id.as_ref().and_then(|id| by_account.iter().find(|(a, _)| a == id)).map_or(Money::ZERO, |(_, m)| *m)
    };
    let receivables_balance = take(&by_account, &receivable_id);
    let payables_balance = sub(Money::ZERO, take(&by_account, &payable_id))?;

    // ── Unpaid invoices and bills ───────────────────────────────────────────
    let scope = Scope { user_id: created_by.map(str::to_string), sees_all: true };
    let mut open_invoices = Money::ZERO;
    let mut open_bills = Money::ZERO;
    for (is_invoice, list, section) in [(true, &req.invoices, "invoices"), (false, &req.bills, "bills")] {
        for (i, d) in list.iter().enumerate() {
            let n = i + 1;
            let number = d.number.trim();
            let label = format!("{number} · {}", d.party.trim());
            if number.is_empty() && d.party.trim().is_empty() && d.amount.trim().is_empty() {
                continue;
            }
            // A balance list (one amount per customer / vendor, no dates) is dated the cut-over day.
            let date = match d.date.trim() {
                "" => cutover.clone(),
                t => match parse_date(t, req.date_order) {
                    Some(date) => date,
                    None => {
                        issues.push(section, n, label, format!("\"{t}\" is not a date"));
                        continue;
                    }
                },
            };
            if date > cutover {
                issues.push(
                    section,
                    n,
                    label,
                    "Dated after the cut-over date; enter it in TallyCheck as a normal document instead",
                );
                continue;
            }
            // D75: without a due date an opening document is due 30 days after its date, so it is not
            // shown as overdue the day it comes in (ageing counts a document without one from its date).
            let due = match d.due_date.trim() {
                "" => chrono::NaiveDate::parse_from_str(&date, "%Y-%m-%d")
                    .ok()
                    .and_then(|d| d.checked_add_days(chrono::Days::new(30)))
                    .map(|d| d.format("%Y-%m-%d").to_string()),
                t => match parse_date(t, req.date_order) {
                    Some(due) => Some(due),
                    None => {
                        issues.push(section, n, label, format!("\"{t}\" is not a date"));
                        continue;
                    }
                },
            };
            let amount = match amount_of(&d.amount) {
                Err(e) => {
                    issues.push(section, n, label, e);
                    continue;
                }
                Ok(a) if a <= Money::ZERO => {
                    issues.push(
                        section,
                        n,
                        label,
                        "Only amounts still owed can come in; a credit balance must be entered as a credit or debit note",
                    );
                    continue;
                }
                Ok(a) => a,
            };
            let party_name = d.party.trim();
            if party_name.is_empty() {
                issues.push(section, n, label, if is_invoice { "No customer" } else { "No vendor" });
                continue;
            }
            let kind = if is_invoice { PartyKind::Customer } else { PartyKind::Vendor };
            let names = if is_invoice { &mut customers } else { &mut vendors };
            let party_id = match names.get(&norm(party_name)) {
                Some(p) => p.clone(),
                None => {
                    let party = parties::create_in(
                        &mut tx,
                        kind,
                        PartyFields { name: Some(party_name.to_string()), ..Default::default() },
                    )
                    .await?;
                    remember(&mut tx, &id, if is_invoice { "CUSTOMER" } else { "VENDOR" }, &party.id, party_name)
                        .await?;
                    names.insert(norm(party_name), party.id.clone());
                    if is_invoice {
                        totals.customers_created += 1;
                    } else {
                        totals.vendors_created += 1;
                    }
                    party.id
                }
            };
            let what = if is_invoice { "invoice" } else { "bill" };
            let description = if number.is_empty() {
                format!("Opening balance brought in from {source}")
            } else {
                format!("Opening balance: {source} {what} {number}")
            };
            let note = format!("Brought in from {source} on the cut-over to TallyCheck ({cutover}).");
            let saved: AppResult<String> = in_savepoint!(&mut tx, {
                async {
                    if is_invoice {
                        let inv = invoices::create_in(
                            &mut tx,
                            CreateInvoice {
                                customer_id: Some(party_id.clone()),
                                date: date.clone(),
                                due_date: due.clone(),
                                po_number: (!number.is_empty()).then(|| number.to_string()),
                                notes: Some(note.clone()),
                                lines: vec![LineInput {
                                    description: description.clone(),
                                    quantity: Some(Money::from_cents(100)),
                                    unit_price: Some(amount),
                                    account_id: Some(opening.id.clone()),
                                    tax_code_id: None,
                                    item_id: None,
                                    discount: Money::ZERO,
                                }],
                                ..Default::default()
                            },
                            created_by,
                        )
                        .await?;
                        invoices::post_in(&mut tx, &inv.id, &scope).await?;
                        Ok(inv.id)
                    } else {
                        let bill = bills::create_in(
                            &mut tx,
                            CreateBill {
                                vendor_id: party_id.clone(),
                                date: date.clone(),
                                due_date: due.clone(),
                                notes: Some(if number.is_empty() {
                                    note.clone()
                                } else {
                                    format!("{note} Their bill number: {number}.")
                                }),
                                lines: vec![BillLineInput {
                                    description: description.clone(),
                                    quantity: Money::from_cents(100),
                                    unit_price: amount,
                                    account_id: opening.id.clone(),
                                    tax_code_id: None,
                                    item_id: None,
                                }],
                                prices_include_tax: false,
                                currency: None,
                                exchange_rate: None,
                                payable_account_id: None,
                            },
                        )
                        .await?;
                        bills::post_in(&mut tx, &bill.id).await?;
                        Ok(bill.id)
                    }
                }
                .await
            });
            match saved {
                Ok(doc_id) => {
                    remember(&mut tx, &id, if is_invoice { "INVOICE" } else { "BILL" }, &doc_id, number).await?;
                    if is_invoice {
                        open_invoices = add(open_invoices, amount)?;
                        totals.invoices += 1;
                    } else {
                        open_bills = add(open_bills, amount)?;
                        totals.bills += 1;
                    }
                }
                Err(e) => issues.push(section, n, label, e.user_message()),
            }
        }
    }

    // ── The opening entry ───────────────────────────────────────────────────
    let mut opening_entry_id = None;
    if issues.0.is_empty() && !by_account.is_empty() {
        let mut lines = Vec::new();
        for (account_id, net) in &by_account {
            let to_opening = Some(account_id) == receivable_id.as_ref() || Some(account_id) == payable_id.as_ref();
            let (target, description) = if to_opening {
                let what = if Some(account_id) == receivable_id.as_ref() { "Receivables" } else { "Payables" };
                (opening.id.clone(), Some(format!("{what} per {source}: brought in as open documents")))
            } else {
                (account_id.clone(), None)
            };
            let (debit, credit) =
                if *net > Money::ZERO { (*net, Money::ZERO) } else { (Money::ZERO, sub(Money::ZERO, *net)?) };
            lines.push(NewLine { account_id: target, debit, credit, description });
        }
        totals.opening_lines = lines.len();
        let posted = entries::post(
            &mut tx,
            NewEntry {
                date: cutover.clone(),
                description: format!("Opening balances from {source}"),
                reference: Some("OPENING".into()),
                source_type: Some(SOURCE_TYPE.into()),
                source_id: Some(id.clone()),
                lines,
            },
        )
        .await;
        match posted {
            Ok(entry) => opening_entry_id = Some(entry.id),
            Err(e) => issues.push("balances", 0, "", e.user_message()),
        }
    }

    // ── Do the open documents agree with the trial balance? ─────────────────
    let left = add(sub(receivables_balance, open_invoices)?, sub(open_bills, payables_balance)?)?;
    if !req.accept_differences {
        for (what, tb, docs, docs_name) in [
            ("Receivables", receivables_balance, open_invoices, "unpaid invoices"),
            ("Payables", payables_balance, open_bills, "unpaid bills"),
        ] {
            if tb != docs {
                issues.push(
                    "general",
                    0,
                    "",
                    format!(
                        "{what} in the trial balance ({}) and the {docs_name} ({}) differ by {}. Fix the files, or accept \
                         the difference: it stays in {OPENING_CODE} {OPENING_NAME}.",
                        money_text(tb),
                        money_text(docs),
                        money_text(sub(tb, docs)?)
                    ),
                );
            }
        }
    }

    let can_save = issues.0.is_empty();
    let mut name = format!("Moved books in from {source}, cut-over {cutover}");
    if totals.invoices + totals.bills > 0 {
        name.push_str(&format!(": {} open invoice(s), {} open bill(s)", totals.invoices, totals.bills));
    }
    let check = Check {
        debits: debits.to_json_number(),
        credits: credits.to_json_number(),
        receivables_balance: receivables_balance.to_json_number(),
        open_invoices: open_invoices.to_json_number(),
        payables_balance: payables_balance.to_json_number(),
        open_bills: open_bills.to_json_number(),
        left_in_opening_equity: left.to_json_number(),
    };
    let keep = commit && can_save;
    if keep {
        sqlx::query(r#"UPDATE "conversions" SET "openingEntryId" = ?1, "summary" = ?2 WHERE "id" = ?3"#)
            .bind(&opening_entry_id)
            .bind(json!({ "totals": &totals, "check": &check }).to_string())
            .bind(&id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
    } else {
        tx.rollback().await?;
    }
    Ok(Outcome { id: keep.then_some(id), name, totals, check, issues: issues.0, can_save })
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversionRecord {
    pub id: String,
    pub source: String,
    pub cutover_date: String,
    pub status: String,
    pub opening_entry_id: Option<String>,
    pub summary: Value,
    pub created_by_id: Option<String>,
    pub created_by_name: Option<String>,
    pub created_at: String,
    pub reversed_at: Option<String>,
}

const RECORD_SQL: &str = r#"SELECT c."id", c."source", c."cutoverDate", c."status", c."openingEntryId", c."summary",
    c."createdById", u."username" AS "createdByName", c."createdAt", c."reversedAt"
    FROM "conversions" c LEFT JOIN "users" u ON u."id" = c."createdById""#;

fn record(r: &sqlx::sqlite::SqliteRow) -> ConversionRecord {
    ConversionRecord {
        id: r.get("id"),
        source: r.get("source"),
        cutover_date: r.get("cutoverDate"),
        status: r.get("status"),
        opening_entry_id: r.get("openingEntryId"),
        summary: serde_json::from_str(&r.get::<String, _>("summary")).unwrap_or(Value::Null),
        created_by_id: r.get("createdById"),
        created_by_name: r.get("createdByName"),
        created_at: r.get("createdAt"),
        reversed_at: r.get("reversedAt"),
    }
}

pub async fn list(db: &Db) -> AppResult<Vec<ConversionRecord>> {
    let rows = sqlx::query(sqlx::AssertSqlSafe(format!(r#"{RECORD_SQL} ORDER BY c."createdAt" DESC"#)))
        .fetch_all(db.read())
        .await?;
    Ok(rows.iter().map(record).collect())
}

/// Reverses a conversion: voids its invoices and bills and reverses its opening entry, all dated
/// `today` (nothing is deleted). Refused while any of its documents has payments or notes.
/// The accounts, customers and vendors it created stay.
pub async fn reverse(db: &Db, id: &str, today: &str) -> AppResult<ConversionRecord> {
    let mut tx = db.begin().await?;
    let row = sqlx::query(sqlx::AssertSqlSafe(format!(r#"{RECORD_SQL} WHERE c."id" = ?1"#)))
        .bind(id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("Conversion with ID {id} not found")))?;
    let conv = record(&row);
    if conv.status != "ACTIVE" {
        return Err(AppError::bad_request("This conversion was already reversed"));
    }
    let items: Vec<(String, String)> =
        sqlx::query_as(r#"SELECT "recordType", "recordId" FROM "conversion_items" WHERE "conversionId" = ?1"#)
            .bind(id)
            .fetch_all(&mut *tx)
            .await?;
    let scope = Scope { user_id: None, sees_all: true };
    for (kind, record_id) in &items {
        let result = match kind.as_str() {
            "INVOICE" => invoices::void_in(&mut tx, record_id, &scope, today).await.map(|i| i.invoice_number),
            "BILL" => bills::void_in(&mut tx, record_id, today).await.map(|b| b.bill_number),
            _ => continue,
        };
        if let Err(e) = result {
            let what = if kind == "INVOICE" { "invoice" } else { "bill" };
            let number: Option<String> = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
                r#"SELECT "{}" FROM "{}" WHERE "id" = ?1"#,
                if kind == "INVOICE" { "invoiceNumber" } else { "billNumber" },
                if kind == "INVOICE" { "invoices" } else { "bills" }
            )))
            .bind(record_id)
            .fetch_optional(&mut *tx)
            .await?;
            return Err(AppError::bad_request(format!(
                "Can't reverse yet: {what} {}: {}",
                number.unwrap_or_default(),
                e.user_message()
            )));
        }
    }
    if let Some(entry) = &conv.opening_entry_id {
        entries::reverse(&mut tx, entry, today).await?;
    }
    sqlx::query(
        r#"UPDATE "conversions" SET "status" = 'REVERSED', "reversedAt" = ?1, "updatedAt" = ?1 WHERE "id" = ?2"#,
    )
    .bind(now_iso())
    .bind(id)
    .execute(&mut *tx)
    .await?;
    let row = sqlx::query(sqlx::AssertSqlSafe(format!(r#"{RECORD_SQL} WHERE c."id" = ?1"#)))
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(record(&row))
}
