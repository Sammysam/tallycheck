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
- TOTP (SHA-1, 6 digits, 30 s, ±1 step) is **optional per user** (D16, requested by the user
  2026-10-15). Without it, login returns an `accessToken` straight away; a signed-in user can turn
  it on from the status bar ("Turn on 2FA") or the File menu; once on, every login needs the code. An administrator's
  "Reset 2FA" turns it off for that user.
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

- **D16 Optional 2FA** (requested by the user 2026-10-15): the reference forced every user to
  enrol TOTP at first login. Now `POST /auth/login` for a user without 2FA answers
  `{tempToken, requiresTwoFactor:false, isNewUser:false, accessToken}`; users with 2FA are unchanged.
  Frontend changes (minimal): Login accepts the direct token; Layout offers "Turn on 2FA";
  TwoFactorSetup works with the signed-in token and has "Not now"; Users page reset texts.
  Risk accepted: such accounts are protected by the password alone (throttling and failed-login
  audit still apply) — matters most in LAN mode.

- **D17 First-run administrator** (requested by the user 2026-10-19): a fresh install has no
  users, so the sign-in page sends the browser to `/setup` to create the first administrator.
  `GET /api/auth/first-run` → `{needsSetup, canSetUpHere}`; `POST /api/auth/first-run`
  `{username, password}` (same rules as creating a user) → 201 `{accessToken}`. Refused (403)
  once any user exists and from any address that is not loopback, so a LAN client can never
  claim a fresh install. The check and insert share the writer transaction. Audited as
  "Created the first administrator". `ADMIN_USERNAME`/`ADMIN_PASSWORD` and `create-admin` still work.
