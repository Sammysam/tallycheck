# Cenango Accounting for Claude

Lets Claude answer questions from the books of Cenango Accounting running on this PC — and, if you
allow it, create **draft** invoices, bills, customers and vendors for a person to check and post.

What Claude can never do with it: post, pay, void or delete anything; open payroll, employees,
users, settings, integrations or the activity log; use the key from another PC. Everything it does
appears in the app's activity log as "<user> via <key name>".

**Privacy:** what Claude reads to answer a question is sent to Claude (Anthropic). The rest of the
app stays offline. Revoke the key in the app to stop it at once.

## 1. Make a key in the app

Settings → Integrations → **Claude (AI assistant)** → **+ New key**. Leave "Allow drafts" off for
questions only. Copy the key (`cak_…`) — it is shown once. The window also shows ready-made setup
for Claude Desktop and Claude Code with the right paths.

The connector program, `cenango-mcp.exe`, is installed next to the app
(usually `%LOCALAPPDATA%\Cenango Accounting\cenango-mcp.exe`). The app must be open while Claude
uses it.

## 2a. Claude Desktop

Settings → Developer → Edit Config, and add (with your key and path):

```json
{
  "mcpServers": {
    "cenango-accounting": {
      "command": "C:\\Users\\<you>\\AppData\\Local\\Cenango Accounting\\cenango-mcp.exe",
      "env": { "CENANGO_API_KEY": "cak_…", "CENANGO_API_URL": "http://127.0.0.1:47800" }
    }
  }
}
```

Restart Claude Desktop.

## 2b. Claude Code — with the skills (this plugin)

Set two user environment variables (Windows: "Edit environment variables for your account"):

* `CENANGO_MCP_PATH` = the full path of `cenango-mcp.exe`
* `CENANGO_API_KEY` = the key

Then in Claude Code:

```
/plugin marketplace add C:\path\to\claude-plugin
/plugin install cenango-accounting@cenango
```

Or without the plugin, only the connector:

```
claude mcp add cenango-accounting -e CENANGO_API_KEY=cak_… -- "C:\…\cenango-mcp.exe"
```

## What is included

| | |
|---|---|
| Tools | connection_status, company_overview, list_invoices, get_invoice, list_bills, list_payments, list_expenses, list_customers, list_vendors, list_accounts, run_report, account_activity, list_products, bank_lines_to_review; with "Allow drafts": create_customer, create_draft_invoice, create_draft_bill |
| Skills | month-end-close, chase-overdue-customers, prepare-vat-return, explain-a-report, review-online-sales |

Try: *"Who owes us money more than 60 days?"*, *"Why was September's profit lower than August's?"*,
*"Close October for me"*, *"Draft an invoice for ABC Traders: 2 days consulting at 15,000"*.
