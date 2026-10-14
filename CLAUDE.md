# CLAUDE.md — Accounting Desktop (offline Rust port)

Read this whole file before changing anything. It overrides default behaviour.

## 1. What this project is

A fully offline Windows desktop port of the Cenango accounting web app.

- **Reference (source of truth, READ-ONLY):**
  `C:\Users\moham\OneDrive\Desktop\Risahd\New folder\Test project\QuickBooks`
  NestJS backend (`backend/src`), React frontend (`frontend/src`), PostgreSQL migrations
  (`backend/src/migrations`). Never write to it, never run builds or `npm install` inside it,
  never copy its `backend/.env` (it holds production secrets).
- **This repo:** `C:\dev\accounting-desktop` — deliberately outside OneDrive. OneDrive sync
  corrupts live SQLite WAL databases and thrashes `target/`.

If the reference and this file disagree about *behaviour*, the reference wins unless the
deviation is listed in §9 (Approved deviations).

## 2. Architecture

| Layer | Technology |
|---|---|
| Desktop shell | Tauri 2 (`src-tauri`) — a window pointed at the embedded server |
| HTTP API | Rust, Tokio, Axum — same paths/JSON as the Nest API under `/api` |
| Database | SQLite via SQLx, versioned migrations in `migrations/sqlite` |
| Frontend | Copy of the reference React app (`frontend/`), minimal changes |
| PDFs | Generated in the frontend (jsPDF + html2canvas), never in Rust |

Runtime shape: one process. The Axum server serves **both** the built frontend and `/api`
from the same origin (`http://127.0.0.1:<port>`). The Tauri window loads that URL, so the
frontend's `baseURL: '/api'` works unchanged. LAN clients (W6) use a browser against the
main PC's server. Other PCs never open the SQLite file directly.

No Node/NestJS/PostgreSQL/Docker/cloud dependency at runtime. Node is a dev-time tool only
(frontend build, contract tests against the reference).

### Crates (no cycles; arrows = "depends on")

- `app-core` — config, errors (Nest-compatible shape), money, time, ids, permission keys,
  DPAPI secret storage. Depends on nothing internal.
- `database` — SQLite pools, pragmas, migrations, settings repository. → app-core
- `auth` — passwords, JWT, TOTP, login throttle, users & roles services. → database, app-core
- `audit` — audit summariser and log storage. → database, app-core
- `api-server` — Axum router, extractors, validation, handlers, static files, the
  `accounting-server` binary. → all of the above
- `src-tauri` — desktop shell. → api-server
- Later waves add: `ledger`, `invoices`, `payments`, `payroll`, `employees`, `documents`,
  `reports`, `backup`, `importer`.

## 3. Accounting invariants (non-negotiable)

- Every financial mutation posts a journal entry; the ledger is the only source of balances.
- Every posted entry balances exactly to the cent (integer comparison, no tolerance).
- Posted journal entries and lines are immutable. **Never delete a posted journal entry.**
  Corrections are reversals (+ replacement entry when appropriate). DB triggers enforce this.
- The only permitted update to a journal entry is `status` POSTED → REVERSED (+ `updatedAt`).
- A business document and its journal entry are written in **one SQLite transaction**.
- Retried requests must not double-post (idempotency per source document + status checks
  inside the transaction).
- Reports derive from ledger lines using the reference's rules (§9 D1 for the one fix).
- Preserve the chart-of-accounts codes the reference hard-codes (1100 AR, 2000 AP, 1300,
  2200–2260, 6100–6120, …), posting dates, status transitions and rounding.
- **Do not invent accounting rules.** If the reference is silent, stop and ask.

## 4. Money

- Stored as **INTEGER minor units (cents)** in SQLite `STRICT` tables, column names unchanged
  (e.g. `"totalAmount"` holds cents). Quantities (`numeric(15,2)`) and `daysWorked`
  (`numeric(5,2)`) are stored as hundredths.
- Never `f32`/`f64` for money. `app-core::money::Money(i64)` for amounts; `rust_decimal` for
  rate multiplication (payroll) with explicit rounding.
- Rounding: exact decimal **half-up (away from zero)** to 2 dp at the same steps the
  reference rounds (§9 D4). Differential tests compare against the reference JS functions.
- JSON: values the reference returns from `numeric` columns are **strings with 2 dp**
  (`"83850.00"`, TypeORM/pg behaviour); values the reference computes in JS are **numbers**.
  Reproduce per endpoint. Request bodies accept JSON numbers with ≤ 2 dp.

## 5. Payroll reproduction

