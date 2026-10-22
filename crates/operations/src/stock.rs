//! D38: products & services, and stock kept at average cost.
//!
//! * An item is INVENTORY (counted and valued), SERVICE or NON_INVENTORY (sold / bought, not
//!   counted). Invoice and bill lines may name an item.
//! * Stock comes in when a bill with the item is posted, at the line's cost without tax in the
//!   home currency (the amount debited to the inventory account).
//! * Stock goes out when an invoice with the item is posted: cost of goods sold is the average
//!   cost (stock value × quantity ÷ quantity on hand, rounded half-up; selling the last units
//!   takes the whole remaining value). The invoice's journal entry debits COGS and credits the
//!   inventory account by that value. Posting is refused when there is not enough stock
//!   (the user's decision, 2026-10-22).
//! * Voiding / unposting reverses exactly what the document moved. A stock adjustment moves
//!   stock against an account the user chooses.
//!
//! Every movement is a `stock_moves` row whose value is exactly what was posted to the item's
//! inventory account, so the items' stock value always equals that account's balance from them.

use crate::{money_num, overflow};
use app_core::ids::new_id;
use app_core::money::Money;
use app_core::time::now_iso;
use app_core::{AppError, AppResult};
use database::{Db, Tx};
use ledger::{entries, AccountType, NewEntry, NewLine};
use serde_json::{json, Value};
use sqlx::sqlite::SqliteRow;
use sqlx::{Row, SqliteConnection};

pub const INVENTORY: &str = "INVENTORY";
pub const TYPES: &[&str] = &["INVENTORY", "SERVICE", "NON_INVENTORY"];

#[derive(Debug, Clone)]
pub struct Item {
    pub id: String,
    pub sku: Option<String>,
    pub name: String,
    pub item_type: String,
    pub description: Option<String>,
    pub sale_price: Option<Money>,
    pub purchase_cost: Option<Money>,
    pub income_account_id: Option<String>,
    pub expense_account_id: Option<String>,
    pub inventory_account_id: Option<String>,
    pub cogs_account_id: Option<String>,
    pub tax_code_id: Option<String>,
    /// Hundredths, like quantities on lines.
    pub reorder_point: Option<Money>,
    pub quantity_on_hand: Money,
    pub stock_value: Money,
    pub is_active: bool,
    pub created_at: String,
    pub updated_at: String,
}

impl Item {
    pub fn counted(&self) -> bool {
        self.item_type == INVENTORY
    }

    /// Cost of one unit, rounded to the cent (for display; postings use [`issue_value`]).
    pub fn average_cost(&self) -> Option<Money> {
        let q = self.quantity_on_hand.cents();
        (q > 0).then(|| Money::from_cents(div_half_up(self.stock_value.cents() as i128 * 100, q as i128) as i64))
    }

    pub fn low_stock(&self) -> bool {
        self.counted() && self.is_active && self.reorder_point.is_some_and(|r| self.quantity_on_hand <= r)
    }

    pub fn to_json(&self) -> Value {
        let m = |v: Option<Money>| v.map(money_num).unwrap_or(Value::Null);
        json!({
            "id": self.id,
            "sku": self.sku,
            "name": self.name,
            "type": self.item_type,
            "description": self.description,
            "salePrice": m(self.sale_price),
            "purchaseCost": m(self.purchase_cost),
            "incomeAccountId": self.income_account_id,
            "expenseAccountId": self.expense_account_id,
            "inventoryAccountId": self.inventory_account_id,
            "cogsAccountId": self.cogs_account_id,
            "taxCodeId": self.tax_code_id,
            "reorderPoint": m(self.reorder_point),
            "quantityOnHand": money_num(self.quantity_on_hand),
            "stockValue": money_num(self.stock_value),
            "averageCost": m(self.average_cost()),
            "lowStock": self.low_stock(),
            "isActive": self.is_active,
            "createdAt": self.created_at,
            "updatedAt": self.updated_at,
        })
    }
}

/// a ÷ b rounded half-up (away from zero), for b > 0.
fn div_half_up(a: i128, b: i128) -> i128 {
    let (q, r) = (a / b, a % b);
    if r.abs() * 2 >= b {
        q + a.signum()
    } else {
        q
    }
}

