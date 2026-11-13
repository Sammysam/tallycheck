# TallyCheck

Offline accounting for small and medium businesses, as a desktop app for **Windows** and
**macOS**. Everything runs on the user's own computer: a Tauri 2 window around an embedded Rust
server (Axum + SQLite) that serves the React frontend. No cloud account, no database server and
no internet connection are needed; the optional online connectors are used only when the user
syncs them.

[![CI](https://github.com/Sammysam/tallycheck/actions/workflows/ci.yml/badge.svg)](https://github.com/Sammysam/tallycheck/actions/workflows/ci.yml)

## Features

- **Bookkeeping:** chart of accounts, journal entries and reversals, books lock date, bank
  reconciliation, bank statement import and matching.
- **Sales and purchases:** customers, vendors, invoices, bills, expenses, payments, credit and
  debit notes, customer statements, products and stock at average cost.
- **Reports:** profit and loss, balance sheet, cash flow, trial balance, AR/AP ageing, tax
  return, drill-down to every transaction, CSV export.
- **Any country:** company and region settings, user-defined tax codes, tax groups and
  withholding tax, foreign-currency documents and bank accounts.
- **Payroll:** Sri Lankan payroll (EPF, ETF, PAYE) and US payroll (federal and state income tax,
  FICA, FUTA/SUTA, Form 941/940/W-2 figures).
- **Users and security:** roles and permissions, optional two-factor sign-in, recovery keys,
  activity log, encrypted documents, encrypted backups, office LAN mode.
- **Integrations:** import from CSV/Excel; Shopify, WooCommerce, PayPal and Stripe connectors; a
  Claude (MCP) connector with revocable read-only or draft-only keys.
- **Updates:** signed in-app updates (Windows installer) or through the Microsoft Store.

## Download

- **Windows:** the installer from [Releases](https://github.com/Sammysam/tallycheck/releases), or
  the Microsoft Store. Install guide: [docs/INSTALL.md](docs/INSTALL.md).
- **macOS 11 or later** (Apple silicon and Intel): see [docs/macos.md](docs/macos.md).

## Repository layout

```
crates/app-core      config, errors, money (integer cents), time, ids, secret storage
crates/database      SQLite pools, pragmas, migrations, settings
crates/auth          passwords, JWT, TOTP, sign-in throttle, users, roles, recovery keys
crates/audit         activity log
crates/ledger        accounts, journal posting and reversal, balances, reconciliation
crates/operations    customers, vendors, invoices, bills, expenses, payments, notes, stock, imports
crates/reports       profit and loss, balance sheet, cash flow, ageing, tax return
crates/documents     AES-256-GCM file encryption and the file store
crates/employees     employees, documents, salaries
crates/payroll       Sri Lankan and US payroll
crates/backup        encrypted backups, verification, staged restore, schedule
crates/importer      one-time import from the PostgreSQL web edition
crates/integrations  Shopify, WooCommerce, PayPal and Stripe connectors
crates/mcp           the Claude (MCP) connector, tallycheck-mcp
crates/api-server    HTTP routes, validation, middleware, accounting-server binary
src-tauri            desktop shell, installer, macOS bundle and MSIX settings
frontend             React frontend (Vite)
migrations/sqlite    schema migrations (append-only)
tests/contract       side-by-side API comparison with the web edition
tests/ui             end-to-end smoke test of the real UI
tools                build, release and fixture scripts
docs                 design notes and guides
claude-plugin        Claude Code plugin (MCP server and skills)
```

## Build and run

Prerequisites: Rust 1.99 (pinned in `rust-toolchain.toml`), Node 24, Python 3.9+. On Windows,
WebView2 (built into Windows 11); on macOS, the Xcode command line tools.

```
cd frontend && npm ci && npm run build && cd ..
cargo run -p accounting-desktop                    # desktop app
cargo run -p api-server --bin accounting-server    # headless server, open the printed URL
```

On a fresh install the sign-in page opens a first-run screen to create the administrator.

Release builds:

```
npm ci && npm run installer                        # Windows installer (frontend built first)
python tools/build_msix.py --identity-name … --publisher "CN=…" --publisher-display-name …
bash tools/build_macos.sh                          # macOS universal .dmg, on a Mac
```

## Tests

```
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
node tests/ui/smoke.mjs http://127.0.0.1:47800     # needs a running server with a fresh database
bash tests/contract/run-all.sh                     # needs Docker and the web edition's backend
```

`powershell -ExecutionPolicy Bypass -File tools\ci.ps1` runs the checks in one go. GitHub
Actions runs them on Windows and macOS for every push ([docs/ci-cd.md](docs/ci-cd.md)).

## Configuration

Data lives in a per-user folder, never in the program folder:

- Windows: `%LOCALAPPDATA%\TallyCheckData`
- macOS: `~/Library/Application Support/TallyCheckData`

`ACCOUNTING_DATA_DIR` overrides it. The folder holds `accounting.db` (+ WAL files), `secrets/`
(keys protected by Windows DPAPI or the macOS Keychain), `files/`, `logs/` and an optional
`config.toml`:

```toml
port = 47800
bind_address = "127.0.0.1"   # anything else requires lan_enabled = true
lan_enabled = false
```

Environment overrides: `ACCOUNTING_PORT`, `ACCOUNTING_BIND`, `ACCOUNTING_LAN`,
`ACCOUNTING_FRONTEND_DIR`, `RUST_LOG`. Logs record method, URL, status and latency only, never
bodies, passwords, tokens or keys.

## Documentation

- [Design notes and decisions](docs/design.md): architecture, accounting rules, security and
  every numbered decision (D1, D2, …) referred to in code comments
- [Backup and recovery](docs/backup-and-recovery.md) · [LAN mode](docs/lan-mode.md) ·
  [Importer](docs/importer.md) · [Schema differences](docs/schema-differences.md)
- [Installer and updates](docs/installer-and-updates.md) · [Microsoft Store](docs/microsoft-store.md) ·
  [macOS](docs/macos.md) · [Code signing](docs/signpath.md) · [CI/CD](docs/ci-cd.md)
- [User guide (PDF)](docs/user-guide/TallyCheck-User-Guide.pdf)

## Contributing and security

See [CONTRIBUTING.md](CONTRIBUTING.md). Please report security problems privately as described in
[SECURITY.md](SECURITY.md), not in public issues. Changes are listed in [CHANGELOG.md](CHANGELOG.md).

## License

Copyright (C) 2026 Cenango (Pvt) Ltd.

Licensed under either of [MIT](LICENSE-MIT) or [Apache License 2.0](LICENSE-APACHE)
(`MIT OR Apache-2.0`), at your option. The TallyCheck and Cenango names and logos are not
licensed; see [NOTICE](NOTICE). Privacy policy: [PRIVACY.md](PRIVACY.md). Code signing policy:
[CODE_SIGNING_POLICY.md](CODE_SIGNING_POLICY.md) (free code signing provided by SignPath.io,
certificate by SignPath Foundation).

Third-party libraries and their licenses: `THIRD-PARTY-NOTICES.txt`, regenerated by
`python tools/third_party_notices.py` (every release build runs it).
