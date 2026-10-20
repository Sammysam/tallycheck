//! W2 request DTOs: customers, vendors, bank accounts, invoices, bills, expenses, payments.
//! Each mirrors the reference class-validator decorators in declaration order and is checked
//! against `tests/fixtures/reference_validation.json`.

use super::{nested_array, nullable, opt_string, pagination, Rule, Validator};
use app_core::money::Money;
use app_core::AppError;
use operations::invoices::{InvoicePatch, LineInput, PAYMENT_TERMS};
use serde_json::{Map, Value};

pub const INVOICE_STATUSES: &[&str] = &["DRAFT", "POSTED", "PAID", "VOID"];
const PAYMENT_TYPES: &[&str] = &["AR", "AP"];
const PAYMENT_METHODS: &[&str] = &["CASH", "BANK_TRANSFER", "CHECK"];

fn money(m: &Map<String, Value>, key: &str) -> Option<Money> {
    m.get(key).and_then(Money::from_json)
}

// ─── Customers and vendors ──────────────────────────────────────────────────

pub(crate) fn party_rules(v: &mut Validator, optional: bool) {
    v.field("name", optional, &[Rule::IsString, Rule::IsNotEmpty])
        .field("email", true, &[Rule::IsEmail])
        .field("phone", true, &[Rule::IsString])
        .field("address", true, &[Rule::IsString])
        .field("taxId", true, &[Rule::IsString]);
}

pub fn party(body: Map<String, Value>, optional: bool) -> Result<operations::parties::PartyFields, AppError> {
    let mut v = Validator::new(body);
    party_rules(&mut v, optional);
    let m = v.finish()?;
    Ok(operations::parties::PartyFields {
        name: opt_string(&m, "name"),
        email: nullable(&m, "email"),
        phone: nullable(&m, "phone"),
        address: nullable(&m, "address"),
        tax_id: nullable(&m, "taxId"),
    })
}

// ─── Bank accounts ──────────────────────────────────────────────────────────

pub(crate) fn bank_rules(v: &mut Validator, optional: bool) {
    v.field("name", optional, &[Rule::IsString, Rule::IsNotEmpty])
        .field("bankName", true, &[Rule::IsString])
        .field("accountNumber", true, &[Rule::IsString])
        .field("accountId", optional, &[Rule::IsUuid]);
}

pub fn bank(body: Map<String, Value>, optional: bool) -> Result<operations::banking::BankFields, AppError> {
    let mut v = Validator::new(body);
    bank_rules(&mut v, optional);
    let m = v.finish()?;
    Ok(operations::banking::BankFields {
        name: opt_string(&m, "name"),
        bank_name: nullable(&m, "bankName"),
        account_number: nullable(&m, "accountNumber"),
        account_id: opt_string(&m, "accountId"),
    })
}

// ─── Invoices ───────────────────────────────────────────────────────────────

pub(crate) fn invoice_line_rules(item: &Value, prefix: String) -> (Vec<String>, Map<String, Value>) {
    let mut v = Validator::nested(item.as_object().cloned().unwrap_or_default(), prefix);
    v.field("description", false, &[Rule::IsString, Rule::IsNotEmpty])
        .field("quantity", true, &[Rule::IsMoneyNumber, Rule::MinNum(0.001, "0.001")])
        .field("unitPrice", true, &[Rule::IsMoneyNumber, Rule::MinZero])
        .field("accountId", true, &[Rule::IsUuid])
        .field("taxCodeId", true, &[Rule::IsUuid]);
    v.into_parts()
}

pub(crate) fn invoice_rules(v: &mut Validator, optional: bool) {
    v.field("customerId", true, &[Rule::IsUuid])
        .field("date", optional, &[Rule::IsDateFormat])
        .field("dueDate", true, &[Rule::IsDateFormat])
        .field("paymentTerms", true, &[Rule::IsEnum(PAYMENT_TERMS)])
        .field("poNumber", true, &[Rule::IsString])
        .field("notes", true, &[Rule::IsString])
        .field("billToName", true, &[Rule::IsString, Rule::MaxLength(160)])
        .field("billToAddress", true, &[Rule::IsString, Rule::MaxLength(500)])
        .field("paymentOptions", true, &[Rule::IsArray, Rule::IsStringEach, Rule::MaxLengthEach(300)])
        .field("signatoryName", true, &[Rule::IsString, Rule::MaxLength(120)])
        .field("signatoryTitle", true, &[Rule::IsString, Rule::MaxLength(120)])
        .field("pricesIncludeTax", true, &[Rule::IsBoolean]);
    nested_array(v, "lines", optional, 1, invoice_line_rules);
}

