//! The tools Claude sees, each a call to the app's own API made with the key.

use serde_json::{json, Map, Value};
use std::time::Duration;

pub const INSTRUCTIONS: &str = "TallyCheck: an offline accounting app running on this PC. \
Use these tools to answer questions from the company's books (invoices, bills, payments, reports, \
products and stock, bank transactions waiting to be matched). Amounts are in the company's currency \
unless a currency is given. If the key allows it, you can create DRAFT invoices, bills, customers \
and vendors — always show the user what you are about to create and ask first. Drafts must be \
checked and posted by a person in the app; you cannot post, pay, void or delete anything. Never \
invent figures: if a tool does not give a number, say so.";

pub struct Api {
    base: String,
    key: Option<String>,
    http: reqwest::Client,
}

impl Api {
    pub fn from_env() -> Api {
        let base = std::env::var("CENANGO_API_URL").unwrap_or_else(|_| "http://127.0.0.1:47800".into());
        let key = std::env::var("CENANGO_API_KEY").ok().map(|k| k.trim().to_string()).filter(|k| !k.is_empty());
        let http = reqwest::Client::builder().timeout(Duration::from_secs(60)).build().unwrap_or_default();
        Api { base: base.trim_end_matches('/').to_string(), key, http }
    }

    async fn send(&self, req: reqwest::RequestBuilder) -> Result<Value, String> {
        let key = self.key.as_deref().ok_or_else(|| {
            "No key: set CENANGO_API_KEY to a key made in TallyCheck under Settings → Integrations → Claude."
                .to_string()
        })?;
        let res =
            req.bearer_auth(key).send().await.map_err(|e| {
                format!("TallyCheck is not reachable at {} ({e}). Is the app open on this PC?", self.base)
            })?;
        let status = res.status().as_u16();
        let body: Value = res.json().await.unwrap_or(Value::Null);
        if (200..300).contains(&status) {
            return Ok(body);
        }
        let message = match &body["message"] {
            Value::String(s) => s.clone(),
            Value::Array(a) => a.iter().filter_map(Value::as_str).collect::<Vec<_>>().join("; "),
            _ => format!("answered {status}"),
        };
        Err(match status {
            401 => "The key was not accepted (wrong, or revoked). Make a new one in the app.".into(),
            403 => format!("Not allowed: {message}"),
            _ => message,
        })
    }

    async fn get(&self, path: &str, query: &[(&str, String)]) -> Result<Value, String> {
        let q: Vec<(&str, &String)> = query.iter().filter(|(_, v)| !v.is_empty()).map(|(k, v)| (*k, v)).collect();
        self.send(self.http.get(format!("{}/api{path}", self.base)).query(&q)).await
    }

    async fn post(&self, path: &str, body: &Value) -> Result<Value, String> {
        self.send(self.http.post(format!("{}/api{path}", self.base)).json(body)).await
    }
}