`backend/src/modules/payroll/payroll-rates.ts` (`computeLine`, `calcPaye`) is reproduced
exactly: proration, EPF base = basicForMonth + BRA2005 + BRA2016, EPF 8/12, ETF 3, progressive
monthly PAYE slabs on gross, flat stamp duty and employer welfare when gross > 0, rounding at
each step. Required regression (April 2026): gross 83,850; EPF employee 3,520; EPF employer
5,280; ETF 1,320; net 80,005. The audit reproduced these with the reference code using
basic+BRA = 44,000, allowances 39,850, 30/30 days, welfare 300 — the real employee's split is
still needed (§10). Never change a formula to make a test pass; report the discrepancy.

## 6. API compatibility

- Same paths under `/api`, methods, status codes, JSON shapes, validation messages
  (class-validator wording), pagination `{data,total,page,limit,totalPages}` (limit ≤ 100).
- Error body: `{statusCode, timestamp, path, message}`; `message` is a string or an array of
  per-field strings; `path` is the full URL incl. query. Unknown routes: 404
  `"Cannot <METHOD> <url>"` (checked before auth, like Express). Unexpected failures:
  `{"statusCode":500,"message":"Internal server error"}`.
- Guard order: authentication (401) → permission (403) → validation (400) → handler.
- Unknown body properties are stripped (whitelist), not rejected.
- Contract tests (`tests/contract`) run identical requests against the reference (in Docker
  with a scratch Postgres) and this server. Do not claim compatibility without them.

## 7. Security

- Passwords: Argon2id for new hashes; legacy bcrypt hashes verified then rehashed on login.
- JWT HS256, claims `{sub, username, stage, iat, exp}`; access 8 h, pre-2FA 10 min.
  The JWT secret is random per installation, stored DPAPI-protected in the data directory.
- TOTP (SHA-1, 6 digits, 30 s, ±1 step) is mandatory for every user, as in the reference.
- Exact permission keys from the reference `permissions.ts`; Admin role bypasses checks.
- Server binds 127.0.0.1 by default. LAN requires `lan_enabled = true` in config. With LAN off,
  requests whose `Host` is not loopback are refused (DNS-rebinding defence).
- Never log passwords, hashes, TOTP secrets, tokens, keys or document contents. No request
  bodies in logs.
- Encrypted files (W4): reference format `"QBE1" | IV(12) | TAG(16) | ciphertext`,
  AES-256-GCM, raw 32-byte key; key protected with DPAPI plus an offline recovery copy.

## 8. Database & migrations

- SQLite, `STRICT` tables, quoted camelCase column names identical to the reference.
- Every connection: `foreign_keys=ON` (verified by test), `journal_mode=WAL`,
  `synchronous=FULL`, `busy_timeout=5000`. One writer connection, separate read-only pool.
- Enums → `CHECK` constraints; `jsonb` → TEXT with `json_valid` checks; UUIDs generated in Rust
  (seed rows use a SQL v4 expression); timestamps TEXT ISO-8601 UTC with milliseconds
  (`2026-10-14T05:30:00.000Z`); dates TEXT `YYYY-MM-DD` with a `date()` check.
- Migrations are append-only. Never edit an applied migration; add a new one.
- `docs/schema-differences.md` records every difference from PostgreSQL.

## 9. Approved deviations from the reference

Adopted when W0 was approved on 2026-10-14. The user approved the plan containing these
recommendations without answering each one individually; revisit any of them on request.

- **D1 Reversal balances:** the reference counts only POSTED entries, so a reversed entry is
  excluded while its reversal is counted (voiding a 100 invoice leaves AR at −100). Here
  balances count the original **and** its reversal (net zero). Historical reports will differ
  from the reference wherever documents were voided; the importer reconciliation must show it.
- **D2 Security fixes:** 2FA setup is refused once 2FA is enabled (the reference let a password
  alone re-enrol 2FA); inactive users cannot log in or use any token; failed logins are
  throttled (5 failures / 15 min per username → 429) and audited as "Failed login".
- **D3 Money as integer cents** (§4).
- **D4 Exact decimal half-up rounding** instead of JS binary-float quirks; any input where the
  two differ is reported by the differential tests.
- **D5 "Today"** is the Sri Lanka local date (UTC+05:30, no DST), not UTC.
- **D6 Foreign keys** are added where the reference relationship is unambiguous but no FK
  existed (journalEntryId columns, reversalOfId, payment references, createdById, …). The
  importer must report orphans instead of failing silently.
- **D7 Repo location** `C:\dev\accounting-desktop`.
- **D8 Full schema** is created in W0 so later waves do not churn tables.
- Non-UUID ids in paths return 404 instead of the reference's accidental Postgres 500.
- Login with a missing username/password returns 401 "Invalid credentials" (the reference
  crashed with 500).