fn invoice_lines(m: &Map<String, Value>) -> Option<Option<Vec<LineInput>>> {
    m.get("lines").map(|v| {
        v.as_array().map(|items| {
            items
                .iter()
                .map(|l| LineInput {
                    description: l.get("description").and_then(Value::as_str).unwrap_or_default().to_string(),
                    quantity: l.get("quantity").and_then(Money::from_json),
                    unit_price: l.get("unitPrice").and_then(Money::from_json),
                    account_id: l.get("accountId").and_then(Value::as_str).map(str::to_string),
                    tax_code_id: l.get("taxCodeId").and_then(Value::as_str).map(str::to_string),
                })
                .collect()
        })
    })
}

fn string_vec(m: &Map<String, Value>, key: &str) -> Option<Option<Vec<String>>> {
    m.get(key).map(|v| v.as_array().map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect()))
}

pub fn create_invoice(body: Map<String, Value>) -> Result<operations::invoices::CreateInvoice, AppError> {
    let mut v = Validator::new(body);
    invoice_rules(&mut v, false);
    let m = v.finish()?;
    Ok(operations::invoices::CreateInvoice {
        customer_id: opt_string(&m, "customerId"),
        date: opt_string(&m, "date").unwrap_or_default(),
        due_date: opt_string(&m, "dueDate"),
        payment_terms: opt_string(&m, "paymentTerms"),
        po_number: opt_string(&m, "poNumber"),
        notes: opt_string(&m, "notes"),
        bill_to_name: opt_string(&m, "billToName"),
        bill_to_address: opt_string(&m, "billToAddress"),
        payment_options: string_vec(&m, "paymentOptions").flatten(),
        signatory_name: opt_string(&m, "signatoryName"),
        signatory_title: opt_string(&m, "signatoryTitle"),
        lines: invoice_lines(&m).flatten().unwrap_or_default(),
        prices_include_tax: m.get("pricesIncludeTax").and_then(Value::as_bool).unwrap_or(false),
    })
}

pub fn update_invoice(body: Map<String, Value>) -> Result<InvoicePatch, AppError> {
    let mut v = Validator::new(body);
    invoice_rules(&mut v, true);
    let m = v.finish()?;
    Ok(InvoicePatch {
        customer_id: nullable(&m, "customerId"),
        date: nullable(&m, "date"),
        due_date: nullable(&m, "dueDate"),
        payment_terms: nullable(&m, "paymentTerms"),
        po_number: nullable(&m, "poNumber"),
        notes: nullable(&m, "notes"),
        bill_to_name: nullable(&m, "billToName"),
        bill_to_address: nullable(&m, "billToAddress"),
        payment_options: string_vec(&m, "paymentOptions"),
        signatory_name: nullable(&m, "signatoryName"),
        signatory_title: nullable(&m, "signatoryTitle"),
        lines: invoice_lines(&m),
        prices_include_tax: m.get("pricesIncludeTax").and_then(Value::as_bool),
    })
}

pub(crate) fn status_rules(v: &mut Validator) {
    v.field("status", false, &[Rule::IsEnum(INVOICE_STATUSES)]);
}

pub fn change_status(body: Map<String, Value>) -> Result<String, AppError> {
    let mut v = Validator::new(body);
    status_rules(&mut v);
    Ok(opt_string(&v.finish()?, "status").unwrap_or_default())
}

/// `QueryInvoicesDto extends PaginationDto`: the subclass's own errors come first, then the
/// inherited page/limit errors (verified against the reference).
pub fn invoice_query(query: Map<String, Value>) -> Result<operations::invoices::InvoiceQuery, AppError> {
    let paging = pagination(&query);
    let mut v = Validator::new(query);
    v.field("q", true, &[Rule::IsString, Rule::MaxLength(120)])
        .field("status", true, &[Rule::IsEnum(INVOICE_STATUSES)])
        .field("customerId", true, &[Rule::IsUuid])
        .field("dateFrom", true, &[Rule::IsDateFormat])
        .field("dateTo", true, &[Rule::IsDateFormat]);
    let (errors, m) = v.into_parts();
    let (page, limit) = match paging {
        Ok(pl) => pl,
        Err(AppError::BadRequest(app_core::ErrorMessage::List(e))) => {
            let mut all = errors;
            all.extend(e);
            return Err(AppError::validation(all));
        }
        Err(other) => return Err(other),
    };
    if !errors.is_empty() {
        return Err(AppError::validation(errors));
    }
    Ok(operations::invoices::InvoiceQuery {
        page,
        limit,
        q: opt_string(&m, "q"),
        status: opt_string(&m, "status"),
        customer_id: opt_string(&m, "customerId"),
        date_from: opt_string(&m, "dateFrom"),
        date_to: opt_string(&m, "dateTo"),
    })
}

