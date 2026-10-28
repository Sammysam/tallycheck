---
name: month-end-close
description: Walk through closing a month in TallyCheck — unmatched bank lines, draft documents, overdue items, foreign-currency revaluation, the reports to review, and locking the month. Use when the user asks to close, finish or review a month.
---

# Month-end close

Work through these steps for the month the user names (ask if unclear). Report findings as a short
checklist with ✅ / ⚠️ per step. You can only read and, if allowed, create drafts — tell the user
what to do in the app for anything else.

1. **Connection** — call `connection_status` and say which company and user you are working as.
2. **Bank and payment lines** — call `bank_lines_to_review`. List lines dated in or before the
   month that are still unmatched, per account, with the suggested match if any. The user matches
   them on Accounting → Bank Transactions.
3. **Drafts** — call `list_invoices` with status `DRAFT`, and `list_bills`. Drafts dated in the
   month are not in the books yet; list them so the user can post or delete them.
4. **Overdue** — call `run_report` with `ar_aging` and `ap_aging` as of the month's last day.
   Name customers more than 30 days overdue and bills overdue.
5. **Stock** — call `list_products` with `low_stock_only: true`; mention items to reorder.
6. **Foreign currency** — call `list_accounts`; for any account with a currency other than the
   company's, remind the user to use **Revalue** on the Chart of Accounts at the month-end rate.
7. **Reports** — call `run_report` for `profit_loss` (the month, and the same month last year if
   useful) and `balance_sheet` as of the last day. Summarise revenue, expenses, profit and cash in
   three or four lines. Point out large or unusual movements, and use `account_activity` to explain
   one if the user asks.
8. **Lock** — when everything above is clean, tell the user to lock the month in
   Settings → Closing. Never claim it is locked; you cannot do it.

Do not invent figures. If a tool fails, say which step could not be checked.