- No CORS layer: the frontend is same-origin. Tokens travel in the Authorization header, never
  cookies, so cross-site request forgery does not apply.
- **D9 User role changes persist** (found by the W0 contract tests). In the reference,
  TypeORM 1.0's eager `role` relation overrides `roleId` on save: changing a user from one role
  to another is silently dropped, and a change from "no role" is saved but answered with the
  stale `role: null, roleId: null`. Here `PATCH /users/:id` saves the new role and returns it.
- `null` in optional PATCH fields means "not sent" (the reference stored the text "null" in
  settings, and crashed with 500 for `isActive: null` / role `name: null`).
- 2FA codes are throttled per user like passwords (5 failures / 15 min).
- W1 ledger/accounts (verified by the contract suite):
  - D1 confirmed live on the reference: after reversing a 250.00 expense its trial balance
    showed Cash 1,350.80 (correct 1,100.80) and the expense at −250.00. Here both count.
  - Reversals are dated with the local business date (D5); a date-ranged balance shows the
    original in its period and the reversal in its own period.
  - Changing an account to a code that already exists is a 409 (the reference crashed, 500).
  - Posting to, or parenting under, an unknown account is a 404 `Account <id> not found` (the
    reference crashed on the foreign key, 500).
  - Raw date query parameters (`asOf`, `dateFrom`, `dateTo` on trial balance / balance) must be
    `YYYY-MM-DD`, else 400 (the reference passed them to SQL and crashed).
  - `null` for an account's code, name or type in PATCH is ignored (reference: 500).
  - Entry numbers are taken inside the posting transaction: a failed post never skips a number.
  - Journal lines are returned in posting order (the reference returned them unordered).

Reference quirks reproduced on purpose (verified by tests): audit actions such as
"Created an user" and "Posted a payroll" (the payroll-specific texts are unreachable in the
reference); `POST /users` returns the user without a `role` key; `POST /users/:id/reset-*`
answer 201.

## 10. Open items

- April 2026 regression: need the real employee's salary row (basic / BRA2005 / BRA2016 /
  vehicle / other split). Until then the fixture is reverse-engineered and marked as such.
- Production files live in S3 + key in the reference `.env`; W6 needs an export procedure.
- Reference quirks to decide in their wave: bills never accumulate partial AP payments; a
  part-paid bill can be voided; `POST /accounts/seed` does nothing after the payroll migration
  seeded 11 accounts.
- The login page shows its own fixed "Invalid username or password" for every error, so a
  throttled (429) or inactive (403) user does not see the real reason. Changing it is a
  frontend change; not done in W0.
- The copied frontend fails `npm run lint` (77 errors, 10 warnings) exactly as the reference
  does, and `npm audit --omit=dev` reports advisories in react-router, form-data and dompurify.
  Upgrades need a decision; they are not "minimal changes".
- **Fresh installs have no base chart of accounts.** The migration seeds only the 11 payroll
  accounts, and `POST /accounts/seed` (like the reference) does nothing once any account exists,
  so 1000 Cash, 1100 AR, 2000 AP … must be created by hand or come from the importer. W2
  postings need 1100 and 2000. Decide before W2 whether seeding should add missing codes.
- The production reference server's time zone (for importing `timestamp without time zone`
  values) is unconfirmed.

## 11. Commands

```
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
cd frontend && npm ci && npm run build && npm run lint
cargo run -p api-server --bin accounting-server -- serve
cargo run -p api-server --bin accounting-server -- create-admin <username>
cargo run -p accounting-desktop
```
Contract tests: see `tests/contract/README.md` (`bash tests/contract/run-all.sh`).
UI smoke test: `node tests/ui/smoke.mjs http://127.0.0.1:47800` against a fresh database.
Reference-derived fixtures are regenerated with the scripts in `tools/`; never hand-edit them.

## 12. Waves and approval gates

W0 foundation → W1 ledger → W2 operational accounting → W3 reports → W4 employees →
W5 payroll → W6 production readiness. One wave at a time. At the end of each wave run every
command in §11, exercise the UI against the new backend, inspect logs for secrets, write the
wave report (features, files, tests with real results, failures, risks, next step) and
**STOP until the user explicitly approves the next wave.** Never claim a command passed
unless it ran and its output was read.

## 13. Working rules

- Small, testable modules; no placeholder code that fakes success; never swallow errors
  (exception: the audit write, which the reference also treats as best-effort — it is logged).
- Don't redesign the UI. Don't start future waves early. Never commit secrets or real data.
- Prefer reading the reference over asking; when evidence is missing, ask instead of guessing.
