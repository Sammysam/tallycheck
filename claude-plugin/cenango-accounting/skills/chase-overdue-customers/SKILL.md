---
name: chase-overdue-customers
description: Find customers with overdue invoices in TallyCheck and draft polite payment reminders. Use when the user asks who owes money, who is late, or to chase or remind customers.
---

# Chase overdue customers

1. Call `run_report` with `ar_aging` (today unless the user gives a date). Keep customers with
   anything in the 1–30, 31–60, 61–90 or 90+ columns.
2. For each, call `list_invoices` with the customer's name as `search` and status `POSTED` to get
   the open invoice numbers, dates, due dates and amounts still owed (total − paid − credited).
3. Show a table: customer, invoices, amount overdue, oldest due date, days late — worst first.
4. Draft a reminder for each customer the user picks, in the user's language and tone:
   - friendly for under 30 days late, firmer for 60+;
   - list each invoice number, its date and the amount due;
   - say how to pay if the user tells you (the app prints payment details on invoices);
   - sign with the user's name or company name (from `connection_status`).
5. The reminders are text for the user to send. You do not send email, and you do not change
   anything in the app.

Amounts are in the company's currency unless the invoice shows another currency. Do not invent
contact details: use only what `list_customers` returns.