- **D18 Void a payment** (requested by the user 2026-10-20). The reference could not undo a
  payment, although voiding a paid invoice told the user to "reverse the payment first".
  `POST /api/payments/:id/void` (permission `payments`, 201) reverses the payment's entry dated
  today, takes the amount off the invoice's or bill's `amountPaid` (PAID → POSTED) and marks the
  payment `status` VOID (new column, default POSTED; payments JSON gains `status`). The row is
  kept. One transaction; a second void is 400 "Payment is already voided". Audited as "Voided a
  payment". `POST /api/ledger/entries/:id/reverse` now refuses an entry posted by an invoice,
  bill, expense, payment or payroll run (400 "This entry was posted by a payment. Void or unpost
  it from that screen instead."), since reversing it there left the document out of step.
- **D19 Books lock date** (requested by the user 2026-10-20). Settings key `booksLockedThrough`.
  `ledger::entries::insert` — the one path every posting and reversal takes — refuses any entry
  dated on or before it (400 "The books are locked up to and including YYYY-MM-DD. …"), so it
  covers documents, payroll and manual entries alike. Voids and unposts are dated today, so a
  locked period is corrected in the open one. `GET /api/books/lock` (any signed-in user) →
  `{lockedThrough}`; `PUT /api/books/lock` `{lockedThrough: "YYYY-MM-DD" | null}` is Admin only
  (403 otherwise), the date may not be after today, audited as "Changed the books lock date"
  with "Locked through …" / "Unlocked". Shown and changed under Settings → Lock the books.
- **D20 Default chart on a fresh install** (requested by the user 2026-10-20). When the server
  starts (or `create-admin` runs) on a database with no users, `seed_fresh_install` adds the D10
  default chart (1000 Cash, 1100 AR, 2000 AP, 3000, 4000, 6000, …) next to the payroll accounts,
  so nobody has to press "Seed". Only missing codes are added, and never once a user exists, so
  an imported database or a chart someone has changed is left alone.
- **D21 Credit notes and debit notes** (requested by the user 2026-10-20). Table `credit_notes`
  (type AR/AP, `noteNumber` CN-/DN-000001, invoiceId or billId, date, reason, amount, accountId,
  status POSTED/VOID, journalEntryId, createdById) and `amountCredited` on invoices and bills
  (exposed in invoice JSON; internal on bills like `amountPaid`). AR: debit a revenue account,
  credit 1100; AP: debit 2000, credit an expense or asset account. Only against a POSTED
  document, never more than outstanding = total − paid − credited, not dated before the
  document. The document becomes PAID when payments + credits cover it, POSTED again when a
  void reopens it. `POST /api/credit-notes` / `/api/debit-notes` (permissions `invoices` /
  `bills`, credit notes follow `invoices:all` scope), `GET` lists, `POST …/:id/void` reverses the
  entry dated today. Invoices and bills with notes cannot be voided or unposted until the notes
  are voided. Payments and AR/AP ageing use the same outstanding figure. Refunding money a
  customer already paid is not covered (a note cannot exceed what is owed).
- **D22 Bank reconciliation** (requested by the user 2026-10-20). Tables `bank_reconciliations`
  (accountId, statementDate, statementBalance, openingBalance = previous completed statement
  balance, status DRAFT/COMPLETED, completedAt/By) and `reconciled_lines` (journalLineId PK →
  reconciliationId), so journal lines stay immutable and a line is cleared at most once. Any
  active ASSET account can be reconciled; one DRAFT per account; statement dates move forward.
  Candidates: lines on the account dated ≤ statement date not cleared elsewhere; a REVERSED entry
  and its reversal, both unticked, are left off (they net to zero). Complete only when
  opening + ticked debits − ticked credits = statement balance exactly. Only the latest completed
  one can be undone (back to DRAFT, ticks kept). Nothing posts to the ledger: bank charges are
  journal entries first. Routes under `/api/reconciliations` (permission `ledger`): GET
  `?accountId=`, POST start, GET/DELETE `:id`, PUT `:id/lines` `{lineIds}`, POST `:id/complete`,
  POST `:id/undo`. Screen: Accounting → Bank Reconciliation. The unused `bank_accounts` table
  is untouched.
- **D23 Statements, drill-down and export** (requested by the user 2026-10-20).
  * `GET /api/customers/:id/statement?dateFrom&dateTo` (permission `invoices`, `invoices:all`
    scope): POSTED/PAID invoices less POSTED AR payments and POSTED credit notes, date order,
    running balance, balance brought forward; drafts and voids never count. With no end date the
    closing balance equals the customer's outstanding invoices. Screen: Customers → Statement
    (company letterhead, Print / Save as PDF).
  * `GET /api/ledger/activity/:accountId?dateFrom&dateTo` (permission `ledger`): opening balance,
    every line with a running balance in the account's normal direction (all entries, D1),
    closing = trial balance for the same dates. Account names in Trial Balance, Profit & Loss and
    Balance Sheet link to it (`/ledger/account/:id`) with the report's dates. The P&L, Balance
    Sheet and ageing screens now draw their figures as grid tables (`report-table`).
  * Export (toolbar, File menu, Ctrl+E): every table on the screen into one UTF-8 CSV named after
    the screen heading, amounts as plain numbers, action/checkbox columns left out, text that
    Excel would run as a formula prefixed with an apostrophe (`lib/exportCsv.ts`).
- **D24 Cash flow, comparisons, sign-in messages** (requested by the user 2026-10-20).
  * `GET /api/reports/cash-flow?dateFrom&dateTo&cashAccounts=id,id` (permission `reports`; dates
    as for P&L; cashAccounts default = active 10xx asset accounts). Direct method from the
    ledger: for each entry in the period touching a cash account, every other line's
    (credit − debit) is cash in/out, grouped by that account. Investing = assets whose subtype
    contains fixed/non_current/noncurrent/long_term/investment; Financing = equity and
    liabilities whose subtype contains long_term/non_current/noncurrent/loan; everything else
    Operating. Cash-to-cash moves drop out. The handler refuses (500) if opening + net change ≠
    the cash accounts' ledger balance, which cannot happen while entries balance.
  * P&L "Compare: previous period / same period last year" and Balance Sheet "Compare: same date
    last year / last year end": a second request, a comparison column and a Change column
    (frontend only, `lib/periods.ts`).
  * Sign-in screens show the server's reason for 429 (too many attempts), a turned-off account
    (403, only after the right password), an unreachable server and server errors; a wrong
    username or password stays one message (`lib/signInError.ts`).
- **D25 Any company, any country** (requested by the user 2026-10-20). The app is a product
  sold to many companies, so **no customer's own details ship with it** — never add a company
  name, address, bank account, signatory, registration number or logo to code or seed data.
  * Company and region are settings (`company.name/address/phone/fax/email/website/taxNumber/
    logo/paymentInstructions/footerNote`, `region.country/currency/currencySymbol/currencyWord/
    currencyMinorWord/locale/fiscalYearStart`), validated in `validate::update_settings` (ISO
    codes, BCP-47 locale, month 1–12, logo a PNG/JPEG/WebP data URL ≤ 400k chars). The
    reference's signatory defaults are gone (`settings::DEFAULTS` is empty); migration
    `20261020000004` blanks the baseline's seeded signatory and payroll company details on a
    fresh install (an installation already in use keeps what its owner saved).
  * The administrator is sent to `/company-setup` after the first sign-in until `company.name`
    is set; also editable under Settings → Company. Country presets (`lib/countries.ts`) fill
    currency, number format, currency words and financial year; only 2-decimal currencies.
  * The frontend formats every amount with `lib/money.ts` (company locale and currency) and
    prints letterheads from `lib/region.ts` (`company()`); invoices, statements, payslips and
    CSV export follow it. The Sri Lankan EPF/ETF statutory forms keep "Rs.".
  * "Today" is the main PC's own local date (`chrono::Local`), replacing the fixed +05:30 of D5.
  * Report defaults (P&L, cash flow, dashboard, statements) start at the financial year start.
  * "Cenango Accounting" remains the product's own name (window title, installer, data folder).
- **D26 Taxes for any country** (requested by the user 2026-10-20). No country's tax law is
  built in; each company defines its own **tax codes** (`tax_codes`: name, rate in ten-thousandths
  of a percent — 18% = 180000 — sales account (liability) for output tax, purchase account
  (asset or liability) for input tax, active flag). `GET /api/tax-codes` (any signed-in user),
  `POST`/`PATCH /:id`/`DELETE /:id` (permission `settings`; a code used on any document cannot be
  deleted, only switched off; a switched-off code is refused on new lines).
  * Invoice and bill lines carry `taxCodeId` and `taxAmount`; documents carry `taxAmount` and
    `pricesIncludeTax`. Tax is integer arithmetic, half-up **per line** (`operations::tax`):
    exclusive tax = amount × rate; inclusive tax = amount × rate / (1 + rate). The document
    total includes the tax; revenue/expense lines post the net, tax posts per code to the tax
    accounts. Expenses and credit/debit notes take an optional `taxCodeId`; their amount is
    gross (tax included) and the tax inside it posts/reverses to the tax account.
  * `GET /api/reports/tax?dateFrom&dateTo` (permission `reports`): per code, sales net and output
    tax (posted/paid invoices less posted credit notes) and purchases net and input tax
    (posted/paid bills, posted expenses, less posted debit notes) by document date; drafts and
    voids never count; lines without a code on their own row; `netTax` = output − input.
  * Settings `tax.defaultSales` / `tax.defaultPurchases` preselect the code on new lines.
    Screens: Administration → Taxes, Reports → Tax Return; invoices with tax print as
    "Tax Invoice" with Subtotal, one row per tax code, and Total (form and PDF alike).
  * Country return formats are not built (the return gives the figures to copy).
- **D27 Tax groups and withholding tax** (requested by the user 2026-10-21). `tax_codes.kind` is
  STANDARD, GROUP or WITHHOLDING. A GROUP (`tax_code_components`: ordered standard codes, each
  optionally compound — charged on the net plus the parts before it) is charged on lines like a
  standard code; each part is rounded half-up on its own and posts to its own accounts. With
  prices including tax the net is found from the combined rate, then each part is worked out
  on it and the last part absorbs the rounding difference. Every taxed line's split is stored in
  `document_taxes` (sourceType INVOICE_LINE / BILL_LINE / EXPENSE / CREDIT_NOTE); postings and
  the tax return read it, so later rate or group changes never rewrite saved tax.
  WITHHOLDING codes are never on lines: a payment may carry `withholdingTaxCodeId` and
  `withholdingAmount`; money + withheld settles the document; AR: debit the code's sales
  account (an asset, a tax credit), AP: credit its purchase account (a liability, owed to the
  tax office). The tax return lists a group's parts on their own rows (totals count each line
  once) and adds withholding by payment date. Codes in use, or in a group, cannot be deleted.