const COLUMNS: &str = r#""id","sku","name","type","description","salePrice","purchaseCost","incomeAccountId","expenseAccountId",
    "inventoryAccountId","cogsAccountId","taxCodeId","reorderPoint","quantityOnHand","stockValue","isActive","createdAt","updatedAt""#;

fn from_row(r: &SqliteRow) -> AppResult<Item> {
    let money = |c: &str| -> AppResult<Option<Money>> { Ok(r.try_get::<Option<i64>, _>(c)?.map(Money::from_cents)) };
    Ok(Item {
        id: r.try_get("id")?,
        sku: r.try_get("sku")?,
        name: r.try_get("name")?,
        item_type: r.try_get("type")?,
        description: r.try_get("description")?,
        sale_price: money("salePrice")?,
        purchase_cost: money("purchaseCost")?,
        income_account_id: r.try_get("incomeAccountId")?,
        expense_account_id: r.try_get("expenseAccountId")?,
        inventory_account_id: r.try_get("inventoryAccountId")?,
        cogs_account_id: r.try_get("cogsAccountId")?,
        tax_code_id: r.try_get("taxCodeId")?,
        reorder_point: money("reorderPoint")?,
        quantity_on_hand: Money::from_cents(r.try_get("quantityOnHand")?),
        stock_value: Money::from_cents(r.try_get("stockValue")?),
        is_active: r.try_get::<i64, _>("isActive")? == 1,
        created_at: r.try_get("createdAt")?,
        updated_at: r.try_get("updatedAt")?,
    })
}