fn s(args: &Value, k: &str) -> String {
    match &args[k] {
        Value::String(v) => v.trim().to_string(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        _ => String::new(),
    }
}

fn limit(args: &Value) -> String {
    args["limit"].as_u64().map(|l| l.clamp(1, 100).to_string()).unwrap_or_else(|| "50".into())
}

fn obj(pairs: &[(&str, &Value)]) -> Value {
    let mut m = Map::new();
    for (k, v) in pairs {
        if !v.is_null() {
            m.insert((*k).to_string(), (*v).clone());
        }
    }
    Value::Object(m)
}

fn tool(name: &str, description: &str, properties: Value, required: &[&str]) -> Value {
    json!({
        "name": name,
        "description": description,
        "inputSchema": { "type": "object", "properties": properties, "required": required },
    })
}

fn date(desc: &str) -> Value {
    json!({ "type": "string", "description": format!("{desc} (YYYY-MM-DD)") })
}

fn read_tools() -> Vec<Value> {
    vec![
        tool("connection_status", "Which company this is connected to, as which user, and whether drafts may be created.", json!({}), &[]),
        tool(
            "company_overview",
            "Balances of every account today (cash, banks, receivables, payables…) and who owes and is owed, by age.",
            json!({}),
            &[],
        ),
        tool(
            "list_invoices",
            "Sales invoices, newest first. Filter by status, text (number, PO or customer) and dates.",
            json!({
                "status": { "type": "string", "enum": ["DRAFT", "POSTED", "PAID", "VOID"] },
                "search": { "type": "string" },
                "date_from": date("Invoices on or after"),
                "date_to": date("Invoices on or before"),
                "limit": { "type": "integer", "minimum": 1, "maximum": 100 }
            }),
            &[],
        ),
        tool("get_invoice", "One invoice in full (lines, tax, payments), by its number such as INV-9004 or a PO number.", json!({ "number": { "type": "string" } }), &["number"]),
        tool("list_bills", "Vendor bills, newest first.", json!({ "limit": { "type": "integer", "minimum": 1, "maximum": 100 } }), &[]),
        tool("list_payments", "Payments received and made, newest first.", json!({ "limit": { "type": "integer", "minimum": 1, "maximum": 100 } }), &[]),
        tool("list_expenses", "Expenses paid on the spot, newest first.", json!({ "limit": { "type": "integer", "minimum": 1, "maximum": 100 } }), &[]),
        tool("list_customers", "Customers, optionally only those whose name contains a text.", json!({ "search": { "type": "string" } }), &[]),
        tool("list_vendors", "Vendors (suppliers), optionally only those whose name contains a text.", json!({ "search": { "type": "string" } }), &[]),
        tool("list_accounts", "The chart of accounts: code, name, type and currency.", json!({}), &[]),
        tool(
            "run_report",
            "A financial report. profit_loss and cash_flow use date_from/date_to; balance_sheet, ar_aging and ap_aging use as_of; tax_return uses date_from/date_to; trial_balance is today.",
            json!({
                "report": { "type": "string", "enum": ["profit_loss", "balance_sheet", "cash_flow", "trial_balance", "ar_aging", "ap_aging", "tax_return"] },
                "date_from": date("Period start"),
                "date_to": date("Period end"),
                "as_of": date("Report date")
            }),
            &["report"],
        ),
        tool(
            "account_activity",
            "Every entry behind an account's balance for a period, with a running balance. Use the account code, e.g. 6000.",
            json!({ "account_code": { "type": "string" }, "date_from": date("From"), "date_to": date("To") }),
            &["account_code"],
        ),
        tool("list_products", "Products & services with prices, stock on hand, stock value and low-stock flags.", json!({ "low_stock_only": { "type": "boolean" } }), &[]),
        tool(
            "bank_lines_to_review",
            "Imported bank, PayPal, Stripe or Shopify lines not yet matched, with the app's suggested matches.",
            json!({ "account_code": { "type": "string", "description": "Only this account; all accounts with lines when empty" } }),
            &[],
        ),
    ]
}

fn line_schema(account_required: bool) -> Value {
    json!({
        "type": "array",
        "minItems": 1,
        "items": {
            "type": "object",
            "properties": {
                "description": { "type": "string" },
                "quantity": { "type": "number", "description": "Default 1" },
                "unit_price": { "type": "number", "description": "Price of one, at most 2 decimals" },
                "account_code": { "type": "string", "description": if account_required { "Expense account code, e.g. 6000 (not needed with a product)" } else { "Revenue account code; default 4000 or the product's" } },
                "tax_code": { "type": "string", "description": "Name of one of the company's tax codes, e.g. VAT 18%" },
                "product_sku": { "type": "string", "description": "SKU or name of a product in Products & Services" },
                "discount": { "type": "number", "description": "Amount off this line, before tax (invoices only)" }
            },
            "required": ["description", "unit_price"]
        }
    })
}

fn write_tools() -> Vec<Value> {
    vec![
        tool(
            "create_customer",
            "Add a customer. Ask the user before creating it.",
            json!({ "name": { "type": "string" }, "email": { "type": "string" }, "phone": { "type": "string" }, "address": { "type": "string" } }),
            &["name"],
        ),
        tool(
            "create_draft_invoice",
            "Create a DRAFT sales invoice (it is not posted; a person checks and posts it in the app). Show the user the lines and totals and ask before calling.",
            json!({
                "customer": { "type": "string", "description": "Customer name as in the app; anyone else is a one-off invoice" },
                "date": date("Invoice date; default today"),
                "due_date": date("Due date"),
                "po_number": { "type": "string" },
                "prices_include_tax": { "type": "boolean" },
                "notes": { "type": "string" },
                "lines": line_schema(false)
            }),
            &["customer", "lines"],
        ),
        tool(
            "create_draft_bill",
            "Create a DRAFT vendor bill (not posted). Show the user the lines and ask before calling.",
            json!({
                "vendor": { "type": "string", "description": "Vendor name as in the app" },
                "date": date("Bill date"),
                "due_date": date("Due date"),
                "prices_include_tax": { "type": "boolean" },
                "lines": line_schema(true)
            }),
            &["vendor", "date", "lines"],
        ),
    ]
}

/// The tools for this key: drafts only when the key allows them.
pub async fn list(api: &Api) -> Vec<Value> {
    let mut tools = read_tools();
    if api.get("/api-keys/me", &[]).await.is_ok_and(|me| me["canWrite"].as_bool() == Some(true)) {
        tools.extend(write_tools());
    }
    tools
}

/// Today on this PC, for an invoice without a date.
fn today() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

async fn find_by_name(api: &Api, path: &str, name: &str) -> Result<Option<Value>, String> {
    let all = api.get(path, &[]).await?;
    let list = all.as_array().cloned().unwrap_or_default();
    let lower = name.to_lowercase();
    if let Some(exact) = list.iter().find(|x| x["name"].as_str().is_some_and(|n| n.to_lowercase() == lower)) {
        return Ok(Some(exact.clone()));
    }
    let partial: Vec<&Value> =
        list.iter().filter(|x| x["name"].as_str().is_some_and(|n| n.to_lowercase().contains(&lower))).collect();
    Ok(if partial.len() == 1 { Some(partial[0].clone()) } else { None })
}

async fn account_id(api: &Api, code: &str) -> Result<String, String> {
    let accounts = api.get("/accounts", &[]).await?;
    accounts
        .as_array()
        .and_then(|a| a.iter().find(|x| x["code"].as_str() == Some(code)))
        .and_then(|x| x["id"].as_str())
        .map(str::to_string)
        .ok_or_else(|| format!("No account with code {code}. Use list_accounts to see the codes."))
}

/// The lines as the app takes them: codes, tax names and SKUs turned into ids.
async fn lines(api: &Api, args: &Value, invoice: bool) -> Result<Vec<Value>, String> {
    let given = args["lines"].as_array().cloned().unwrap_or_default();
    if given.is_empty() {
        return Err("Give at least one line".into());
    }
    let taxes = api.get("/tax-codes", &[]).await.unwrap_or(Value::Null);
    let items = api.get("/items", &[]).await.unwrap_or(Value::Null);
    let mut out = Vec::new();
    for l in given {
        let mut line = Map::new();
        line.insert("description".into(), json!(s(&l, "description")));
        if !l["quantity"].is_null() {
            line.insert("quantity".into(), l["quantity"].clone());
        }
        line.insert("unitPrice".into(), l["unit_price"].clone());
        let sku = s(&l, "product_sku");
        if !sku.is_empty() {
            let lower = sku.to_lowercase();
            let item = items.as_array().and_then(|a| {
                a.iter().find(|i| {
                    i["sku"].as_str().is_some_and(|x| x.to_lowercase() == lower)
                        || i["name"].as_str().is_some_and(|x| x.to_lowercase() == lower)
                })
            });
            let item = item.ok_or_else(|| format!("No product \"{sku}\". Use list_products to see them."))?;
            line.insert("itemId".into(), item["id"].clone());
            let acct = if invoice {
                &item["incomeAccountId"]
            } else if item["type"] == "INVENTORY" {
                &item["inventoryAccountId"]
            } else {
                &item["expenseAccountId"]
            };
            if !acct.is_null() {
                line.insert("accountId".into(), acct.clone());
            }
        }
        let code = s(&l, "account_code");
        if !code.is_empty() {
            line.insert("accountId".into(), json!(account_id(api, &code).await?));
        }
        if !invoice && !line.contains_key("accountId") {
            return Err(format!("Line \"{}\" needs an account_code (or a product)", s(&l, "description")));
        }
        let tax = s(&l, "tax_code");
        if !tax.is_empty() {
            let lower = tax.to_lowercase();
            let t = taxes
                .as_array()
                .and_then(|a| a.iter().find(|t| t["name"].as_str().is_some_and(|n| n.to_lowercase() == lower)))
                .ok_or_else(|| format!("No tax code named \"{tax}\""))?;
            line.insert("taxCodeId".into(), t["id"].clone());
        }
        if invoice && !l["discount"].is_null() {
            line.insert("discountAmount".into(), l["discount"].clone());
        }
        out.push(Value::Object(line));
    }
    Ok(out)
}

pub async fn call(api: &Api, name: &str, a: &Value) -> Result<Value, String> {
    match name {
        "connection_status" => {
            let me = api.get("/api-keys/me", &[]).await?;
            let settings = api.get("/settings", &[]).await.unwrap_or(Value::Null);
            Ok(json!({
                "company": settings["company.name"], "currency": settings["region.currency"], "country": settings["region.country"],
                "user": me["user"], "role": me["role"], "key": me["name"],
                "mayCreateDrafts": me["canWrite"], "today": today(),
            }))
        }
        "company_overview" => {
            let tb = api.get("/ledger/trial-balance", &[]).await?;
            let rows: Vec<Value> = tb
                .as_array()
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .map(|r| json!({ "code": r["account"]["code"], "name": r["account"]["name"], "type": r["account"]["type"], "balance": r["balance"] }))
                .collect();
            let ar = api.get("/reports/ar-aging", &[]).await.unwrap_or(Value::Null);
            let ap = api.get("/reports/ap-aging", &[]).await.unwrap_or(Value::Null);
            Ok(json!({ "accountBalances": rows, "receivablesByAge": ar, "payablesByAge": ap,
                       "note": "Balances are in each account's normal direction (assets and expenses as debits, income and liabilities as credits)." }))
        }
        "list_invoices" => {
            let r = api
                .get(
                    "/invoices",
                    &[
                        ("status", s(a, "status")),
                        ("q", s(a, "search")),
                        ("dateFrom", s(a, "date_from")),
                        ("dateTo", s(a, "date_to")),
                        ("limit", limit(a)),
                        ("page", "1".into()),
                    ],
                )
                .await?;
            let rows: Vec<Value> = r["data"]
                .as_array()
                .cloned()
                .unwrap_or_default()
                .iter()
                .map(|i| {
                    obj(&[
                        ("number", &i["invoiceNumber"]),
                        ("poNumber", &i["poNumber"]),
                        ("date", &i["date"]),
                        ("dueDate", &i["dueDate"]),
                        ("customer", &i["billToName"]),
                        ("status", &i["status"]),
                        ("total", &i["totalAmount"]),
                        ("paid", &i["amountPaid"]),
                        ("credited", &i["amountCredited"]),
                        ("currency", &i["currency"]),
                    ])
                })
                .collect();
            Ok(json!({ "total": r["total"], "invoices": rows }))
        }
        "get_invoice" => {
            let number = s(a, "number");
            let r =
                api.get("/invoices", &[("q", number.clone()), ("limit", "10".into()), ("page", "1".into())]).await?;
            let list = r["data"].as_array().cloned().unwrap_or_default();
            let hit = list
                .iter()
                .find(|i| i["invoiceNumber"].as_str().is_some_and(|n| n.eq_ignore_ascii_case(&number)))
                .or_else(|| list.first())
                .ok_or_else(|| format!("No invoice matches \"{number}\""))?;
            api.get(&format!("/invoices/{}", hit["id"].as_str().unwrap_or("")), &[]).await
        }
        "list_bills" => api.get("/bills", &[("limit", limit(a)), ("page", "1".into())]).await,
        "list_payments" => api.get("/payments", &[("limit", limit(a)), ("page", "1".into())]).await,
        "list_expenses" => api.get("/expenses", &[("limit", limit(a)), ("page", "1".into())]).await,
        "list_customers" | "list_vendors" => {
            let path = if name == "list_customers" { "/customers" } else { "/vendors" };
            let all = api.get(path, &[]).await?;
            let q = s(a, "search").to_lowercase();
            let rows: Vec<Value> = all
                .as_array()
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .filter(|p| q.is_empty() || p["name"].as_str().is_some_and(|n| n.to_lowercase().contains(&q)))
                .map(|p| {
                    obj(&[
                        ("name", &p["name"]),
                        ("email", &p["email"]),
                        ("phone", &p["phone"]),
                        ("active", &p["isActive"]),
                    ])
                })
                .collect();
            Ok(Value::Array(rows))
        }
        "list_accounts" => {
            let all = api.get("/accounts", &[]).await?;
            Ok(Value::Array(
                all.as_array()
                    .cloned()
                    .unwrap_or_default()
                    .iter()
                    .map(|x| {
                        obj(&[
                            ("code", &x["code"]),
                            ("name", &x["name"]),
                            ("type", &x["type"]),
                            ("currency", &x["currency"]),
                            ("active", &x["isActive"]),
                        ])
                    })
                    .collect(),
            ))
        }
        "run_report" => {
            let (from, to, as_of) = (s(a, "date_from"), s(a, "date_to"), s(a, "as_of"));
            match s(a, "report").as_str() {
                "profit_loss" => api.get("/reports/profit-loss", &[("dateFrom", from), ("dateTo", to)]).await,
                "cash_flow" => api.get("/reports/cash-flow", &[("dateFrom", from), ("dateTo", to)]).await,
                "tax_return" => api.get("/reports/tax", &[("dateFrom", from), ("dateTo", to)]).await,
                "balance_sheet" => api.get("/reports/balance-sheet", &[("asOf", as_of)]).await,
                "ar_aging" => api.get("/reports/ar-aging", &[("asOf", as_of)]).await,
                "ap_aging" => api.get("/reports/ap-aging", &[("asOf", as_of)]).await,
                "trial_balance" => api.get("/ledger/trial-balance", &[]).await,
                other => Err(format!("Unknown report \"{other}\"")),
            }
        }
        "account_activity" => {
            let id = account_id(api, &s(a, "account_code")).await?;
            api.get(&format!("/ledger/activity/{id}"), &[("dateFrom", s(a, "date_from")), ("dateTo", s(a, "date_to"))])
                .await
        }
        "list_products" => {
            let all = api.get("/items", &[]).await?;
            let low_only = a["low_stock_only"].as_bool() == Some(true);
            Ok(Value::Array(
                all.as_array()
                    .cloned()
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|i| i["isActive"] == true && (!low_only || i["lowStock"] == true))
                    .map(|i| {
                        obj(&[
                            ("name", &i["name"]),
                            ("sku", &i["sku"]),
                            ("type", &i["type"]),
                            ("salePrice", &i["salePrice"]),
                            ("onHand", &i["quantityOnHand"]),
                            ("stockValue", &i["stockValue"]),
                            ("averageCost", &i["averageCost"]),
                            ("reorderPoint", &i["reorderPoint"]),
                            ("lowStock", &i["lowStock"]),
                        ])
                    })
                    .collect(),
            ))
        }
        "bank_lines_to_review" => {
            let code = s(a, "account_code");
            let accounts = api.get("/accounts", &[]).await?;
            let by_id = |id: &str| {
                accounts.as_array().and_then(|x| x.iter().find(|y| y["id"] == id)).cloned().unwrap_or(Value::Null)
            };
            let ids: Vec<String> = if code.is_empty() {
                api.get("/bank-lines/accounts", &[])
                    .await?
                    .as_array()
                    .cloned()
                    .unwrap_or_default()
                    .iter()
                    .filter(|x| x["unmatched"].as_i64().unwrap_or(0) > 0)
                    .filter_map(|x| x["accountId"].as_str().map(str::to_string))
                    .collect()
            } else {
                vec![account_id(api, &code).await?]
            };
            let mut out = Vec::new();
            for id in ids {
                let r = api.get("/bank-lines", &[("accountId", id.clone()), ("status", "UNMATCHED".into())]).await?;
                let acct = by_id(&id);
                out.push(json!({ "account": format!("{} {}", s(&acct, "code"), s(&acct, "name")), "currency": r["currency"], "lines": r["data"] }));
            }
            Ok(Value::Array(out))
        }
        "create_customer" => {
            let body = obj(&[
                ("name", &json!(s(a, "name"))),
                ("email", &a["email"]),
                ("phone", &a["phone"]),
                ("address", &a["address"]),
            ]);
            let c = api.post("/customers", &body).await?;
            Ok(json!({ "created": c["name"], "id": c["id"] }))
        }
        "create_draft_invoice" => {
            let who = s(a, "customer");
            let mut body = Map::new();
            match find_by_name(api, "/customers", &who).await? {
                Some(c) => {
                    body.insert("customerId".into(), c["id"].clone());
                    body.insert("billToName".into(), c["name"].clone());
                    if !c["address"].is_null() {
                        body.insert("billToAddress".into(), c["address"].clone());
                    }
                }
                None => {
                    body.insert("billToName".into(), json!(who));
                }
            }
            let day = s(a, "date");
            body.insert("date".into(), json!(if day.is_empty() { today() } else { day }));
            for (from, to) in [("due_date", "dueDate"), ("po_number", "poNumber"), ("notes", "notes")] {
                if !s(a, from).is_empty() {
                    body.insert(to.into(), json!(s(a, from)));
                }
            }
            if a["prices_include_tax"].as_bool() == Some(true) {
                body.insert("pricesIncludeTax".into(), json!(true));
            }
            body.insert("lines".into(), Value::Array(lines(api, a, true).await?));
            let inv = api.post("/invoices", &Value::Object(body)).await?;
            Ok(json!({ "draft": inv["invoiceNumber"], "status": inv["status"], "total": inv["totalAmount"],
                       "next": "Ask the user to check and post it on the Invoices screen." }))
        }
        "create_draft_bill" => {
            let v = s(a, "vendor");
            let vendor = find_by_name(api, "/vendors", &v)
                .await?
                .ok_or_else(|| format!("No vendor named \"{v}\". Add it in the app first."))?;
            let mut body = Map::new();
            body.insert("vendorId".into(), vendor["id"].clone());
            body.insert("date".into(), json!(s(a, "date")));
            if !s(a, "due_date").is_empty() {
                body.insert("dueDate".into(), json!(s(a, "due_date")));
            }
            if a["prices_include_tax"].as_bool() == Some(true) {
                body.insert("pricesIncludeTax".into(), json!(true));
            }
            body.insert("lines".into(), Value::Array(lines(api, a, false).await?));
            let bill = api.post("/bills", &Value::Object(body)).await?;
            Ok(json!({ "draft": bill["billNumber"], "status": bill["status"], "total": bill["totalAmount"],
                       "next": "Ask the user to check and post it on the Bills screen." }))
        }
        other => Err(format!("Unknown tool \"{other}\"")),
    }
}
