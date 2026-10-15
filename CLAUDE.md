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
- `ledger` (W1) — accounts, journal entries, balances. → database, app-core
- `operations` (W2) — customers, vendors, banking, invoices, bills, expenses, payments. → ledger
- `reports` (W3) — profit & loss, balance sheet, AR/AP ageing. → ledger, database
- Later waves add: `payroll`, `employees`, `documents`,
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
- LAN mode (W6, `docs/lan-mode.md`): clients must come from `lan_allowed_networks` (private
  ranges by default) and use an IP, `localhost`, this PC's name or a configured name as host;
  plain HTTP on the LAN is a documented limitation. Backup/restore endpoints answer only an
  Administrator connected from loopback. The data folder must be local (network drives refused)
  and is locked to one server.
- Server binds 127.0.0.1 by default. LAN requires `lan_enabled = true` in config. With LAN off,
  requests whose `Host` is not loopback are refused (DNS-rebinding defence).
- Never log passwords, hashes, TOTP secrets, tokens, keys or document contents. No request
  bodies in logs.
- Encrypted files (W4): reference format `"QBE1" | IV(12) | TAG(16) | ciphertext`,
  AES-256-GCM, raw 32-byte key, fresh random IV per file. Files live under
  `<data_dir>/files/<storage key>` with a `.meta` content-type sidecar (the reference's local
  driver layout). The key is `secrets/storage-key.dpapi` (DPAPI, current user) and is created
  on first start. `accounting-server export-storage-key` prints it for an offline recovery
  copy; `import-storage-key` installs one and refuses to replace a different key. Without the
  recovery copy, files cannot be opened after a reinstall or on another PC.
- Downloads go through the server only, with `Cache-Control: private, no-store`.

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
- **D10 Seeding adds missing default accounts** (approved by the user 2026-10-14):
  `POST /accounts/seed` inserts each of the 10 default codes that does not exist yet and never
  touches existing accounts. The reference only seeded an empty chart.
- W2 (verified by the contract suite, 232 steps):
  - Every document and its journal entry are written in one transaction; status checks inside it
    make retried posts/payments fail instead of double-posting.
  - Unknown customer, vendor, account or bank-account ids are a 404 `<Thing> <id> not found`
    (the reference crashed on the foreign key, 500). Payments must reference an existing
    customer/vendor if one is given (the reference stored any id).
  - "Cannot void a paid bill — reverse the payment first": the reference source file garbled
    the dash (`â€”`); the intended text is sent.
  - Line amounts: exact half-up (D4). 0 of 20,000 random quantity × price pairs differ from the
    reference; known edge cases such as 0.29 × 0.50 give 0.15 here and 0.14 there.
  - Response shapes reproduce TypeORM exactly: a created invoice/bill echoes `unitPrice` and
    `amount` as JSON numbers but `quantity`/`totalAmount` as strings and has no `customer` /
    `vendor`; a draft PATCH that replaces lines answers `totalAmount` as a number.
- **D11–D15** (approved by the user 2026-10-14, after W2):
  - D11/D12: an empty string in an optional email or date field means "not given". The
    Customers/Vendors forms send `email: ""` and the Bills form `dueDate: ""`; the reference
    rejected both, which made those fields required in practice.
  - D13: bills accumulate AP payments in a new `bills."amountPaid"` column (migration 0002, kept
    out of the JSON so the bill shape is unchanged) and become PAID when nothing is outstanding;
    an AP payment larger than the outstanding amount is refused, as for invoices. The importer
    must back-fill `amountPaid` from each bill's AP payments.
  - D14: a DRAFT bill must be posted before it can be paid (the reference marked it PAID
    without ever posting it).
  - D15: a part-paid bill cannot be voided (the reference voided it and left the payment's AP
    debit standing alone).
- W3 reports (verified by the contract suite, 248 steps):
  - Profit & Loss and Balance Sheet come only from ledger lines and count every entry (D1), so
    voided documents cancel out. On ranges without voids they match the reference exactly.
  - Totals are exact cents (D4); the reference summed floats and could show e.g.
    3687.4300000000003. `balanced` is an exact comparison (the reference allowed 0.01).
  - AP ageing ages what is still owed on each POSTED bill (D13); AR ageing as the reference.
  - Report dates: absent → default (1 January of the current local year / today, D5), empty →
    no bound (as the reference), malformed → 400 (the reference crashed with 500).