// ─── Bills ──────────────────────────────────────────────────────────────────

pub(crate) fn bill_line_rules(item: &Value, prefix: String) -> (Vec<String>, Map<String, Value>) {
    let mut v = Validator::nested(item.as_object().cloned().unwrap_or_default(), prefix);
    v.field("description", false, &[Rule::IsString, Rule::IsNotEmpty])
        .field("quantity", false, &[Rule::IsMoneyNumber, Rule::MinNum(0.001, "0.001")])
        .field("unitPrice", false, &[Rule::IsMoneyNumber, Rule::MinZero])
        .field("accountId", false, &[Rule::IsUuid])
        .field("taxCodeId", true, &[Rule::IsUuid]);
    v.into_parts()
}

pub(crate) fn bill_rules(v: &mut Validator) {
    v.field("vendorId", false, &[Rule::IsUuid])
        .field("date", false, &[Rule::IsDateFormat])
        .field("dueDate", true, &[Rule::IsDateFormat])
        .field("notes", true, &[Rule::IsString])
        .field("pricesIncludeTax", true, &[Rule::IsBoolean]);
    nested_array(v, "lines", false, 1, bill_line_rules);
}

pub fn create_bill(body: Map<String, Value>) -> Result<operations::bills::CreateBill, AppError> {
    let mut v = Validator::new(body);
    bill_rules(&mut v);
    let m = v.finish()?;
    let lines = m
        .get("lines")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .map(|l| operations::bills::BillLineInput {
                    description: l.get("description").and_then(Value::as_str).unwrap_or_default().to_string(),
                    quantity: l.get("quantity").and_then(Money::from_json).unwrap_or_default(),
                    unit_price: l.get("unitPrice").and_then(Money::from_json).unwrap_or_default(),
                    account_id: l.get("accountId").and_then(Value::as_str).unwrap_or_default().to_string(),
                    tax_code_id: l.get("taxCodeId").and_then(Value::as_str).map(str::to_string),
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(operations::bills::CreateBill {
        vendor_id: opt_string(&m, "vendorId").unwrap_or_default(),
        date: opt_string(&m, "date").unwrap_or_default(),
        due_date: opt_string(&m, "dueDate"),
        notes: opt_string(&m, "notes"),
        lines,
        prices_include_tax: m.get("pricesIncludeTax").and_then(Value::as_bool).unwrap_or(false),
    })
}

// ─── Expenses ───────────────────────────────────────────────────────────────

pub(crate) fn expense_rules(v: &mut Validator) {
    v.field("date", false, &[Rule::IsDateFormat])
        .field("description", false, &[Rule::IsString, Rule::IsNotEmpty])
        .field("amount", false, &[Rule::IsMoneyNumber, Rule::MinNum(0.01, "0.01")])
        .field("expenseAccountId", false, &[Rule::IsUuid])
        .field("paymentAccountId", false, &[Rule::IsUuid])
        .field("receiptUrl", true, &[Rule::IsString])
        .field("taxCodeId", true, &[Rule::IsUuid]);
}

pub fn create_expense(body: Map<String, Value>) -> Result<operations::expenses::CreateExpense, AppError> {
    let mut v = Validator::new(body);
    expense_rules(&mut v);
    let m = v.finish()?;
    Ok(operations::expenses::CreateExpense {
        date: opt_string(&m, "date").unwrap_or_default(),
        description: opt_string(&m, "description").unwrap_or_default(),
        amount: money(&m, "amount").unwrap_or_default(),
        expense_account_id: opt_string(&m, "expenseAccountId").unwrap_or_default(),
        payment_account_id: opt_string(&m, "paymentAccountId").unwrap_or_default(),
        receipt_url: opt_string(&m, "receiptUrl"),
        tax_code_id: opt_string(&m, "taxCodeId"),
    })
}

// ─── Payments ───────────────────────────────────────────────────────────────

pub(crate) fn payment_rules(v: &mut Validator) {
    v.field("type", false, &[Rule::IsEnum(PAYMENT_TYPES)])
        .field("invoiceId", true, &[Rule::IsUuid])
        .field("billId", true, &[Rule::IsUuid])
        .field("customerId", true, &[Rule::IsUuid])
        .field("vendorId", true, &[Rule::IsUuid])
        .field("date", false, &[Rule::IsDateFormat])
        .field("amount", false, &[Rule::IsMoneyNumber, Rule::MinNum(0.01, "0.01")])
        .field("paymentMethod", false, &[Rule::IsEnum(PAYMENT_METHODS)])
        .field("paymentAccountId", false, &[Rule::IsUuid])
        .field("notes", true, &[Rule::IsString]);
}

pub fn create_payment(body: Map<String, Value>) -> Result<operations::payments::CreatePayment, AppError> {
    let mut v = Validator::new(body);
    payment_rules(&mut v);
    let m = v.finish()?;
    Ok(operations::payments::CreatePayment {
        kind: opt_string(&m, "type").unwrap_or_default(),
        invoice_id: opt_string(&m, "invoiceId"),
        bill_id: opt_string(&m, "billId"),
        customer_id: opt_string(&m, "customerId"),
        vendor_id: opt_string(&m, "vendorId"),
        date: opt_string(&m, "date").unwrap_or_default(),
        amount: money(&m, "amount").unwrap_or_default(),
        payment_method: opt_string(&m, "paymentMethod").unwrap_or_default(),
        payment_account_id: opt_string(&m, "paymentAccountId").unwrap_or_default(),
        notes: opt_string(&m, "notes"),
    })
}

/// D21: a credit note (`kind` AR, against `invoiceId`) or debit note (AP, against `billId`).
pub fn create_note(body: Map<String, Value>, kind: &str) -> Result<operations::credit_notes::CreateNote, AppError> {
    let doc_field = if kind == operations::credit_notes::AR { "invoiceId" } else { "billId" };
    let mut v = Validator::new(body);
    v.field(doc_field, false, &[Rule::IsUuid])
        .field("date", false, &[Rule::IsDateFormat])
        .field("reason", false, &[Rule::IsString, Rule::IsNotEmpty, Rule::MaxLength(500)])
        .field("amount", false, &[Rule::IsMoneyNumber, Rule::MinNum(0.01, "0.01")])
        .field("accountId", false, &[Rule::IsUuid])
        .field("taxCodeId", true, &[Rule::IsUuid]);
    let m = v.finish()?;
    Ok(operations::credit_notes::CreateNote {
        kind: kind.to_string(),
        document_id: opt_string(&m, doc_field).unwrap_or_default(),
        date: opt_string(&m, "date").unwrap_or_default(),
        reason: opt_string(&m, "reason").unwrap_or_default().trim().to_string(),
        amount: money(&m, "amount").unwrap_or_default(),
        account_id: opt_string(&m, "accountId").unwrap_or_default(),
        tax_code_id: opt_string(&m, "taxCodeId"),
    })
}

// ─── Tax codes (D26) ────────────────────────────────────────────────────────

/// A percentage from 0 to 100 with at most four decimals, as ten-thousandths (8.875 → 88750).
/// Read from the JSON text, so no floating point is involved.
fn rate_of(v: &Value) -> Option<i64> {
    let text = match v {
        Value::Number(n) => n.to_string(),
        _ => return None,
    };
    let (whole, frac) = text.split_once('.').unwrap_or((&text, ""));
    if whole.is_empty() || whole.starts_with('-') || frac.len() > 4 || text.contains(['e', 'E']) {
        return None;
    }
    let whole: i64 = whole.parse().ok()?;
    let frac: i64 = if frac.is_empty() { 0 } else { format!("{frac:0<4}").parse().ok()? };
    let rate = whole.checked_mul(10_000)?.checked_add(frac)?;
    (0..=1_000_000).contains(&rate).then_some(rate)
}

pub fn tax_code(body: Map<String, Value>, optional: bool) -> Result<operations::tax::TaxCodeInput, AppError> {
    let mut v = Validator::new(body);
    v.field("name", optional, &[Rule::IsString, Rule::IsNotEmpty, Rule::MaxLength(60)])
        .field("rate", optional, &[Rule::IsFiniteNumber])
        .field("description", true, &[Rule::IsString, Rule::MaxLength(200)])
        .field("salesAccountId", optional, &[Rule::IsUuid])
        .field("purchaseAccountId", optional, &[Rule::IsUuid])
        .field("isActive", true, &[Rule::IsBoolean]);
    let m = v.finish()?;
    let rate = match m.get("rate") {
        None | Some(Value::Null) => None,
        Some(r) => Some(rate_of(r).ok_or_else(|| {
            AppError::validation(vec!["rate must be a percentage from 0 to 100 with at most 4 decimal places".into()])
        })?),
    };
    Ok(operations::tax::TaxCodeInput {
        name: opt_string(&m, "name").map(|n| n.trim().to_string()),
        rate,
        description: nullable(&m, "description"),
        sales_account_id: opt_string(&m, "salesAccountId"),
        purchase_account_id: opt_string(&m, "purchaseAccountId"),
        is_active: m.get("isActive").and_then(Value::as_bool),
    })
}