- **D28 Dates in the company's style** (2026-10-21): screens print `YYYY-MM-DD` dates with
  `fmtDate` (company locale, e.g. "30 Apr 2026", "Apr 30, 2026"); CSV export keeps ISO dates.
- **D29 Currencies without decimals** (2026-10-21). Amounts stay in hundredths, but when the
  company currency has no minor unit (JPY, KRW, VND, … — `app_core::money::ZERO_DECIMAL`) a
  process-wide switch (`set_currency`, set at start-up and on every `region.currency` save)
  makes line totals, tax parts and conversions round half-up to whole units, and `IsMoneyNumber`
  refuses fractional amounts ("…must be a whole amount…"); quantities use `IsQuantity` and keep
  two decimals. Three-decimal currencies (BHD, IQD, JOD, KWD, LYD, OMR, TND) are refused in
  settings — hundredths cannot hold them. The switch is global, so tests that turn it on live in
  their own test binary (`operations/tests/whole_units.rs`).
- **D30 Foreign-currency invoices and bills** (2026-10-21). An invoice or bill may carry a
  `currency` other than the company's, plus `exchangeRate` (home units per one foreign unit,
  8 decimals, `fx::RATE_ONE`). Amounts on the document stay in its own currency; it posts to the
  ledger in the home currency at its rate (`baseTotal`, `invoices::booked`/`bills::booked`). A
  payment is entered in the document's currency at the day's rate; the books move by
  `baseAmount`, the receivable/payable clears by `baseSettled` (its share as booked, and the
  final payment or note clears exactly what is left), and the difference posts to 7900
  "Foreign Exchange Gain/Loss" (an expense, created when first needed). Credit and debit notes
  use the document's own rate, so they make no exchange difference. The last rate used per
  currency is remembered (`fx.rate.XXX`) as the next default. Statements, ageing and the tax
  return use the as-booked home amounts. Not covered: bank accounts in a foreign currency,
  revaluing open items at period end, and foreign currencies with other than two decimals
  (foreign amounts are held in hundredths).
