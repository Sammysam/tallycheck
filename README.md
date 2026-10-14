# Cenango Accounting — offline desktop edition

A fully offline Windows port of the Cenango accounting web application: a Tauri 2 desktop
shell around an embedded Rust (Axum + SQLite) server that serves the original React frontend.
Read `CLAUDE.md` before changing anything — it holds the accounting invariants, the approved
deviations from the reference app and the wave approval gates.

## Status

| Wave | Scope | State |
|---|---|---|
| W0 | Foundation: shell, server, schema, auth + 2FA, users, roles, audit, settings | done (approved) |
| W1 | Ledger: chart of accounts, journal entries, reversals, trial balance, account balances | done, awaiting approval |
| W2–W6 | Operations, reports, employees, payroll, production readiness | not started |

Screens that depend on later waves (dashboard figures, invoices, payroll, …) load but show no
data: their API routes answer 404 until their wave is implemented.

## Layout

```
crates/app-core     config, Nest-shaped errors, money (integer cents), time, ids, DPAPI secrets
crates/database     SQLite pools, pragmas, migrations, settings repository
crates/auth         passwords (Argon2id, legacy bcrypt), JWT, TOTP, throttle, users, roles
crates/audit        activity log and the reference's action summariser
crates/ledger       chart of accounts, journal posting and reversal, balances, trial balance
crates/api-server   Axum routes, validation, middleware, static frontend, accounting-server binary
src-tauri           desktop shell (window on the embedded server)
frontend            the reference React app, copied unchanged
migrations/sqlite   versioned schema migrations
tests/contract      side-by-side comparison with the reference API
tests/ui            headless-Edge smoke test of the real UI
tools               scripts that generate fixtures from the reference code
docs                schema differences and other design notes
```

## Build and run

Prerequisites: Rust 1.99 (pinned in `rust-toolchain.toml`), Node 24, WebView2 (built into
Windows 11).

```
cd frontend && npm ci && npm run build && cd ..
cargo run -p accounting-desktop                    # desktop app
cargo run -p api-server --bin accounting-server    # headless server, open the printed URL
```

First administrator — either set `ADMIN_USERNAME` / `ADMIN_PASSWORD` for the first start (as
in the reference), or:

```
cargo run -p api-server --bin accounting-server -- create-admin admin
```

Every user enrols an authenticator app (TOTP) at first login, as in the reference.

## Configuration

Data lives in `%LOCALAPPDATA%\CenangoAccounting` (override with `ACCOUNTING_DATA_DIR`):
`accounting.db` (+ WAL files), `secrets\` (DPAPI-protected keys), `logs\`, `config.toml`.

```toml
# config.toml — all optional
port = 47800
bind_address = "127.0.0.1"   # anything else requires lan_enabled = true
lan_enabled = false
# frontend_dir = "C:\\path\\to\\frontend\\dist"
```

Environment overrides: `ACCOUNTING_PORT`, `ACCOUNTING_BIND`, `ACCOUNTING_LAN`,
`ACCOUNTING_FRONTEND_DIR`, `RUST_LOG`.

The JWT signing key is created on first start and stored encrypted with Windows DPAPI for the
current user; it never appears in plain text on disk. Logs record method, URL, status and
latency only — never headers, bodies, passwords, hashes, tokens or secrets.

## Tests

```
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
bash tests/contract/run-all.sh                       # needs Docker and the reference repo
node tests/ui/smoke.mjs http://127.0.0.1:47800       # needs a running server with a fresh DB
```
