# Changelog

All notable changes to TallyCheck. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and versions follow
[Semantic Versioning](https://semver.org/). Numbers such as D49 refer to the decisions in
[docs/design.md](docs/design.md).

## [Unreleased]

### Added
- Move books in from another system: an administrator brings the books over from QuickBooks,
  Xero, Zoho, Sage, TallyPrime or a spreadsheet on a cut-over date. The trial balance becomes the
  opening balances, unpaid invoices and bills come in one by one so they can be paid, and
  everything is checked to add up before anything is saved. A conversion can be reversed (D59).
- Quick start for moving books in: no files needed. Type the bank balances, who owes you, whom
  you owe, other assets and loans, and TallyCheck sets up the opening balances. A list of what
  each customer or supplier owes (such as an ageing summary) works instead of an invoice list.

### Fixed
- Import from File reads CSV reports that start with title lines (the delimiter was guessed
  from the first line only).

### Changed
- Only Sri Lanka and the United States can be chosen as the company's country for now (their
  payroll rules are built; more countries will follow). Companies that already use another
  country keep it. Payroll follows the country: Sri Lanka uses EPF / ETF / PAYE, the United
  States US payroll (D57).

## [1.1.0] - 2026-11-18

### Added
- macOS app: one universal build for Apple silicon and Intel, macOS 11 or later (D53).
- Forgotten passwords: recovery keys, and on Windows a reset with Windows Hello or the Windows
  password (D49).
- The Claude connector is also installed as `tallycheck-mcp` and reads `TALLYCHECK_API_KEY` (D51).
- Continuous integration on GitHub Actions (Windows and macOS) and Bitbucket Pipelines; the
  Microsoft Store package and the Mac app can be built on GitHub.
- Forgotten usernames: "Forgot username?" on the sign-in screen shows the username of a recovery
  key, or on the main PC lists the usernames after Windows sign-in; no screen reveals whether a
  username exists (D56).

### Security
- Full security review and fixes: sign-in throttling, "this PC" checks, Content-Security-Policy,
  safer imports, connector and backup hardening (D50).
- Users who are not administrators can only give rights they hold themselves; sign-ins are
  ended after password or 2FA resets; 2FA codes work only once; failed sign-ins take a fixed
  minimum time (D52).
- SQLite 3.54 (sqlx 0.9.1, libsqlite3-sys 0.40): fixes RUSTSEC-2026-0459, where a crafted
  database and `-wal` file pair could make SQLite return leftover memory. Books made by 1.0.0
  open unchanged.
- Dependency updates are proposed only after a version has been public for 7 days.

### Changed
- Microsoft Store version: the company's books are kept in `%USERPROFILE%\TallyCheckData`, so
  they stay when the app is updated or uninstalled, without the restricted capability the Store
  refused (D55). The direct installer is unchanged.
- Import from file: the column boxes no longer cover the file preview, and examples show real
  values; imported sales invoices keep their number from the file as their reference; bank
  lines quoting an invoice number suggest that invoice, and a name + amount match is listed
  first.

### Fixed
- Restoring a backup on a Mac left the file encryption key unreadable; it is now sealed with
  the data folder's Keychain key.

## [1.0.0] - 2026-11-02

First release: offline accounting for Windows with bookkeeping, sales and purchases, reports,
taxes for any country, foreign currencies, Sri Lankan and US payroll, products and stock,
bank reconciliation, imports, Shopify / WooCommerce / PayPal / Stripe connectors, the Claude
(MCP) connector, encrypted backups, LAN mode, in-app updates and the Microsoft Store package.

[Unreleased]: https://github.com/Sammysam/tallycheck/compare/v1.1.0...HEAD
[1.1.0]: https://github.com/Sammysam/tallycheck/compare/v1.0.0...v1.1.0
[1.0.0]: https://github.com/Sammysam/tallycheck/releases/tag/v1.0.0