- **D31 Company setup wizard** (requested by the user 2026-10-20). The first-run company screen
  asks one thing per page (name, address, email, phone, website, country, tax number, logo, how
  customers pay, invoice note, then a review). Each page saves on Next and can be skipped; "Set
  up later" leaves it for this session (a sessionStorage flag) and it asks again at the next
  sign-in until a company name is saved. Administration → Company Setup reopens it. The country
  list now holds every ISO 3166 country with its ISO 4217 currency; those without a preset take
  their name, currency word and symbol from the system's Intl data and a January year start.
- **D32 Journal entries on the Ledger screen** (2026-10-21). The reference had no screen for a manual
  journal entry (only the API). The Ledger page now has "+ New Journal Entry": date, description,
  reference and debit / credit lines, posted only once balanced, plus "Reverse this entry" for manual
  entries (document entries are still undone from their document, D18). A made-up demo company is
  typed in through the screens by `tests/ui/demo-data.mjs` into its own data folder
  (`docs/demo/README.md`, `scripts/run-demo.cmd`).
- **D33 Payable accounts and account edits** (senior's list, 2026-10-21). A bill may be owed on any
  liability (`bills.payableAccountId`, default 2000): posting credits it, its payments debit it and
  its debit notes reduce it, so e.g. Telephone Payable shows on its own Balance Sheet line. Bill
  lines may use expense, asset or liability accounts. An AP payment without a bill pays a liability
  account directly (`payments.payableAccountId`, source `PAYMENT_LIABILITY`): debit the liability,
  credit the money; no withholding. The Chart of Accounts has Edit; an account with ledger lines
  keeps its type and currency.
- **D34 Foreign-currency bank accounts** (optional item, 2026-10-21). An asset or liability account
  may have a `currency` (USD…). Its ledger lines carry `foreignAmount` (signed, debit +), and
  `entries::post` refuses a line on such an account without one, so no screen can move it in
  rupees only; `post_with_foreign` is used by manual journal entries (USD amount × rate on the
  form), payments (documents in the account's own currency; payable-account payments at the day's
  rate) and reversals (mirrored). Expenses and payroll cannot use a foreign account (refused with
  a message — use a journal entry). Account activity shows the USD amounts and balance; bank
  reconciliation of such an account is in USD; `POST /accounts/:id/revalue {date, exchangeRate}`
  posts the unrealised difference to 7900 (Chart of Accounts → Revalue).
- **D35 Transaction details** (2026-10-21). `/ledger/entries/:id` shows one transaction: the entry,
  its lines (account links, USD amounts) and the document behind it (invoice / bill lines, payment,
  expense, note; payroll links to its run). Ledger rows, account-activity entry numbers (so every
  report drills down to it) and the document numbers in the Invoices, Bills, Expenses, Payments and
  Credit / Debit Notes lists open it. Manual entries are reversed from there.
- **D36 Administration redesign** (2026-10-21). An Admin Centre (`/admin`) with status cards (users,
  2FA, last backup, lock date, company details, tax codes), quick actions and the latest activity.
  Users: search, 2FA and last sign-in columns (`GET /users/last-sign-in`, from the log), a "⋯" menu
  and dialogs for role, password and 2FA; nobody can deactivate or demote themselves (the last
  Admin keeps the reference's own rule and message). Roles: permissions in plain words, grouped,
  with user counts and Duplicate. Activity Log: user / kind / date / text filters (`kind`,
  `dateFrom`, `dateTo`, `search` on `/audit-logs`) and Excel export; "an user" is shown as
  "a user" (the stored text is unchanged). Settings: tabs (Company, Region & currency, Invoices &
  documents, Closing, Backup & restore); backup moved into the React app and `/system/backup`
  now redirects there. Toasts and the app's own dialogs replace browser alert / confirm here.
- **D37 Import from file** (2026-10-22, integrations plan phase 1, approved by the user's senior).
  `POST /imports/read` reads an uploaded CSV / Excel file (`operations::sheet`: UTF-8 / UTF-16 /
  Windows-1252, `,` `;` tab; XLSX / XLS / ODS via calamine, numbers kept as their shortest text so
  money is parsed to exact cents, never computed in floating point). The screen (`/import`) maps the
  columns (saved per source in `import_layouts`), `POST /imports/preview` runs the import and rolls
  back, `POST /imports` commits. Kinds: BANK (statement lines into `bank_lines`), INVOICES (DRAFT
  invoices; rows with one number are one invoice; unknown customers are created), CUSTOMERS /
  VENDORS (new names only). Opening balances never reach the server as an import: they open as a
  prefilled journal entry on the Ledger to check and post. Each outside record's key is unique in
  `import_items`, so re-importing a file skips what is there; each row runs in a savepoint; rows with
  errors are reported and skipped. `DELETE /imports/:id` undoes an import while its lines are
  unmatched, its invoices drafts and its customers / vendors unused. Permissions: those of the
  records created (banking, invoices, customers, vendors). Bank Transactions (`/bank-transactions`,
  `banking`): each line is matched to a posted payment / expense on that account (same money and
  direction; suggested within 7 days), or pays an open invoice / bill in the account's currency
  (`payments::create_in`), or becomes an expense created and posted at once
  (`expenses::create_in` + `post_in`), or is ignored. Nothing is matched without a click; unmatching
  keeps the payment / expense. Reading and previewing are not written to the activity log.
- **D38 Products, stock and discounts** (2026-10-22, integrations plan phase 2; rules chosen by the
  user: average cost, a separate discounts account, posting blocked when stock is short). `items`
  (INVENTORY / SERVICE / NON_INVENTORY; `/items`, Master Data → Products & Services; managing needs
  `invoices` or `bills`). Bills: a line with a stock item always debits the item's inventory account
  (1200 by default); posting adds stock at the line's home-currency cost without tax. Invoices:
  posting takes stock out at average cost (value × qty ÷ on hand, half-up; the last units take the
  remaining value) and the invoice's own entry debits COGS (5000) / credits inventory; posting is
  refused with "Not enough stock of …" when short. Unpost / void reverse exactly what the document
  still has moved (`stock::undo_source`); a bill whose stock has since been sold cannot be voided
  (adjust instead). Stock adjustments (`POST /items/:id/adjust`, `ledger` permission) post against an
  account the user chooses (source STOCK_ADJUSTMENT, not reversible from the Ledger). Every
  `stock_moves` value equals what was posted to the inventory account. Invoice lines carry
  `discountAmount` (before tax; `amount` = qty × price − discount, so totals and tax are unchanged);
  posting credits revenue with the price before the discount and debits the discount without its
  tax to 4900 Discounts Given (added when first used, like 7900). "+ Add shipping" adds a line on
  4800 if that account exists. Not yet: stock on credit / debit notes (returns go through an
  adjustment), foreign-currency costs other than via the bill's rate, FIFO.
- **D39 Shopify connector** (2026-10-22, integrations plan phase 3; built against Shopify's published
  Admin REST API and a stand-in server in the tests — not yet tried against a real store). Settings →
  Integrations (`settings`): store address + Admin API access key (custom app; read_orders,
  read_shopify_payments_payouts). The key is DPAPI-encrypted in `<data>/secrets/shopify-token.dpapi`,
  never in the database or a response; settings live under `integration.shopify.*`. The app goes
  online only to sync (`crates/integrations`, reqwest + rustls; 429 retried; pages followed by the
  Link header; `ACCOUNTING_SHOPIFY_BASE_URL` may point at a local stand-in only). A sync (`invoices` +
  `banking`; or hourly when switched on) fetches first, then writes in one transaction with a
  savepoint per order: orders → DRAFT invoices (order number as PO number; product lines with
  their discounts and tax; shipping lines; products matched by SKU; customer by name or created;
  cancelled skipped; an unmatched Shopify tax leaves the order out and is listed for matching; a
  different total is reported). Shopify Payments balance transactions → bank lines on a clearing
  account (1090 Shopify Clearing on request): payments, fees (hint: expense on the fee account),
  refunds, payouts (hint: transfer to the payout account); test transactions skipped. Every order
  and line is keyed (`shopify:<shop>:order|bt|fee:<id>`) so it comes in once, and each sync is an
  import that Import from File can undo. Bank lines gained `TRANSFER` matches (a BANK_TRANSFER
  journal entry, matched once per account, suggested on the other account's statement) and hints
  (`bank_lines` rebuilt; it was new in D37). Refunds still need a credit note by hand (the report
  says so); orders in a currency other than the home currency are not brought in yet.
- **D40 More connectors** (2026-10-22, integrations plan phase 4; built against each service's
  published API with stand-in servers in the tests — not yet tried against real accounts).
  `crates/integrations` now has a shared framework (`common.rs`: settings under
  `integration.<id>.*` with `remoteId/remoteName/remoteCurrency`, credentials as DPAPI JSON in
  `<data>/secrets/<id>-credentials.dpapi`, HTTP with 429 retries, writing orders as DRAFT invoices and
  money movements as hinted bank lines, reports, the hourly loop) and one adapter per service:
  Shopify (D39, moved onto it), **WooCommerce** (site + REST key, Read; https only, query-string key
  as fallback; paid statuses processing / completed / on-hold / refunded become drafts; amounts are
  taken without tax, the discount = subtotal − total; a quantity that does not divide the subtotal
  becomes one "× n" line; tax rates as label|fraction, converted from percent by text),
  **PayPal** (REST app client id + secret, live or sandbox; Transaction Search in 31-day windows;
  completed only; payments, fees, refunds, withdrawals) and **Stripe** (secret or restricted key;
  balance transactions with the charge expanded; smallest units → cents, zero-decimal currencies
  ×100; payments, fees, refunds, payouts). Money in another currency than the money account's is
  left out and reported. Routes are `/integrations` and `/integrations/{id}[/connect|/sync|
  /clearing-account]`; money accounts 1090 Shopify Clearing, 1091 PayPal, 1092 Stripe on request.
  PayHere is not connected: as far as known its API only looks up one order at a time, so its
  settlement export goes through Import from File.
- **D41 Claude (MCP) and keys** (2026-10-22, integrations plan phase 5). `api_keys` (Settings →
  Integrations → Claude, `settings` permission): a key `cak_` + 40 hex acts as the user who made it,
  stored only as SHA-256, shown once, revocable; read-only unless `canWrite`. A request with a key
  (`extract::by_key`, path taken from `OriginalUri`) is accepted only from loopback; reads only, or
  with `canWrite` only `POST /api/invoices|bills|customers|vendors` (drafts) — never post, pay,
  void, delete or change settings; payroll, employees, users, roles, audit logs, system,
  integrations, imports and keys (except `/api-keys/me`) are closed even to reads. The activity log
  names "<user> via <key name>". `crates/mcp` builds `cenango-mcp.exe`, an MCP server over stdio
  (protocol 2025-06-18 / 2025-03-26 / 2024-11-05) that calls the app with `CENANGO_API_KEY` at
  `CENANGO_API_URL` (default http://127.0.0.1:47800): 14 read tools (connection_status,
  company_overview, invoices, bills, payments, expenses, customers, vendors, accounts, run_report,
  account_activity, products, bank lines to review) and, for write keys, create_customer /
  create_draft_invoice / create_draft_bill (names, codes, tax names and SKUs resolved to ids). The
  installer ships it next to the app (`src-tauri/tauri.installer.json`). `claude-plugin/` is a
  Claude Code marketplace with the plugin (the MCP server + skills: month-end-close,
  chase-overdue-customers, prepare-vat-return, explain-a-report, review-online-sales).

- **D42 US payroll taxes** (2026-10-27, ported from the web Rust edition's waves U1 + U2, commits
  `b78e032` and `082082a`; the user asked for the same here). `payroll::us` works out one pay period:
  federal income tax withholding by IRS Pub 15-T (2026) Worksheet 1A with the annual percentage
  tables (2020+ Form W-4 and older W-4s with allowances; no whole-dollar rounding), social security
  6.2% each to $184,500, Medicare 1.45% each, Additional Medicare 0.9% employee-only over $200,000,
  FUTA 0.6% on $7,000 plus a manual credit reduction, and the employer's SUTA rate and wage base.
  Integer cents, rates in ppm, each tax rounded half-up once; checked against all 4,794 values of the
  IRS 2026 wage bracket tables (`crates/payroll/tests/fixtures/us_2026_wage_brackets.json`). Only
  2026 tables. `payroll::us_setup`: settings `payroll.country` (LK default | US) and `payroll.us.*`
  (pay frequency, state, SUTA rate / wage base, FUTA credit reduction, EIN, state account no.),
  table `employee_us_tax` (one current W-4 / pay record per employee, no SSN). Routes (`payroll`):
  GET|PATCH `/payroll/us/settings`, GET|PUT `/payroll/employees/:id/us-tax`, POST
  `/payroll/us/preview` (201, saves nothing). Not built: US pay runs and ledger posting, screens,
  state / local income tax, overtime, employee-paid SUTA, 941 / 940 / W-2. Sri Lankan payroll is
  unchanged. Choices made by the web agent without a spec (company-level state and pay frequency, no
  W-4 history, no dollar rounding) are kept so both editions agree.

- **D43 US pay runs, state income tax and form figures** (2026-10-27, ported from the web Rust
  edition's waves U3 `e1f403d`, U4 `450e073`, U5a `d62293d` and U5b `fa0c09c`; the user asked for
  the same here). `payroll::us_runs`: runs (`USPR-0001`, DRAFT → POSTED → PAID, VOID), one line per
  employee (salary / periods, or hourly hours × rate with 1.5× overtime; bonus, other earnings), W-4
  and state certificate snapshotted on the line, year-to-date wages from other POSTED / PAID runs of
  the pay-date year (worked out again at posting). Each step is one transaction with its entry:
  post (dated the period end, source `US_PAYROLL`) Dr gross wages + employer taxes, Cr net pay owed,
  federal income tax, social security (both shares), Medicare (both + Additional), FUTA, SUTA,
  401(k), section 125, state income tax; pay (`US_PAYROLL_PAYMENT`) Dr net pay owed / Cr the bank;
  remit (`US_PAYROLL_REMITTANCE`) one tax, or a Form 941 deposit clearing federal income tax + social
  security + Medicare in one entry, never more than outstanding; void reverses every entry dated
  today. These entries can't be reversed from the Ledger (D18 message "…a US payroll run…");
  Transaction details link to the run. `payroll::us_state`: each company enters its states' rules
  (FLAT or BRACKETS, standard deductions, per-allowance deduction / credit, 401(k) excluded or not);
  the 9 states without income tax on wages are built in; **no state rates ship**; posting is refused
  while a work state has no rule. `payroll::us_reports`: Form 941 / 940 / W-2 (+ W-3 totals, boxes
  15–17) figures from POSTED / PAID runs by pay date, read-only. Routes (`payroll`): `/payroll/us/runs`
  (+ `/:id`, `/lines`, `/post`, `/pay`, `/remit`, `/void`), `/payroll/us/reports/941|940|w2`,
  `/payroll/us/state-taxes[/:state]`. Screens: Payroll → US payroll (settings, state rules, runs),
  run page, tax forms page, US tax section on the employee profile (instead of the Sri Lankan salary
  section when `payroll.country` = US). Desktop-only, beyond the web edition:
  * **Accounts are chosen, never taken over** (user's choice, option b): each of the 11 roles
    (`us_runs::ROLES`, defaults 6100, 6130, 2200, 2300–2370) is a setting `payroll.us.account.<role>`
    (account id; PATCH `/payroll/us/settings` `accounts: {role: id | null}`). An unset role takes the
    account with its default code if it has the default name and type, or adds it if the code is
    free; when the code is some other account (e.g. a company's own 2300 Sales Tax Payable) switching
    to US is refused: `Account 2300 is already "Sales Tax Payable". Choose the account for Federal
    income tax withheld under US payroll settings`. A chosen account must be active, of the right
    type (liability / expense) and in the home currency. A posted run keeps the accounts it used
    (`accountsSnapshot`), so paying it and its deposits clear those even after the settings change.
  * US payroll needs the company currency to be USD (switching and new runs are refused otherwise).
  The web agent's own choices are kept: one company SUTA state and pay frequency, 2,080 standard
  hours, overtime 1.5× for hourly only, one line per employee per run, no W-4 / state rule history
  (runs keep snapshots), only 2026 federal tables, local taxes not covered.
- Stripe: a publishable key (`pk_…`) is now refused with "That is a publishable key; use a secret or
  restricted key" (the general key check ran first and made that message unreachable; fixed in the
  web editions).

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
- The copied frontend fails `npm run lint` (77 errors, 10 warnings) exactly as the reference
  does, and `npm audit --omit=dev` reports advisories in react-router, form-data and dompurify.
  Upgrades need a decision; they are not "minimal changes".
- Fresh installs get the default chart automatically (D20); the migration itself still seeds
  only the 11 payroll accounts, as the reference did, so imports are unaffected.
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
UI smoke test: `node tests/ui/smoke.mjs http://127.0.0.1:47800` against a fresh, empty database
(no ADMIN_* variables; the test creates the admin on the first-run screen).
Desktop shell (`components/Layout.tsx`): the page draws a Windows-style menu bar, toolbar,
navigation pane and status bar; the Tauri window has no native menu. A list page registers what
Ctrl+N and Esc do with `useCommand('new' | 'close', …)` from `lib/commands.ts`; Ctrl+S submits the
open form, F5 remounts the screen, and browser keys (reload, zoom, right-click menu) are off.
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
