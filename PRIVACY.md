# TallyCheck privacy policy

Last updated: 14 November 2026

TallyCheck Desktop is an offline accounting, invoicing and payroll application for Windows and
macOS, published by Cenango (Pvt) Ltd ("we"). This policy explains what happens to your
information when you use the desktop app. The website and the hosted TallyCheck Web edition have
their own policy at https://tallycheck.io/privacy.

## Your data stays on your computer

* Everything you enter — your company details, customers, vendors, invoices, bills, payments,
  employees, salaries, bank details and documents — is stored **only on the computer where
  TallyCheck is installed**:

  | Version | Data folder |
  |---|---|
  | Windows, installer from the website or GitHub | `%LOCALAPPDATA%\TallyCheckData` (or `%LOCALAPPDATA%\CenangoAccounting` on PCs set up with an earlier version) |
  | Windows, Microsoft Store | `%USERPROFILE%\TallyCheckData` |
  | macOS | `~/Library/Application Support/TallyCheckData` |

* Employee documents, connection keys and backups are stored encrypted. Passwords are stored only
  as secure hashes. Employee national ID numbers, when you enter them for Sri Lankan payroll
  forms, are kept in the database; US Social Security numbers are not stored.
* **We do not receive, collect or see any of your data.** TallyCheck Desktop has no accounts with
  us, no analytics, no telemetry and no advertising. We do not sell or share personal information.
* Uninstalling TallyCheck leaves your data folder in place on purpose, so that your books are not
  lost. You can delete the folder yourself.
* We recommend turning on disk encryption (BitLocker on Windows, FileVault on macOS), because the
  data folder holds your books and payroll.

## When TallyCheck connects to the internet

TallyCheck works without the internet. It goes online only in these cases:

| What | When | What is sent |
|---|---|---|
| Update check (installer version only) | When an Administrator opens the app | A request to GitHub (`github.com/Sammysam/tallycheck`) for the newest version number. GitHub sees your IP address, as for any web request. No accounting data is sent. The Microsoft Store version is updated by the Microsoft Store instead. |
| Shopify, WooCommerce, PayPal, Stripe | Only after you connect them in Settings → Integrations, and only when you sync | The requests those services need to send your orders and payments to TallyCheck. Their own privacy policies apply. |
| Claude connection | Only if you create a key and set it up yourself | The questions you ask and the accounting data needed to answer them are sent to Anthropic. Anthropic's terms and privacy policy apply; if your books contain other people's personal data, you are responsible for having a suitable agreement with Anthropic. |

The keys you give for these services are stored encrypted on your computer.

## Other computers in your office

LAN mode, which lets other PCs in your office use TallyCheck through a browser, is off until an
administrator turns it on. When it is on, the data stays on the main PC and the other PCs view it
over your office network. This traffic is not encrypted, so use LAN mode only on a network you
trust.

## Backups

Backups are created only when you make them (or schedule them), and are saved where you choose.
They are encrypted with a password you set.

## Your responsibilities and rights (GDPR, UK GDPR, California)

* **For the people in your books** (your customers, vendors and employees), your business is the
  data controller (GDPR) or the business (CCPA). TallyCheck helps you answer their requests: you
  can view, correct and export their records (CSV export, customer statements) and delete records
  that have no transactions. Accounting and payroll laws usually require you to keep posted
  records for several years.
* **For us:** because TallyCheck Desktop sends us no personal data, we hold none about you from the
  app. If you write to us, we use your message only to answer it. You can ask what we hold about
  you, and ask us to correct or delete it, at the address below. If you are in the EU or UK you may
  also complain to your data protection authority; California residents will not be treated
  differently for using their rights.

A checklist of what TallyCheck does, and what is still planned, for GDPR and California privacy
law is in [docs/privacy-compliance.md](docs/privacy-compliance.md).

## Children

TallyCheck is business software and is not directed at children.

## Changes and contact

If this policy changes, the new version will be published at
https://github.com/Sammysam/tallycheck/blob/main/PRIVACY.md with a new date.

Privacy questions and requests: **hello@tallycheck.io** (please do not post personal information
in public GitHub issues).