pub async fn get(conn: &mut SqliteConnection, id: &str) -> AppResult<Option<Item>> {
    let row =
        sqlx::query(&format!(r#"SELECT {COLUMNS} FROM "items" WHERE "id" = ?1"#)).bind(id).fetch_optional(conn).await?;
    row.as_ref().map(from_row).transpose()
}

pub async fn find_one(conn: &mut SqliteConnection, id: &str) -> AppResult<Item> {
    get(conn, id).await?.ok_or_else(|| AppError::NotFound(format!("Product or service with ID {id} not found")))
}

pub async fn find_all(db: &Db) -> AppResult<Vec<Item>> {
    let rows =
        sqlx::query(&format!(r#"SELECT {COLUMNS} FROM "items" ORDER BY "isActive" DESC, "name" COLLATE NOCASE"#))
            .fetch_all(db.read())
            .await?;
    rows.iter().map(from_row).collect()
}

/// A PATCH-style set of fields: `None` = not sent, `Some(None)` = cleared.
#[derive(Debug, Clone, Default)]
pub struct ItemFields {
    pub sku: Option<Option<String>>,
    pub name: Option<String>,
    pub item_type: Option<String>,
    pub description: Option<Option<String>>,
    pub sale_price: Option<Option<Money>>,
    pub purchase_cost: Option<Option<Money>>,
    pub income_account_id: Option<Option<String>>,
    pub expense_account_id: Option<Option<String>>,
    pub inventory_account_id: Option<Option<String>>,
    pub cogs_account_id: Option<Option<String>>,
    pub tax_code_id: Option<Option<String>>,
    pub reorder_point: Option<Option<Money>>,
    pub is_active: Option<bool>,
}

async fn account_of(conn: &mut SqliteConnection, id: &str, allowed: &[AccountType], what: &str) -> AppResult<()> {
    let a = ledger::accounts::find_one(conn, id).await?;
    if !allowed.contains(&a.account_type) {
        return Err(AppError::bad_request(format!("{} {} cannot be the {what}", a.code, a.name)));
    }
    if a.currency.is_some() {
        return Err(AppError::bad_request(format!(
            "{} {} is kept in {}; choose a home-currency account",
            a.code,
            a.name,
            a.currency.unwrap_or_default()
        )));
    }
    Ok(())
}

async fn by_code(conn: &mut SqliteConnection, code: &str) -> Option<String> {
    ledger::accounts::find_by_code(conn, code).await.ok().map(|a| a.id)
}

fn unique_violation(e: &sqlx::Error) -> bool {
    matches!(e, sqlx::Error::Database(d) if d.is_unique_violation())
}

/// Checks and completes an item before it is saved: names and SKUs unique, accounts of the right
/// kind, and the accounts an inventory item needs (1200 Inventory and 5000 Cost of Goods Sold by
/// default, 4000 Revenue for sales).
async fn settle(conn: &mut SqliteConnection, item: &mut Item, moved: bool) -> AppResult<()> {
    item.name = item.name.trim().to_string();
    if item.name.is_empty() {
        return Err(AppError::bad_request("Name the product or service"));
    }
    item.sku = item.sku.as_ref().map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    if !TYPES.contains(&item.item_type.as_str()) {
        return Err(AppError::bad_request("type must be one of INVENTORY, SERVICE, NON_INVENTORY"));
    }
    if item.income_account_id.is_none() {
        item.income_account_id = by_code(conn, "4000").await;
    }
    if item.counted() {
        if item.inventory_account_id.is_none() {
            item.inventory_account_id = by_code(conn, "1200").await;
        }
        if item.cogs_account_id.is_none() {
            item.cogs_account_id = by_code(conn, "5000").await;
        }
        if item.inventory_account_id.is_none() || item.cogs_account_id.is_none() {
            return Err(AppError::bad_request(
                "A stock item needs an inventory account and a cost of goods sold account (1200 and 5000 by default)",
            ));
        }
    } else {
        if moved {
            return Err(AppError::bad_request("This item's stock has moved, so it stays a stock item"));
        }
        item.inventory_account_id = None;
        item.cogs_account_id = None;
    }
    if let Some(a) = item.income_account_id.clone() {
        account_of(conn, &a, &[AccountType::Revenue], "income account").await?;
    }
    if let Some(a) = item.expense_account_id.clone() {
        account_of(conn, &a, &[AccountType::Expense, AccountType::Asset], "purchase account").await?;
    }
    if let Some(a) = item.inventory_account_id.clone() {
        account_of(conn, &a, &[AccountType::Asset], "inventory account").await?;
    }
    if let Some(a) = item.cogs_account_id.clone() {
        account_of(conn, &a, &[AccountType::Expense], "cost of goods sold account").await?;
    }
    if let Some(t) = item.tax_code_id.clone() {
        crate::tax::usable(conn, &t).await?;
    }
    Ok(())
}

fn apply(item: &mut Item, f: ItemFields) {
    if let Some(v) = f.sku {
        item.sku = v;
    }
    if let Some(v) = f.name {
        item.name = v;
    }
    if let Some(v) = f.item_type {
        item.item_type = v;
    }
    if let Some(v) = f.description {
        item.description = v.filter(|d| !d.trim().is_empty());
    }
    if let Some(v) = f.sale_price {
        item.sale_price = v;
    }
    if let Some(v) = f.purchase_cost {
        item.purchase_cost = v;
    }
    if let Some(v) = f.income_account_id {
        item.income_account_id = v;
    }
    if let Some(v) = f.expense_account_id {
        item.expense_account_id = v;
    }
    if let Some(v) = f.inventory_account_id {
        item.inventory_account_id = v;
    }
    if let Some(v) = f.cogs_account_id {
        item.cogs_account_id = v;
    }
    if let Some(v) = f.tax_code_id {
        item.tax_code_id = v;
    }
    if let Some(v) = f.reorder_point {
        item.reorder_point = v;
    }
    if let Some(v) = f.is_active {
        item.is_active = v;
    }
}

async fn save(tx: &mut Tx, item: &Item, insert: bool) -> AppResult<()> {
    let sql = if insert {
        r#"INSERT INTO "items" ("sku","name","type","description","salePrice","purchaseCost","incomeAccountId","expenseAccountId",
             "inventoryAccountId","cogsAccountId","taxCodeId","reorderPoint","isActive","createdAt","updatedAt","id")
           VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?14,?15)"#
    } else {
        r#"UPDATE "items" SET "sku"=?1,"name"=?2,"type"=?3,"description"=?4,"salePrice"=?5,"purchaseCost"=?6,"incomeAccountId"=?7,
             "expenseAccountId"=?8,"inventoryAccountId"=?9,"cogsAccountId"=?10,"taxCodeId"=?11,"reorderPoint"=?12,"isActive"=?13,
             "updatedAt"=?14 WHERE "id"=?15"#
    };
    let res = sqlx::query(sql)
        .bind(&item.sku)
        .bind(&item.name)
        .bind(&item.item_type)
        .bind(&item.description)
        .bind(item.sale_price.map(Money::cents))
        .bind(item.purchase_cost.map(Money::cents))
        .bind(&item.income_account_id)
        .bind(&item.expense_account_id)
        .bind(&item.inventory_account_id)
        .bind(&item.cogs_account_id)
        .bind(&item.tax_code_id)
        .bind(item.reorder_point.map(Money::cents))
        .bind(item.is_active)
        .bind(now_iso())
        .bind(&item.id)
        .execute(&mut **tx)
        .await;
    match res {
        Err(e) if unique_violation(&e) => Err(AppError::bad_request(format!(
            "Another product or service already has the name or SKU of \"{}\"",
            item.name
        ))),
        other => other.map(|_| ()).map_err(Into::into),
    }
}

pub async fn create(db: &Db, f: ItemFields) -> AppResult<Item> {
    let mut tx = db.begin().await?;
    let now = now_iso();
    let mut item = Item {
        id: new_id(),
        sku: None,
        name: String::new(),
        item_type: INVENTORY.into(),
        description: None,
        sale_price: None,
        purchase_cost: None,
        income_account_id: None,
        expense_account_id: None,
        inventory_account_id: None,
        cogs_account_id: None,
        tax_code_id: None,
        reorder_point: None,
        quantity_on_hand: Money::ZERO,
        stock_value: Money::ZERO,
        is_active: true,
        created_at: now.clone(),
        updated_at: now,
    };
    apply(&mut item, f);
    settle(&mut tx, &mut item, false).await?;
    save(&mut tx, &item, true).await?;
    let out = find_one(&mut tx, &item.id).await?;
    tx.commit().await?;
    Ok(out)
}

async fn has_moves(conn: &mut SqliteConnection, id: &str) -> AppResult<bool> {
    let n: Option<i64> = sqlx::query_scalar(r#"SELECT 1 FROM "stock_moves" WHERE "itemId" = ?1 LIMIT 1"#)
        .bind(id)
        .fetch_optional(conn)
        .await?;
    Ok(n.is_some())
}

pub async fn update(db: &Db, id: &str, f: ItemFields) -> AppResult<Item> {
    let mut tx = db.begin().await?;
    let before = find_one(&mut tx, id).await?;
    let moved = has_moves(&mut tx, id).await?;
    let mut item = before.clone();
    apply(&mut item, f);
    if moved && (item.inventory_account_id != before.inventory_account_id || item.item_type != before.item_type) {
        return Err(AppError::bad_request(
            "This item's stock has moved, so its type and inventory account stay as they are",
        ));
    }
    settle(&mut tx, &mut item, moved && before.counted()).await?;
    save(&mut tx, &item, false).await?;
    let out = find_one(&mut tx, id).await?;
    tx.commit().await?;
    Ok(out)
}

/// Deletes an item no document or stock movement uses; otherwise make it inactive.
pub async fn remove(db: &Db, id: &str) -> AppResult<()> {
    let mut tx = db.begin().await?;
    find_one(&mut tx, id).await?;
    match sqlx::query(r#"DELETE FROM "items" WHERE "id" = ?1"#).bind(id).execute(&mut *tx).await {
        Err(sqlx::Error::Database(e)) if e.is_foreign_key_violation() => {
            return Err(AppError::bad_request(
                "This product or service is used on invoices, bills or stock movements. Make it inactive instead.",
            ))
        }
        other => other?,
    };
    tx.commit().await?;
    Ok(())
}

// ─── Movements ──────────────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
async fn record(
    tx: &mut Tx,
    item: &Item,
    date: &str,
    quantity: Money,
    value: Money,
    source_type: &str,
    source_id: Option<&str>,
    journal_entry_id: Option<&str>,
    note: Option<&str>,
) -> AppResult<()> {
    let q = item.quantity_on_hand.checked_add(quantity).ok_or_else(overflow)?;
    let v = item.stock_value.checked_add(value).ok_or_else(overflow)?;
    if q < Money::ZERO || v < Money::ZERO || (q == Money::ZERO && v != Money::ZERO) {
        return Err(AppError::internal(format!("stock of {} would become {q} worth {v}", item.name)));
    }
    sqlx::query(
        r#"INSERT INTO "stock_moves" ("id","itemId","date","quantity","value","sourceType","sourceId","journalEntryId","note","createdAt")
           VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)"#,
    )
    .bind(new_id())
    .bind(&item.id)
    .bind(date)
    .bind(quantity.cents())
    .bind(value.cents())
    .bind(source_type)
    .bind(source_id)
    .bind(journal_entry_id)
    .bind(note)
    .bind(now_iso())
    .execute(&mut **tx)
    .await?;
    sqlx::query(r#"UPDATE "items" SET "quantityOnHand" = ?1, "stockValue" = ?2, "updatedAt" = ?3 WHERE "id" = ?4"#)
        .bind(q.cents())
        .bind(v.cents())
        .bind(now_iso())
        .bind(&item.id)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

fn qty_text(q: Money) -> String {
    let s = q.to_db_string();
    s.strip_suffix(".00").map(str::to_string).unwrap_or(s)
}

/// What taking `quantity` out of stock is worth at average cost; all of it for the last units.
pub fn issue_value(item: &Item, quantity: Money) -> AppResult<Money> {
    if quantity > item.quantity_on_hand {
        return Err(AppError::bad_request(format!(
            "Not enough stock of {}: {} on hand, {} needed. Post the bill for it first, or adjust the stock.",
            item.name,
            qty_text(item.quantity_on_hand),
            qty_text(quantity)
        )));
    }
    if quantity == item.quantity_on_hand {
        return Ok(item.stock_value);
    }
    let v =
        div_half_up(item.stock_value.cents() as i128 * quantity.cents() as i128, item.quantity_on_hand.cents() as i128);
    Ok(Money::from_cents(v as i64))
}

/// One invoice line's stock taken out: its cost and the accounts it moves between.
pub struct Issue {
    pub value: Money,
    pub cogs_account_id: String,
    pub inventory_account_id: String,
    pub name: String,
}

/// Takes stock out for an invoice line (before its entry exists; [`link_entry`] attaches it).
pub(crate) async fn issue(
    tx: &mut Tx,
    item_id: &str,
    quantity: Money,
    date: &str,
    invoice_id: &str,
) -> AppResult<Option<Issue>> {
    let item = find_one(tx, item_id).await?;
    if !item.counted() {
        return Ok(None);
    }
    if quantity <= Money::ZERO {
        return Err(AppError::bad_request(format!("The quantity of {} must be more than 0", item.name)));
    }
    let value = issue_value(&item, quantity)?;
    let neg = |m: Money| Money::ZERO.checked_sub(m).ok_or_else(overflow);
    record(tx, &item, date, neg(quantity)?, neg(value)?, "INVOICE", Some(invoice_id), None, None).await?;
    Ok(Some(Issue {
        value,
        cogs_account_id: item.cogs_account_id.clone().unwrap_or_default(),
        inventory_account_id: item.inventory_account_id.clone().unwrap_or_default(),
        name: item.name,
    }))
}

/// Puts stock in for a bill line, at its home-currency cost without tax.
pub(crate) async fn receive(
    tx: &mut Tx,
    item_id: &str,
    quantity: Money,
    value: Money,
    date: &str,
    bill_id: &str,
    entry_id: &str,
) -> AppResult<()> {
    let item = find_one(tx, item_id).await?;
    if !item.counted() {
        return Ok(());
    }
    if quantity <= Money::ZERO {
        return Err(AppError::bad_request(format!("The quantity of {} must be more than 0", item.name)));
    }
    record(tx, &item, date, quantity, value, "BILL", Some(bill_id), Some(entry_id), None).await
}

/// Attaches the posted entry to the stock a document has just moved.
pub(crate) async fn link_entry(tx: &mut Tx, source_type: &str, source_id: &str, entry_id: &str) -> AppResult<()> {
    sqlx::query(
        r#"UPDATE "stock_moves" SET "journalEntryId" = ?1 WHERE "sourceType" = ?2 AND "sourceId" = ?3 AND "journalEntryId" IS NULL"#,
    )
    .bind(entry_id)
    .bind(source_type)
    .bind(source_id)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

/// Undoes what a document still has moved (its own moves and earlier undos netted), when it is
/// voided or returned to draft. `reversal_type` is BILL_VOID or INVOICE_REVERSAL.
pub(crate) async fn undo_source(
    tx: &mut Tx,
    source_type: &str,
    reversal_type: &str,
    source_id: &str,
    date: &str,
    entry_id: &str,
) -> AppResult<()> {
    let nets: Vec<(String, i64, i64)> = sqlx::query_as(
        r#"SELECT "itemId", SUM("quantity"), SUM("value") FROM "stock_moves"
           WHERE "sourceId" = ?1 AND "sourceType" IN (?2, ?3) GROUP BY "itemId""#,
    )
    .bind(source_id)
    .bind(source_type)
    .bind(reversal_type)
    .fetch_all(&mut **tx)
    .await?;
    for (item_id, q, v) in nets {
        if q == 0 && v == 0 {
            continue;
        }
        let item = find_one(tx, &item_id).await?;
        let (q, v) = (Money::from_cents(-q), Money::from_cents(-v));
        let left_q = item.quantity_on_hand.checked_add(q).ok_or_else(overflow)?;
        let left_v = item.stock_value.checked_add(v).ok_or_else(overflow)?;
        if left_q < Money::ZERO || left_v < Money::ZERO || (left_q == Money::ZERO && left_v != Money::ZERO) {
            return Err(AppError::bad_request(format!(
                "Some of the {} on this bill has been sold since. Adjust the stock instead of voiding the bill.",
                item.name
            )));
        }
        record(tx, &item, date, q, v, reversal_type, Some(source_id), Some(entry_id), None).await?;
    }
    Ok(())
}

pub struct Adjustment {
    pub date: String,
    /// The change in quantity (hundredths): positive adds stock, negative removes it.
    pub quantity: Money,
    /// For stock added: the cost of one unit (default: the average cost, then the purchase cost).
    pub unit_cost: Option<Money>,
    /// The other side of the entry: e.g. 5000 for shrinkage, an equity account for opening stock.
    pub account_id: String,
    pub note: Option<String>,
}

/// A stock count or correction: moves stock and posts the value against the chosen account.
pub async fn adjust(db: &Db, id: &str, a: Adjustment) -> AppResult<Item> {
    let mut tx = db.begin().await?;
    let item = find_one(&mut tx, id).await?;
    if !item.counted() {
        return Err(AppError::bad_request(format!("{} is not a stock item", item.name)));
    }
    if a.quantity == Money::ZERO {
        return Err(AppError::bad_request("Enter how much the quantity changes"));
    }
    let other = ledger::accounts::find_one(&mut tx, &a.account_id).await?;
    let inventory = item.inventory_account_id.clone().unwrap_or_default();
    if other.id == inventory {
        return Err(AppError::bad_request("Choose an account other than the inventory account"));
    }
    if other.currency.is_some() {
        return Err(AppError::bad_request("Choose a home-currency account"));
    }
    let (value, adding) = if a.quantity > Money::ZERO {
        let unit = a
            .unit_cost
            .or_else(|| item.average_cost())
            .or(item.purchase_cost)
            .ok_or_else(|| AppError::bad_request("Enter the cost of one unit"))?;
        (Money::line_amount(a.quantity, unit).ok_or_else(overflow)?, true)
    } else {
        (issue_value(&item, Money::ZERO.checked_sub(a.quantity).ok_or_else(overflow)?)?, false)
    };
    let note = a.note.as_deref().map(str::trim).filter(|n| !n.is_empty());
    let move_id = new_id();
    let entry_id = if value > Money::ZERO {
        let (debit, credit) =
            if adding { (inventory.clone(), other.id.clone()) } else { (other.id.clone(), inventory.clone()) };
        let entry = entries::post(
            &mut tx,
            NewEntry {
                date: a.date.clone(),
                description: format!(
                    "Stock adjustment: {}{}",
                    item.name,
                    note.map(|n| format!(" — {n}")).unwrap_or_default()
                ),
                reference: item.sku.clone(),
                source_type: Some("STOCK_ADJUSTMENT".into()),
                source_id: Some(move_id.clone()),
                lines: vec![
                    NewLine { account_id: debit, debit: value, credit: Money::ZERO, description: None },
                    NewLine { account_id: credit, debit: Money::ZERO, credit: value, description: None },
                ],
            },
        )
        .await?;
        Some(entry.id)
    } else {
        None
    };
    let signed = if adding { value } else { Money::ZERO.checked_sub(value).ok_or_else(overflow)? };
    record(&mut tx, &item, &a.date, a.quantity, signed, "ADJUSTMENT", Some(&move_id), entry_id.as_deref(), note)
        .await?;
    let out = find_one(&mut tx, id).await?;
    tx.commit().await?;
    Ok(out)
}

/// An item's movements, newest first, with the document each came from.
pub async fn moves(db: &Db, id: &str) -> AppResult<Value> {
    let mut conn = db.read().acquire().await?;
    find_one(&mut conn, id).await?;
    let rows = sqlx::query(
        r#"SELECT m."id", m."date", m."quantity", m."value", m."sourceType", m."sourceId", m."journalEntryId", m."note",
                  COALESCE(i."invoiceNumber", b."billNumber") AS "number"
             FROM "stock_moves" m
             LEFT JOIN "invoices" i ON m."sourceType" IN ('INVOICE','INVOICE_REVERSAL') AND i."id" = m."sourceId"
             LEFT JOIN "bills" b ON m."sourceType" IN ('BILL','BILL_VOID') AND b."id" = m."sourceId"
            WHERE m."itemId" = ?1 ORDER BY m."date" DESC, m."createdAt" DESC LIMIT 500"#,
    )
    .bind(id)
    .fetch_all(&mut *conn)
    .await?;
    let out = rows
        .iter()
        .map(|r| {
            Ok(json!({
                "id": r.try_get::<String, _>("id")?,
                "date": r.try_get::<String, _>("date")?,
                "quantity": money_num(Money::from_cents(r.try_get("quantity")?)),
                "value": money_num(Money::from_cents(r.try_get("value")?)),
                "sourceType": r.try_get::<String, _>("sourceType")?,
                "sourceId": r.try_get::<Option<String>, _>("sourceId")?,
                "journalEntryId": r.try_get::<Option<String>, _>("journalEntryId")?,
                "note": r.try_get::<Option<String>, _>("note")?,
                "number": r.try_get::<Option<String>, _>("number")?,
            }))
        })
        .collect::<AppResult<Vec<_>>>()?;
    Ok(Value::Array(out))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(q: i64, v: i64) -> Item {
        Item {
            id: "i".into(),
            sku: None,
            name: "Tea".into(),
            item_type: INVENTORY.into(),
            description: None,
            sale_price: None,
            purchase_cost: None,
            income_account_id: None,
            expense_account_id: None,
            inventory_account_id: None,
            cogs_account_id: None,
            tax_code_id: None,
            reorder_point: Some(Money::from_cents(500)),
            quantity_on_hand: Money::from_cents(q),
            stock_value: Money::from_cents(v),
            is_active: true,
            created_at: String::new(),
            updated_at: String::new(),
        }
    }

    #[test]
    fn average_cost_rounds_half_up_and_the_last_units_take_the_rest() {
        // 3 units worth 100.00: one unit is 33.33, two are 66.67, all three are 100.00.
        let it = item(300, 10000);
        assert_eq!(issue_value(&it, Money::from_cents(100)).ok().map(Money::cents), Some(3333));
        assert_eq!(issue_value(&it, Money::from_cents(200)).ok().map(Money::cents), Some(6667));
        assert_eq!(issue_value(&it, Money::from_cents(300)).ok().map(Money::cents), Some(10000));
        assert_eq!(it.average_cost().map(Money::cents), Some(3333));
        assert!(issue_value(&it, Money::from_cents(301)).is_err());
        assert!(it.low_stock());
        assert!(!item(600, 1).low_stock());
    }
}
