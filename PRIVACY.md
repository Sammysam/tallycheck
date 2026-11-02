# TallyCheck privacy policy

Last updated: 1 November 2026

TallyCheck is an offline accounting, invoicing and payroll application for Windows, published by
Cenango (Pvt) Ltd. This policy explains what happens to your information.

## Your data stays on your PC

* Everything you enter — your company details, customers, vendors, invoices, bills, payments,
  employees, salaries, bank details and documents — is stored **only on the PC where TallyCheck
  is installed**, in `%LOCALAPPDATA%\TallyCheckData` (or `%LOCALAPPDATA%\CenangoAccounting` on PCs
  set up with an earlier version).
* Employee documents are stored encrypted. Passwords are stored only as secure hashes.
* **Cenango (Pvt) Ltd does not receive, collect or see any of your data.** TallyCheck has no
  accounts with us, no analytics and no advertising.
* Uninstalling TallyCheck leaves your data folder in place on purpose, so that your books are not
  lost. You can delete the folder yourself.

## When TallyCheck connects to the internet

TallyCheck works without the internet. It goes online only in these cases:

| What | When | What is sent |
|---|---|---|
| Update check (direct-download version only) | When an Administrator opens the app | A request to `github.com/Sammysam/tallycheck` for the newest version number. No accounting data. The Microsoft Store version is updated by the Microsoft Store instead. |
| Shopify, WooCommerce, PayPal, Stripe | Only after you connect them in Settings → Integrations, and only when you sync | The requests those services need to send your orders and payments to TallyCheck. Their own privacy policies apply. |
| Claude connection | Only if you create a key and set it up yourself | The questions you ask and the accounting data needed to answer them are sent to Anthropic. Anthropic's privacy policy applies. |

The keys you give for these services are stored encrypted on your PC.

## Other PCs in your office

LAN mode, which lets other PCs in your office use TallyCheck through a browser, is off until an
administrator turns it on. When it is on, the data still stays on the main PC; the other PCs view
it over your office network.

## Backups

Backups are created only when you make them (or schedule them), and are saved where you choose.
They are encrypted with a password you set.

## Children

TallyCheck is business software and is not directed at children.

## Changes and contact

If this policy changes, the new version will be published at
https://github.com/Sammysam/tallycheck/blob/main/PRIVACY.md with a new date.

Questions: open an issue at https://github.com/Sammysam/tallycheck/issues.