- W4 employees (verified by the contract suite, 304 steps; awaiting approval with W4):
  - Salary states use the local date (D5); the reference used the UTC date.
  - `employees.userId` must name an existing user (D6 adds the foreign key): 404
    `User <id> not found` instead of a stored dangling id.
  - Replace and PATCH of a document validate the UploadDocumentDto rules as optional. The
    reference left those bodies unvalidated, so a bad category or date was a database 500.
    A blank title on replace keeps the previous title, as in the reference.
  - Null for a NOT NULL column (employee name / staff number / status / type, document
    title / category, staff loan balance, salary amounts) means "not changing it" instead of
    a database 500.
  - Uploaded file names are kept as real UTF-8 (multer decoded them as Latin-1, so Sinhala or
    Tamil names were garbled). `Content-Disposition` reduces the plain `filename=` part to
    ASCII; `filename*` carries the exact name, byte for byte as the reference.
  - Amounts beyond numeric(15,2) are a 400 `Amount is too large` (the reference: 500).

- W5 payroll runs (verified by the contract suite, 360 steps; awaiting approval with W5):
  - `computeLine` / `calcPaye` are compared with the reference's own functions on 401
    generated lines and 255 PAYE cases (`tools/gen_payroll_fixture.mjs`). The April 2026
    regression row matches exactly. 6 of the 401 lines differ by one cent, each an exact
    half-cent tie (e.g. 269,874.97 × 15/30 = 134,937.485) that the reference's binary floats
    round down and D4 rounds up. Ties come from fractional days worked; the test proves every
    difference is such a tie.
  - Creating, posting, paying, remitting and voiding a run are each one transaction with their
    journal entries and loan-balance changes (the reference wrote them in separate
    statements). A posting the ledger refuses leaves the run DRAFT and the loans untouched.
  - Void reversals are dated with the local date (D5), like the W2 voids.
  - The trial balance after a voided run nets every payroll account to zero (D1); the
    reference keeps only the reversals (salaries expense -103,400.25 in the contract run).
  - Payslips, bank advice, EPF C Form, ETF form and the acknowledgement sheet are generated by
    the copied frontend in the browser, unchanged; filed slips go through the W4 documents API.

- W6 adds endpoints the reference does not have (`/api/system/backup*`, `/api/system/restore*`)
  and a plain page at `/system/backup`; nothing of the reference API changes. Design and
  recovery scenarios: `docs/backup-and-recovery.md`; importer: `docs/importer.md`.

Reference quirks reproduced on purpose (verified by tests): audit actions such as
"Created an user" and "Posted a payroll" (the payroll-specific texts are unreachable in the
reference); `POST /users` returns the user without a `role` key; `POST /users/:id/reset-*`
answer 201.

## 10. Open items

- April 2026 regression: need the real employee's salary row (basic / BRA2005 / BRA2016 /
  vehicle / other split). Until then the fixture is reverse-engineered and marked as such.
  W5 computes the reverse-engineered row exactly as the reference does.
- Payroll rounding: keep D4 (exact half-up, 1-cent differences on exact ties) or reproduce
  the reference's float rounding bit for bit. Needs the user's decision.
- Production files live in S3 and their key in the reference `.env`. The container format is
  identical, so W6 can copy the encrypted objects unchanged; the administrator installs the
  production key with `import-storage-key` (typed in by them, never copied from the `.env`).
- Reference quirks to decide in their wave: bills never accumulate partial AP payments; a
  part-paid bill can be voided; `POST /accounts/seed` does nothing after the payroll migration
  seeded 11 accounts.
- The login page shows its own fixed "Invalid username or password" for every error, so a
  throttled (429) or inactive (403) user does not see the real reason. Changing it is a
  frontend change; not done in W0.
- The copied frontend fails `npm run lint` (77 errors, 10 warnings) exactly as the reference
  does, and `npm audit --omit=dev` reports advisories in react-router, form-data and dompurify.
  Upgrades need a decision; they are not "minimal changes".
- Fresh installs: run `POST /accounts/seed` (D10) to get 1000 Cash, 1100 AR, 2000 AP …; the
  migration still seeds only the 11 payroll accounts, as the reference did.
- `currentBalance` on bank accounts is never updated by postings (balances come from the
    ledger); there is no Banking screen in the frontend.
- W6: no code-signing certificate is available, so installers built here are unsigned
  (`docs/installer-and-updates.md`). LAN traffic is plain HTTP (`docs/lan-mode.md`).
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
cargo run -p api-server --bin accounting-server -- export-storage-key   # offline recovery copy
cargo run -p api-server --bin accounting-server -- import-storage-key   # restore on a new PC
cargo run -p accounting-desktop
cargo run -p api-server --bin accounting-server -- backup <folder> | verify-backup <file> | restore <file>
cargo run -p api-server --bin accounting-server -- set-backup-password
cargo run -p importer -- --source postgres://… --source-tz +05:30 --target <new folder> --files <folder>
npm ci && npm run installer                       # Windows installer (frontend/dist must be built)
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
