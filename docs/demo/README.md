# Demo company: Lotus Spice & Tea Traders (Pvt) Ltd

A complete, **made-up** business for showing the app: a Colombo wholesaler of Ceylon tea and spices, April – October 2026 (financial year from 1 April). No real company's or person's data is used.

Everything was typed in **through the app's own screens** by `tests/ui/demo-data.mjs`, the same forms a person uses. It is kept in its own data folder, `%LOCALAPPDATA%\CenangoAccounting-Demo`, so your real data is never touched.

## Open it

Double-click `scripts\run-demo.cmd`. It starts the app on the demo folder.

| User | Password | Role |
|---|---|---|
| `admin` | `LotusDemo2026` | Admin (everything) |
| `dilini.accounts` | `LotusAccounts26` | Accountant |
| `owner.view` | `LotusViewer26` | Viewer (dashboard, ledger, reports) |

These passwords are for the demo copy only.

## What is in it

| Screen | Data |
|---|---|
| Company setup | Name, address, email, phone, website, VAT number, logo, bank details for customers, invoice footer; Sri Lanka, LKR, year from April |
| Users | Admin, an accountant and a read-only owner |
| Accounts | Default chart plus an LKR current account, a **USD account**, **Telephone Payable**, fixed assets (equipment, delivery van), share capital, delivery income and 8 expense accounts |
| Taxes | VAT 18% (default on sales) and WHT 5% withholding |
| Ledger | Opening balances (1 Apr 2026), the telephone bill accrued each month end (Dr Telephone Expense / Cr Telephone Payable), two USD-to-LKR conversions and two quarterly VAT payments, all on the journal-entry form. Click any entry to open the transaction in full |
| Customers / Vendors | 8 customers (one in the USA, billed in USD) and 6 suppliers |
| Invoices | 18 invoices with customer PO numbers: paid, part-paid, overdue and current; 2 in USD |
| Payments | Customer receipts (one with 5% tax withheld; the US customer pays into the USD account), supplier payments, and monthly **payable-account payments** that clear Telephone Payable |
| Bills / Expenses | 11 supplier bills (tea, spices, packaging, freight) and 27 expenses (rent, power, fuel, stationery, ads, repairs…) |
| Credit / Debit notes | A delivery-damage discount to a café; misprinted pouches returned to the packaging supplier |
| Employees | 5 staff with salary, allowances, bank details; one staff loan with monthly instalments |
| Payroll | April – September posted and paid; EPF / ETF remitted to August; October left as a draft |
| USD account | Export receipts in USD, conversions to rupees, and revaluations at 30 June and 30 September (Chart of Accounts → Revalue); its activity shows the USD balance |
| Bank reconciliation | April – September reconciled to the bank statement |
| Lock the books | Locked up to 30 June 2026 (first quarter closed) |

## Re-create it from scratch

```
rem 1. empty demo folder, server on it
rmdir /s /q "%LOCALAPPDATA%\CenangoAccounting-Demo"
set ACCOUNTING_DATA_DIR=%LOCALAPPDATA%\CenangoAccounting-Demo
set ACCOUNTING_PORT=47811
target\debug\accounting-server.exe serve
rem 2. in another window
node tests\ui\demo-data.mjs http://127.0.0.1:47811 .scratch\demo-shots
```

It takes about four minutes and ends with a check that the trial balance balances.
