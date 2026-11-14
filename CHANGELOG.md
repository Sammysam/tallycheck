# Changelog

All notable changes to TallyCheck. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and versions follow
[Semantic Versioning](https://semver.org/). Numbers such as D49 refer to the decisions in
[docs/design.md](docs/design.md).

## [Unreleased]

### Added
- macOS app: one universal build for Apple silicon and Intel, macOS 11 or later (D53).
- Forgotten passwords: recovery keys, and on Windows a reset with Windows Hello or the Windows
  password (D49).
- The Claude connector is also installed as `tallycheck-mcp` and reads `TALLYCHECK_API_KEY` (D51).
- Continuous integration on GitHub Actions (Windows and macOS) and Bitbucket Pipelines; the
  Microsoft Store package and the Mac app can be built on GitHub.

### Security
- Full security review and fixes: sign-in throttling, "this PC" checks, Content-Security-Policy,
  safer imports, connector and backup hardening (D50).
- Users who are not administrators can only give rights they hold themselves; sign-ins are
  ended after password or 2FA resets; 2FA codes work only once; failed sign-ins take a fixed
  minimum time (D52).

- Forgotten usernames: "Forgot username?" on the sign-in screen shows the username of a recovery
  key, or on the main PC lists the usernames after Windows sign-in; no screen reveals whether a
  username exists (D56).

### Changed
- Microsoft Store version: the company's books are kept in `%USERPROFILE%\TallyCheckData`, so
  they stay when the app is updated or uninstalled, without the restricted capability the Store
  refused (D55). The direct installer is unchanged.

### Fixed
- Restoring a backup on a Mac left the file encryption key unreadable; it is now sealed with
  the data folder's Keychain key.

## [1.0.0] - 2026-11-02

First release: offline accounting for Windows with bookkeeping, sales and purchases, reports,
taxes for any country, foreign currencies, Sri Lankan and US payroll, products and stock,
bank reconciliation, imports, Shopify / WooCommerce / PayPal / Stripe connectors, the Claude
(MCP) connector, encrypted backups, LAN mode, in-app updates and the Microsoft Store package.

[Unreleased]: https://github.com/Sammysam/tallycheck/compare/v1.0.0...HEAD
[1.0.0]: https://github.com/Sammysam/tallycheck/releases/tag/v1.0.0
