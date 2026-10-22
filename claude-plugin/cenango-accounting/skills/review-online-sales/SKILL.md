---
name: review-online-sales
description: Review sales brought in from Shopify or WooCommerce and payments from Shopify Payments, Stripe or PayPal in Cenango Accounting — draft invoices to post, payments to match, fees and payouts. Use when the user asks about online orders, the shop, Shopify, WooCommerce, Stripe or PayPal.
---

# Review online sales

The app brings shop orders in as **draft invoices** (PO number = the shop's order number, e.g.
#1001) and payment-service activity as **lines on a money account** (1090 Shopify Clearing,
1091 PayPal, 1092 Stripe). The user syncs them under Settings → Integrations.

1. Call `list_invoices` with status `DRAFT`; the online orders are those whose PO number starts
   with `#`. Show them: order, customer, date, total. Point out any that look odd (zero total,
   missing customer).
2. Call `bank_lines_to_review`. For each money account:
   - payments: say which have a suggested invoice to pay (the invoice must be posted first);
   - fees: they can be recorded in one click (the line's hint);
   - payouts: one click records the transfer to the bank;
   - refunds: remind the user that a credit note is made in the app for the refunded order.
3. Give the user a short to-do list in this order: post the checked draft invoices → match the
   payments → record fees and payouts → credit notes for refunds.
4. If the user asks for a summary of the period, use `run_report` with `profit_loss` and the
   money account's `account_activity`.

You cannot post, match or pay — the user does that in the app. Never invent orders or amounts.
