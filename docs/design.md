# TallyCheck design notes

How the app is built and why it behaves the way it does. TallyCheck is an offline desktop
port of the Cenango accounting web application (the "reference": a NestJS + React +
PostgreSQL app). Where this edition behaves differently from the reference, the difference is
recorded in §8 as a numbered decision (D1, D2, …); code comments refer to those numbers.

## 1. Architecture

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

## 2. Accounting invariants (non-negotiable)

- Every financial mutation posts a journal entry; the ledger is the only source of balances.
- Every posted entry balances exactly to the cent (integer comparison, no tolerance).
- Posted journal entries and lines are immutable. **Never delete a posted journal entry.**
  Corrections are reversals (+ replacement entry when appropriate). DB triggers enforce this.
- The only permitted update to a journal entry is `status` POSTED → REVERSED (+ `updatedAt`).
- A business document and its journal entry are written in **one SQLite transaction**.
- Retried requests must not double-post (idempotency per source document + status checks
  inside the transaction).
- Reports derive from ledger lines using the reference's rules (§8 D1 for the one fix).
- Preserve the chart-of-accounts codes the reference hard-codes (1100 AR, 2000 AP, 1300,
  2200–2260, 6100–6120, …), posting dates, status transitions and rounding.
- **Do not invent accounting rules.** If the reference is silent, stop and ask.

## 3. Money

- Stored as **INTEGER minor units (cents)** in SQLite `STRICT` tables, column names unchanged
  (e.g. `"totalAmount"` holds cents). Quantities (`numeric(15,2)`) and `daysWorked`
  (`numeric(5,2)`) are stored as hundredths.
- Never `f32`/`f64` for money. `app-core::money::Money(i64)` for amounts; `rust_decimal` for
  rate multiplication (payroll) with explicit rounding.
- Rounding: exact decimal **half-up (away from zero)** to 2 dp at the same steps the
  reference rounds (§8 D4). Differential tests compare against the reference JS functions.
- JSON: values the reference returns from `numeric` columns are **strings with 2 dp**
  (`"83850.00"`, TypeORM/pg behaviour); values the reference computes in JS are **numbers**.
  Reproduce per endpoint. Request bodies accept JSON numbers with ≤ 2 dp.

## 4. Payroll reproduction

`backend/src/modules/payroll/payroll-rates.ts` (`computeLine`, `calcPaye`) is reproduced
exactly: proration, EPF base = basicForMonth + BRA2005 + BRA2016, EPF 8/12, ETF 3, progressive
monthly PAYE slabs on gross, flat stamp duty and employer welfare when gross > 0, rounding at
each step. Required regression (April 2026): gross 83,850; EPF employee 3,520; EPF employer
5,280; ETF 1,320; net 80,005. The audit reproduced these with the reference code using
basic+BRA = 44,000, allowances 39,850, 30/30 days, welfare 300 — the real employee's split is
still needed (§9). Never change a formula to make a test pass; report the discrepancy.

## 5. API compatibility

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

## 6. Security

- Passwords: Argon2id for new hashes; legacy bcrypt hashes verified then rehashed on login.
- JWT HS256, claims `{sub, username, stage, iat, exp}`; access 8 h, pre-2FA 10 min.
  The JWT secret is random per installation, stored DPAPI-protected in the data directory.
- TOTP (SHA-1, 6 digits, 30 s, ±1 step) is **optional per user** (D16,
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

## 7. Database & migrations

- SQLite, `STRICT` tables, quoted camelCase column names identical to the reference.
- Every connection: `foreign_keys=ON` (verified by test), `journal_mode=WAL`,
  `synchronous=FULL`, `busy_timeout=5000`. One writer connection, separate read-only pool.
- Enums → `CHECK` constraints; `jsonb` → TEXT with `json_valid` checks; UUIDs generated in Rust
  (seed rows use a SQL v4 expression); timestamps TEXT ISO-8601 UTC with milliseconds
  (`2026-10-14T05:30:00.000Z`); dates TEXT `YYYY-MM-DD` with a `date()` check.
- Migrations are append-only. Never edit an applied migration; add a new one.
- SQL text is a literal wherever possible, with every value bound (`?1`, `.bind(…)`). sqlx 0.9
  refuses runtime-built SQL unless wrapped in `sqlx::AssertSqlSafe`: wrap only SQL assembled
  from constants (column lists, fixed table names, `?N` placeholders), never from user input;
  helpers that take SQL as a parameter take `&'static str`.
- `docs/schema-differences.md` records every difference from PostgreSQL.

## 8. Approved deviations from the reference

Adopted when W0 was approved on 2026-10-14. The user approved the plan containing these
recommendations without answering each one individually; revisit any of them on request.

- **D1 Reversal balances:** the reference counts only POSTED entries, so a reversed entry is
  excluded while its reversal is counted (voiding a 100 invoice leaves AR at −100). Here
  balances count the original **and** its reversal (net zero). Historical reports will differ
  from the reference wherever documents were voided; the importer reconciliation must show it.
- **D2 Security fixes:** 2FA setup is refused once 2FA is enabled (the reference let a password
  alone re-enrol 2FA); inactive users cannot log in or use any token; failed logins are
  throttled (5 failures / 15 min per username → 429) and audited as "Failed login".
- **D3 Money as integer cents** (§3).
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
- **D10 Seeding adds missing default accounts** (approved 2026-10-14):
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
- **D11–D15** (approved 2026-10-14, after W2):
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

- **D16 Optional 2FA** (2026-10-15): the reference forced every user to
  enrol TOTP at first login. Now `POST /auth/login` for a user without 2FA answers
  `{tempToken, requiresTwoFactor:false, isNewUser:false, accessToken}`; users with 2FA are unchanged.
  Frontend changes (minimal): Login accepts the direct token; Layout offers "Turn on 2FA";
  TwoFactorSetup works with the signed-in token and has "Not now"; Users page reset texts.
  Risk accepted: such accounts are protected by the password alone (throttling and failed-login
  audit still apply) — matters most in LAN mode.

- **D17 First-run administrator** (2026-10-19): a fresh install has no
  users, so the sign-in page sends the browser to `/setup` to create the first administrator.
  `GET /api/auth/first-run` → `{needsSetup, canSetUpHere}`; `POST /api/auth/first-run`
  `{username, password}` (same rules as creating a user) → 201 `{accessToken}`. Refused (403)
  once any user exists and from any address that is not loopback, so a LAN client can never
  claim a fresh install. The check and insert share the writer transaction. Audited as
  "Created the first administrator". `ADMIN_USERNAME`/`ADMIN_PASSWORD` and `create-admin` still work.
- **D18 Void a payment** (2026-10-20). The reference could not undo a
  payment, although voiding a paid invoice told the user to "reverse the payment first".
  `POST /api/payments/:id/void` (permission `payments`, 201) reverses the payment's entry dated
  today, takes the amount off the invoice's or bill's `amountPaid` (PAID → POSTED) and marks the
  payment `status` VOID (new column, default POSTED; payments JSON gains `status`). The row is
  kept. One transaction; a second void is 400 "Payment is already voided". Audited as "Voided a
  payment". `POST /api/ledger/entries/:id/reverse` now refuses an entry posted by an invoice,
  bill, expense, payment or payroll run (400 "This entry was posted by a payment. Void or unpost
  it from that screen instead."), since reversing it there left the document out of step.
- **D19 Books lock date** (2026-10-20). Settings key `booksLockedThrough`.
  `ledger::entries::insert` — the one path every posting and reversal takes — refuses any entry
  dated on or before it (400 "The books are locked up to and including YYYY-MM-DD. …"), so it
  covers documents, payroll and manual entries alike. Voids and unposts are dated today, so a
  locked period is corrected in the open one. `GET /api/books/lock` (any signed-in user) →
  `{lockedThrough}`; `PUT /api/books/lock` `{lockedThrough: "YYYY-MM-DD" | null}` is Admin only
  (403 otherwise), the date may not be after today, audited as "Changed the books lock date"
  with "Locked through …" / "Unlocked". Shown and changed under Settings → Lock the books.
- **D20 Default chart on a fresh install** (2026-10-20). When the server
  starts (or `create-admin` runs) on a database with no users, `seed_fresh_install` adds the D10
  default chart (1000 Cash, 1100 AR, 2000 AP, 3000, 4000, 6000, …) next to the payroll accounts,
  so nobody has to press "Seed". Only missing codes are added, and never once a user exists, so
  an imported database or a chart someone has changed is left alone.
- **D21 Credit notes and debit notes** (2026-10-20). Table `credit_notes`
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
- **D22 Bank reconciliation** (2026-10-20). Tables `bank_reconciliations`
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
- **D23 Statements, drill-down and export** (2026-10-20).
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
- **D24 Cash flow, comparisons, sign-in messages** (2026-10-20).
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
- **D25 Any company, any country** (2026-10-20). The app is a product
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
  * The product's own name is **TallyCheck** (D44; it was "Cenango Accounting").
- **D26 Taxes for any country** (2026-10-20). No country's tax law is
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
- **D27 Tax groups and withholding tax** (2026-10-21). `tax_codes.kind` is
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
- **D31 Company setup wizard** (2026-10-20). The first-run company screen
  asks one thing per page (name, address, email, phone, website, country, tax number, logo, how
  customers pay, invoice note, then a review). Each page saves on Next and can be skipped; "Set
  up later" leaves it for this session (a sessionStorage flag) and it asks again at the next
  sign-in until a company name is saved. Administration → Company Setup reopens it. The country
  list now holds every ISO 3166 country with its ISO 4217 currency (D57 offers only Sri Lanka and the
  United States for now; the full list stays for companies that already use another); those without a preset take
  their name, currency word and symbol from the system's Intl data and a January year start.
- **D32 Journal entries on the Ledger screen** (2026-10-21). The reference had no screen for a manual
  journal entry (only the API). The Ledger page now has "+ New Journal Entry": date, description,
  reference and debit / credit lines, posted only once balanced, plus "Reverse this entry" for manual
  entries (document entries are still undone from their document, D18). A made-up demo company is
  typed in through the screens by `tests/ui/demo-data.mjs` into its own data folder
  (`docs/demo/README.md`, `scripts/run-demo.cmd`).
- **D33 Payable accounts and account edits** (2026-10-21). A bill may be owed on any
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
- **D37 Import from file** (2026-10-22, integrations plan phase 1, approved by the project owner).
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
  `b78e032` and `082082a`; ported to this edition). `payroll::us` works out one pay period:
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
  unchanged. Choices made by the web edition without a spec (company-level state and pay frequency, no
  W-4 history, no dollar rounding) are kept so both editions agree.

- **D43 US pay runs, state income tax and form figures** (2026-10-27, ported from the web Rust
  edition's waves U3 `e1f403d`, U4 `450e073`, U5a `d62293d` and U5b `fa0c09c`; ported to this
  edition). `payroll::us_runs`: runs (`USPR-0001`, DRAFT → POSTED → PAID, VOID), one line per
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
  * **Accounts are chosen, never taken over** (the chosen option): each of the 11 roles
    (`us_runs::ROLES`, defaults 6100, 6130, 2200, 2300–2370) is a setting `payroll.us.account.<role>`
    (account id; PATCH `/payroll/us/settings` `accounts: {role: id | null}`). An unset role takes the
    account with its default code if it has the default name and type, or adds it if the code is
    free; when the code is some other account (e.g. a company's own 2300 Sales Tax Payable) switching
    to US is refused: `Account 2300 is already "Sales Tax Payable". Choose the account for Federal
    income tax withheld under US payroll settings`. A chosen account must be active, of the right
    type (liability / expense) and in the home currency. A posted run keeps the accounts it used
    (`accountsSnapshot`), so paying it and its deposits clear those even after the settings change.
  * US payroll needs the company currency to be USD (switching and new runs are refused otherwise).
  The web edition's own choices are kept: one company SUTA state and pay frequency, 2,080 standard
  hours, overtime 1.5× for hourly only, one line per employee per run, no W-4 / state rule history
  (runs keep snapshots), only 2026 federal tables, local taxes not covered.
- Stripe: a publishable key (`pk_…`) is now refused with "That is a publishable key; use a secret or
  restricted key" (the general key check ran first and made that message unreachable; fixed in the
  web editions).

- **D44 Renamed to TallyCheck** (2026-10-28). Window title, installer and
  Windows app name (`productName`), About, sign-in, messages, MCP texts, plugin descriptions, docs
  and the logo (`frontend/public` icons, `Logo_tallycheck.png` wordmark, `src-tauri/icons/icon.ico`
  from the product design). Kept on purpose so existing installations keep working: the data folder
  the data folder (see D46), the identifier `lk.cenango.accounting`, `cenango-mcp.exe`,
  `CENANGO_API_KEY` and the plugin id `cenango-accounting`. The publisher and copyright holder stay
  Cenango (Pvt) Ltd (D54). The installer installs to `%LOCALAPPDATA%\TallyCheck`, so a PC with
  the old "Cenango Accounting" uninstalls that first (its data is kept).

- **D45 In-app updates** (2026-10-30). Tauri's updater
  (`tauri-plugin-updater`) runs in the desktop shell (`src-tauri/src/updates.rs`) and reaches the
  page only through the server: `GET /api/system/update` → `{current, supported, available
  {version, notes, date} | null, error}`, `POST /api/system/update/install` → 202 once the update is
  downloaded and its signature verified; the shell then closes the server and database and hands
  over to the installer (passive mode), which opens the new version. Both Administrator + loopback
  only (403 "Updates are installed by an Administrator on the main PC"), like backup; a server
  without the shell answers `supported: false` / 400. The page keeps no Tauri IPC. Screens: status
  bar **Update to x.y.z** (checked once when an Administrator opens the app, silent when offline),
  Help → Check for updates…, an update window with the notes. Updates are minisign-signed with
  Cenango's update key; the public key is in `tauri.conf.json`, the private key and password live
  outside the project (`%USERPROFILE%\.tallycheck-signing\`) and must go to the company password
  manager. Release: `python tools/release.py` (docs/installer-and-updates.md). The feed address
  (`UPDATE_FEED`) is the latest GitHub release's `latest.json` of the public repository
  https://github.com/Sammysam/tallycheck (pushed 2026-10-30, remote `github`; Bitbucket stays `origin`), which is also
  the source link shown in Help → About; `ACCOUNTING_UPDATE_URL` overrides it.

- **D46 Data folder `TallyCheckData`** (2026-10-30). The default data folder is
  `%LOCALAPPDATA%\TallyCheckData` — not `TallyCheck`, which is the installer's program folder (an
  uninstall or update would take the books with it). When only the older `%LOCALAPPDATA%\CenangoAccounting` exists, it is
  used where it is — never moved, copied or migrated (`app_core::config::choose_data_dir`).
  `ACCOUNTING_APP_DIR_NAME` set at *build* time replaces both names, for package tests only, so a
  test install can never open real books. The identifier `lk.cenango.accounting`,
  `cenango-mcp.exe`, `CENANGO_API_KEY` and the plugin id stay (they are not shown to users;
  changing them would break updates and Claude connections of existing installations).

- **D47 Microsoft Store (MSIX) build** (2026-10-30; company account, first
  Store version 1.0.0). `python tools/build_msix.py` builds `accounting-desktop --features store`
  (no in-app updater; `/api/system/update` answers `channel: "store"` and install is refused with
  "This copy of TallyCheck is updated by the Microsoft Store") and cenango-mcp in
  `target/msix-build`, lays out `TallyCheck.exe`, `frontend/`, license files and tiles, fills
  `src-tauri/msix/AppxManifest.xml` with the Partner Center identity and packs it with the Windows
  SDK's makeappx. The Store signs MSIX packages itself, so no certificate is bought. The books
  live outside AppData so they survive updates and uninstalling (D55). `--test` builds a
  package with a made-up identity and its own data folder `TallyCheckStoreTest` (compiled in via
  `ACCOUNTING_APP_DIR_NAME`). The normal installer is unchanged. Submission: `docs/microsoft-store.md`. GitHub builds it
  too (`.github/workflows/msix.yml`, Run workflow or tag; the identity is in the workflow, no secrets).

- **D55 Store data folder outside AppData** (2026-11-13). The Store refused the restricted
  capability `unvirtualizedResources` that excluded the D46 data folders from MSIX virtualization.
  The Store build (`ACCOUNTING_STORE_EDITION=1`, set by `tools/build_msix.py`) keeps the books in
  `%USERPROFILE%\TallyCheckData`, which MSIX does not redirect, so they survive updates and
  uninstalling; the package asks only for `runFullTrust`. The direct installer keeps
  `%LOCALAPPDATA%\TallyCheckData`; moving between the two goes through Backup & restore.

- **D48 Free code signing via SignPath Foundation** (replaced by D77) (2026-10-30).
  `.github/workflows/release.yml` (tag `v<version>` or run by hand): builds the NSIS installer on
  GitHub, sends it to SignPath (project `tallycheck`, artifact configuration `installer` =
  `.signpath/artifact-configuration.xml`, policy `release-signing`, manual approval), then makes the
  update signature from the *signed* file, writes `latest.json` (`tools/release.py --skip-build`)
  and creates a draft GitHub release for a person to publish. `CODE_SIGNING_POLICY.md` is the
  policy page SignPath requires. Possible only because the license is MIT OR Apache-2.0 without
  commercial dual licensing. Manual steps and conditions: `docs/signpath.md`.

- **D49 Forgotten passwords** (2026-11-03; the app is offline, so there is
  no e-mail reset). Layer 1, **recovery key**: `recovery_keys` (one per user, SHA-256 of the
  normalised key). A key is `TCRK-` + 20 Crockford base32 characters (100 random bits); case,
  spaces, dashes and O/I/L typed for 0/1 don't matter. The first administrator gets one from
  `POST /api/auth/first-run` (`recoveryKey` in the answer), shown on a "Save your recovery key"
  screen (copy / save as file / print, tick box before Continue). `POST /api/auth/recovery/key`
  (public, 200) `{username, recoveryKey, password}` sets the password, uses the key up and answers
  the new one; wrong username and wrong key get one message ("That username and recovery key don't
  match…"); failures are throttled per username in their own `recovery_throttle` (5 / 15 min, 429)
  and audited ("Failed password recovery" / "Reset the password with the recovery key"). Signed-in
  users: `GET /api/auth/recovery-key` → `{hasKey, createdAt}`, `POST` `{password}` (their current
  password; never through a Claude key) → 201 a new key replacing the old (File → Password
  recovery key…, `/recovery-key`). Layer 3, **Windows sign-in**: `POST /api/auth/recovery/windows`
  `{username, password}` only from loopback and only with the desktop shell's
  `local_verify::WindowsVerifier` (`src-tauri/src/windows_verify.rs`): Windows Hello
  (`UserConsentVerifier`, over the TallyCheck window) or, without Hello, Windows' own password
  window checked with `LogonUserW` and accepted only for the SID that runs TallyCheck (that Windows
  account can already open the data folder). Then the password is set and a new key issued;
  audited "Reset the password with Windows sign-in". `GET /api/auth/recovery/options` →
  `{recoveryKey: true, windows}`. Sign-in screen: "Forgot password?" → `/forgot-password`.
  Existing installations make their key under File → Password recovery key…. Sessions already
  signed in are not ended by a reset (JWTs can't be revoked yet).

- **D56 Forgotten usernames** (2026-11-14). The sign-in screen has "Forgot username?" next to the
  username (`/forgot-password?what=username`, a Password / Username choice on the recovery page).
  `POST /api/auth/recovery/username/key` `{recoveryKey}` (public, 200) → `{username}`: only the
  username that key belongs to, for an active user; the key is **not** used up (it already allows
  a password reset, so showing its own username adds nothing); one message for every failure
  ("That recovery key doesn't match any user…"); failures throttled per side (this PC / other
  PCs) in the recovery throttle; audited "Looked up the username with the recovery key" / "Failed
  username lookup". `POST /api/auth/recovery/username/windows` (public, 200) → `{users:
  [{username, role}]}`: only from this PC with the desktop shell's Windows verification, like D49;
  lists the active users' names and roles only, since that Windows account can already open the
  data folder; audited "Listed the usernames with Windows sign-in". No route reveals whether a
  given username exists (GDPR data minimisation; no username enumeration). "Sign in as this user"
  fills in the sign-in form.

- **D57 Countries with their rules only** (2026-11-18). Companies choose only countries
  whose country rules are built: **Sri Lanka** and the **United States** (`OFFERED_COUNTRY_CODES` in
  `frontend/src/lib/countries.ts`, shown in the setup wizard and Settings → Region & currency, with
  the note that more countries will follow). An existing company with another country keeps it,
  listed as "(current)"; the server still accepts any ISO country code, so the API, imports and
  backups are unchanged. **Payroll follows the company's country**: saving Sri Lanka sets
  `payroll.country` = LK (EPF / ETF / PAYE), saving United States sets US (US payroll; the country
  preset also sets USD); `syncPayrollCountry` in `lib/usPayroll.ts` calls
  `PATCH /payroll/us/settings` and, if the switch is refused (e.g. a US payroll account already used
  for something else), says so while the company details stay saved. The payroll country is no
  longer chosen on the US payroll screen; it shows the country and where it comes from. Other
  countries are added to the list once their payroll rules exist.

- **D58 Mac App Store build** (2026-11-18, in progress, not released). A sandboxed build for the
  Mac App Store next to the website `.dmg` (D53), which stays unsandboxed and unchanged:
  `tools/build_macos_appstore.sh` builds with `--features store` (no in-app updater) and
  `src-tauri/tauri.macos-appstore.json` (app bundle only, entitlements
  `src-tauri/macos/appstore.entitlements`: App Sandbox, network server and client, user-selected
  files read-write, app-scoped bookmarks; the script adds the application / team identifiers
  from `APPLE_TEAM_ID` and embeds the provisioning profile), signs with Apple Distribution and
  packages a `.pkg` with the Mac Installer Distribution certificate. The Claude connector is not
  bundled yet. Open items before submitting (docs/macos-app-store.md): a picked backup folder kept
  as a security-scoped bookmark, the connector, books moved from the website version with Backup &
  restore (container data folder and its own Keychain item), LAN mode, and a full test with the
  sandbox on.

- **D59 Moving books in from another system** (2026-11-20; the treatment of unpaid invoices and
  bills was chosen explicitly: each one as a document, cleared through 3900). An Administrator
  brings a company's books over from QuickBooks, Xero, Zoho, Sage, TallyPrime or any spreadsheet
  on a cut-over date (Admin → File → Move books in from another system…, `/move-books`; Import
  from File links to it). The screen reads the old system's exports with `/imports/read`:
  trial balance (needed), chart of accounts, unpaid invoices, unpaid bills, customers, vendors.
  It understands report layouts (title lines above the headings, a blank first heading, names on
  their own row above their documents, "Total" rows and footers; `lib/conversionFiles.ts`), and
  the CSV reader now picks the delimiter from the first 20 lines instead of the first one. Old
  accounts are matched to TallyCheck accounts or created (receivables to 1100, payables to 2000).
  `POST /api/conversions/preview` runs everything and rolls back; `POST /api/conversions` saves it
  in one transaction, only when nothing is wrong:
  * each unpaid invoice / bill becomes a POSTED document dated as in the old system (old number as
    the invoice's PO number / in the bill's notes), one line on **3900 Opening Balance Equity**
    (equity, created when first needed), so it posts Dr 1100 / Cr 3900 or Dr 3900 / Cr 2000 and
    can be paid, chased and aged here;
  * the trial balance becomes one opening entry on the cut-over date (source `CONVERSION`), its
    1100 and 2000 lines posted to 3900 instead, so 3900 nets to zero when the open documents add
    up to the trial balance; a trial balance that doesn't balance is refused, and a difference
    between the documents and the receivables / payables must be accepted explicitly
    (`acceptDifferences`; it stays in 3900);
  * one conversion at a time; documents dated after the cut-over, credit balances, unknown
    accounts and lock-date refusals are reported per row and nothing is saved.
  Tables `conversions` and `conversion_items`. `GET /api/conversions` lists them;
  `POST /api/conversions/{id}/reverse` voids the documents and reverses the opening entry dated
  today (refused while a document has payments or notes; accounts, customers and vendors stay).
  The opening entry can't be reversed from the Ledger (D18). Administrators only, closed to
  **Two ways to start** (added 2026-11-20): *Quick start* needs no files: the person types bank and
  cash balances, what each customer owes, what is owed to each supplier, other assets and loans
  (`components/QuickStartForm.tsx`); each becomes an account (matched by name or created) and a
  trial-balance line, customer / supplier amounts become one opening invoice / bill each, and the
  difference (what the business is worth) goes to 3900 Opening Balance Equity for the accountant
  to move to capital or retained earnings. *From files* also takes a balance list instead of an
  invoice list (one amount per customer / vendor, e.g. QuickBooks' A/R or A/P Ageing Summary, the
  name under a blank first heading): an open document without a date is dated the cut-over day.
  Claude keys, previews not written to the activity log ("Moved books in from another system",
  "Reversed a book conversion"). Not yet: full history from the general ledger, reading
  QuickBooks Desktop IIF and TallyPrime XML directly, foreign-currency open documents, tax on
  open documents (they come in as one untaxed amount).

- **D60 Estimates / quotes** (2026-11-25, roadmap release 1.2 "Everyday selling"; converting
  makes a DRAFT invoice, as chosen). Tables `estimates` (`estimateNumber` EST-0001, customer or
  Bill To name, date, `expiryDate` "valid until", status DRAFT / SENT / ACCEPTED / DECLINED /
  CONVERTED, print details and payment terms as on invoices, totals, tax, currency and rate,
  `invoiceId` once converted) and `estimate_lines` (as invoice lines). An estimate is priced
  exactly as an invoice (`invoices::build_lines`: discounts, tax codes and groups, prices
  including tax, currency) but **posts nothing**. Statuses are set by hand (CONVERTED only by
  converting); it can be edited while DRAFT or SENT. `POST /api/estimates/{id}/convert` makes a
  DRAFT invoice dated today with the same customer, lines, discounts, tax codes, currency and
  rate, notes, payment terms (the invoice's due date follows them) and print details, and marks
  the estimate CONVERTED with the invoice's id, in one transaction; once only, never from
  DECLINED. A converted estimate can't be edited or deleted; others can be deleted. Tax on the
  invoice is worked out again with the tax codes as they are on the day of converting. Routes
  (permission `invoices`, same visibility as invoices: Admin / `invoices:all` see all, others
  their own): `GET|POST /api/estimates`, `GET|PUT|DELETE /api/estimates/{id}`,
  `PATCH /api/estimates/{id}/status`, `POST /api/estimates/{id}/convert`. Audited as "Created /
  Updated / Deleted an estimate", "Changed an estimate's status", "Turned an estimate into an
  invoice". Screen: Transactions → Estimates (the invoice sheet in estimate mode: no PO number
  or due date, "Valid until"; printed and saved as PDF titled "Estimate"). Not yet: e-mailing
  an estimate, accepting it online, partial conversion.
- **D61 Recurring invoices** (2026-11-25, roadmap release 1.2; chosen: each date makes a
  draft to check, missed dates are made one by one, schedules are made from an invoice).
  Tables `recurring_invoices` (name, the source invoice, a JSON copy of it as the template:
  customer / Bill To, lines with discounts and tax codes, currency and rate, payment terms or
  the days to the due date, notes and print details; frequency WEEKLY / MONTHLY / QUARTERLY /
  YEARLY every 1–99, start date, optional end date, `nextDate` = date number `dateIndex` counted
  from the start date so months never drift (31 Jan, 28 Feb, 31 Mar), status ACTIVE / PAUSED /
  ENDED, `lastError`) and `recurring_invoice_items` (schedule + date → the invoice made; primary
  key on schedule and date, so a date is never made twice). `operations::recurring::run_due`
  runs a few seconds after start-up, every hour, and right after a schedule is made, changed or
  resumed: for every active schedule whose next date has come it makes a **DRAFT** invoice per
  date (`invoices::create_in`, dated that date, made as the schedule's owner), dates missed
  while the app was closed included, each with its own date (at most 60 per schedule per run);
  one transaction per schedule; a schedule that fails (say the customer was deleted) keeps its
  date, shows the reason and is tried again; the others go on. Nothing is posted. Activity log:
  "Made N recurring invoices for …" / "Could not make the recurring invoice …" (user "system"),
  plus "Made an invoice recurring", "Changed / Paused / Resumed / Stopped a recurring invoice".
  Pausing skips dates (resuming continues from today); changing the schedule recounts the next
  date from today without remaking made dates; stopping deletes the schedule and keeps its
  invoices. A foreign-currency template keeps the rate it was copied with (the draft can be
  corrected before posting). Routes (permission `invoices`, visibility as invoices):
  `GET|POST /api/recurring-invoices`, `GET /api/recurring-invoices/waiting` (drafts made by
  schedules still to post), `PATCH|DELETE /api/recurring-invoices/{id}`, `POST …/{id}/pause`,
  `POST …/{id}/resume`. Screens: "Make recurring" in an invoice's ⋮ menu, a notice on Invoices
  while made drafts wait, Transactions → Recurring Invoices. Not yet: recurring bills and
  expenses, posting automatically, e-mailing them.
- **D62 Automatic backups every N days and Choose folder** (2026-11-25; chosen: build both, and
  "clear" means an easy off switch with a warning).
  The automatic backup setting gains `everyDays` (1–30, default 1; settings saved earlier mean
  daily): while the app runs it checks every hour and makes a full backup when the last
  successful one is older than that many days (soon after start-up if one is overdue); copies
  beyond "keep" are still deleted, manual backups never. `GET /api/system/backup` adds
  `nextBackupDue` (ISO time, "now" when overdue, null when off) and `canChooseFolder`;
  `PUT /api/system/backup/settings` takes `everyDays`. **Choose folder…**: `POST
  /api/system/backup/choose-folder {start?}` (Administrator on this PC, like every backup
  route) asks the desktop shell to open the system's own folder window over TallyCheck
  (`api_server::folder_pick::FolderPicker`, implemented in `src-tauri/src/folder_pick.rs` with
  `tauri-plugin-dialog`; the page still has no Tauri IPC) and answers `{folder}` or `null`; a
  network folder is refused as when typed; without the desktop shell (a browser, the headless
  server) it answers 400 and the screen offers typing only. Settings → Backup: an On / Off
  switch, "Back up every __ days", the folder with Choose folder… (also for Back up now), and a
  warning while automatic backups are off ("Automatic backups are off — your books are not
  being backed up automatically"); the Admin Centre backup card warns too.
- **D63 US payroll in several states** (2026-11-26, step 0 of the US state payroll plan).
  Tables `us_state_registrations` (per state: withholding and unemployment account numbers, the
  SUTA rate the state assigned and its wage base, optional payable accounts for that state's
  income tax and unemployment) and `us_state_reciprocity` (work state + home state pairs whose
  residents pay only the home state; entered by the company, no pairs ship yet); state rules gain
  `residentCredit` NONE | DIFFERENCE (as the home state, also withhold its tax less the work
  state's when that is more); employees gain `residentState` (NULL = lives in the work state) and
  `otherWorkStates` (`[[state, share]]`, shares in hundredths of a percent; the work state has
  the rest). `payroll::us_states::split` works out each state's part of a paycheck: a share is
  taxed by the work state — its rule on the whole paycheck × the share — unless the employee
  lives there or the pair has reciprocity (then the home state taxes it); a DIFFERENCE home state
  adds its tax less the work state's on shares taxed elsewhere; the certificate's extra
  withholding is added once, to the main work state; one state at 100% gives the single-state
  result exactly. Unemployment goes to the main work state: its registration's rate and wage
  base, year-to-date unemployment wages counted per state; a work state without a registration
  keeps the company-wide settings (as before). Pay lines keep `residentState`, `workSplit`,
  `stateRules` (every rule used), `stateTaxes` (state, WORK / RESIDENT, share, wages, tax) and
  `sutaState` / `sutaRateUsed` / `sutaWageBaseUsed`; lines made earlier are read as one work
  state. Posting credits state income tax and unemployment per state, to the registration's
  account when it names one (kept as `stateIncomeTax:CA` / `suta:CA` in the run's accounts);
  posting needs the rules of every state a line uses (its home state too). A state income tax
  or unemployment payment is for one state (`us_payroll_remittances.state`); without a state
  and amount it clears every state, one part each; an amount with several states owed needs the
  state. W-2: `states` lists boxes 15–17 per state (box fields show the work state). Routes
  (`payroll`): `GET /payroll/us/states`, `PUT|DELETE /payroll/us/states/{state}`, `POST
  /payroll/us/reciprocity`, `DELETE /payroll/us/reciprocity/{work}/{resident}`; the employee
  record, the preview (`stateTaxes`, `unemploymentState`), the run lines and the remit body
  (`state`) carry the new fields. Screens: US payroll settings → "States you are registered in"
  (registrations and reciprocity), state rules (home-state difference), employee US tax section
  (lives in, also works in), pay run (taxes and payments per state). Not yet: built-in state
  rates and reciprocity pairs (steps 1–3), local taxes, disability / paid leave, state forms.
- **D64 Built-in 2026 state rules, flat-rate states** (2026-11-26, step 1 of the US state payroll plan).
  `payroll::us_state_packs` holds built-in 2026 withholding rules copied from each state's own
  2026 employer publication (named in each rule's `source`, shown on screen): Arizona (A-4:
  the employee elects 0–3.5% of wages, 2.0% without an A-4; method EMPLOYEE_RATE, built in
  only), Colorado (DR 1098: 4.40%, $5,500 / $11,000 unless DR 0004 line 2 gives an amount),
  Georgia (Employer's Guide 2026: 5.19% for pay dates before 11 May 2026, then 4.99%; $15,000 /
  $30,000; $5,000 per allowance), Idaho (EPB00744 07-23-2026: 5.3% over $16,100 / $32,200, whole
  dollars; from 23 July 2026 — the January–July table is not built in), Illinois (IL-700-T:
  4.95%; $2,925 per line 1 and $1,000 per line 2 allowance), Indiana (Departmental Notice #1:
  2.95%; $1,000 per personal and $1,500 per dependent exemption; county tax not included), Iowa
  (2026 formula: 3.8%; $13,000 / $26,000 / $19,500 head of household; IA W-4 line 7 dollars as a
  credit, $40 per allowance on a 2023 or earlier IA W-4), Kentucky (42A003: 3.5% after $3,360),
  Louisiana (R-1306: 3.09%; $12,875 / $25,750, head of household as married), Michigan (Form 446:
  4.25%; $5,900 per exemption), North Carolina (NC-30: 4.09%; $12,750 / $19,125 head of
  household; $2,500 per allowance; whole dollars) and Pennsylvania (REV-415: 3.07%; 401(k)
  deferrals taxable). Each rule applies from its `from` pay date to the next one for the state.
  `load_rule(state, pay date)`: no income tax → NONE; the company's own rule when it entered one
  (it always wins); else the built-in rule in force on the pay date; else none (posting waits).
  Rules gain `rounding` CENT / DOLLAR, `standardDeductionHeadOfHousehold` and a second
  allowance type (`secondAllowanceDeduction` / `Credit`); the employee's state certificate gains
  head of household, second allowances, `stateDeductionOverride` (replaces the standard
  deduction), `stateAnnualCredit` (dollars a year off the tax) and `stateElectedRate`. Each
  state's worked examples from its publication are unit tests. `GET /payroll/us/state-taxes`
  gives each state's `kind` (NO_TAX / BUILT_IN with `source` and `from` / COMPANY with
  `builtInAvailable` / MISSING). Not built in yet: Ohio and Mississippi (brackets, step 2),
  Utah and Massachusetts (credits and surtax, step 3), the bracket and special-formula states.
  Known limits: Pennsylvania taxes some section 125 amounts (other than health, sickness,
  disability or death plans) that the app takes off state wages; Louisiana's "no deduction" L-4
  choice and negative adjustments are not offered.
- **D65 Built-in 2026 state rules, bracket states** (2026-11-27, step 2 of the US state payroll plan).
  Built-in 2026 bracket rules from each state's own publication (`source` on each rule):
  Delaware ($3,250 / $6,500; $110 credit per allowance; 2025 table, unchanged), Hawaii (Booklet A:
  $4,350 lump sum; $1,144 per allowance; married brackets doubled), Kansas (KW-100: $3,605 /
  $8,240; K-4 personal allowances $9,160, dependents / head of household $2,320), Minnesota
  ($5,300 per allowance), Mississippi (0% to $10,000, 4% above; $2,300 / $4,600 / $3,400; Form
  89-350 exemption dollars; whole dollars), Missouri ($16,100 / $32,200 / $24,150; whole
  dollars), Montana (own head-of-household brackets; whole dollars), Nebraska ($2,440 per
  allowance), New Mexico (head of household: $12,075 with the married brackets), North Dakota
  (own head-of-household table; $5,050 per allowance on a pre-2020 W-4; whole dollars), Ohio
  (from 1 August 2026 only; $650 per exemption), Oklahoma ($1,000 per allowance; whole dollars),
  Rhode Island ($1,000 per exemption, none above $290,800 of annual wages), Vermont ($3,925 /
  $11,775; $5,400 per allowance), Virginia ($8,750; $930 / $800 exemptions; yearly tax rounded to
  the dollar before dividing) and West Virginia (IT-104 two-earner table = single, one-earner =
  married; $2,000 per exemption; whole dollars; the SB 392 rate cut applied from 1 April 2026,
  the state gave no start date). The rule engine gains head-of-household brackets, an
  allowance wage limit and yearly dollar rounding (built-in rules only); the state certificate
  gains `stateAnnualDeduction` (exemption dollars). Each state's worked examples are unit tests
  (Montana's married example and Vermont's, Kansas' and New Mexico's per-period table examples
  differ from the exact annual formula by a cent or a dollar, as the guides note). Not built in:
  Alabama (federal tax deduction, phased standard deduction), Arkansas (stepped table), Maine,
  Wisconsin (phased deductions), Connecticut (codes, phase-outs, credit), New Jersey (rate tables
  A–E), Oregon (federal tax subtraction) — step 3 with Utah, Massachusetts, California, New York
  and Maryland; South Carolina (a March 2026 law changed the rates; no new withholding formula
  yet) and the District of Columbia (no 2026 withholding tables) wait for the states to publish.
- **D66 State formulas** (2026-11-27, step 3 of the US state payroll plan).
  States whose method is not one rate or one bracket table get their own formula in
  `payroll::us_state_formulas` (method FORMULA, built in only; the rule names the formula, e.g.
  `AL-2026`), worked out as each state's own 2026 publication writes it: Alabama (standard
  deduction phased out by income, the annual federal income tax withheld subtracted, A4 status
  0 / S / MS / M / H and dependents), Arkansas (revised 3.7% formula, $100 midrange table,
  annual tax rounded to the dollar, $29 per exemption; low-income tables not offered),
  California (Method B annualized: low-income exemption, estimated-deduction allowances,
  standard deduction, annual rate tables, $168.30 credit per regular allowance), Connecticut
  (TPG-211 Tables A–E by CT-W4 code; no code on file = 6.99%), Maine (revised August 2026:
  phased standard deduction, 2% surcharge, whole dollars), Maryland ($3,400 deduction, $3,200
  per exemption, state rates from 4.75% plus the county rate of the chosen county — chart
  rates as the guide directs, Anne Arundel and Frederick progressive; no county = 3.30%,
  nonresident 2.25%), New Jersey (rate tables A–E per pay period, as printed; NJ-W4 rate letter
  or A / B by filing status), New York State (Method II per-period tables as printed and Method
  III; New York City and Yonkers not included) and Wisconsin (W-166 alternate method, deduction
  phased out by wages). The state formulas see the paycheck's federal income tax and employee
  social security / Medicare (`PayContext`). The state certificate gains `stateCode` (the
  form's status code, rate letter or Maryland county). Each state's published examples are
  unit tests; where a state's examples come from rounded per-period tables (California C and D,
  New York's two examples printed one cent off half-up, New Jersey's examples from an older
  table, Connecticut's wage-bracket tables worked on row midpoints) the tests say so. Held:
  Oregon (its 2026 formula and worked example add $263 a year that its own tables do not —
  to be confirmed with the Oregon Department of Revenue), Utah and Massachusetts (their 2026
  publications could not be read; figures only from secondary sources), South Carolina and DC
  (no 2026 withholding method published). Known limits: New Jersey and Pennsylvania tax some
  section 125 amounts the app takes off state wages; Maryland's 401(k) treatment follows the
  federal one as the guide says in general terms.
- **D67 State disability and paid leave** (2026-11-27, step 4 of the US state payroll plan).
  `payroll::us_leave` builds in the 2026 state disability and paid family / medical leave
  programs from each program's own publications (`GET /payroll/us/leave-programs` lists them
  with their sources): California SDI 1.3% (no limit; 401(k) counts, section 125 does not),
  Colorado FAMLI 0.44% + 0.44% (no employer share at 9 or fewer employees), Connecticut Paid
  Leave 0.5%, DC Paid Family Leave 0.75% employer only, Delaware Paid Leave 0.4% + 0.4% (10–24
  employees parental 0.16% + 0.16%; under 10 none), Hawaii TDI 0.5% of weekly wages up to
  $1,500.21 (never more than half the insurer's premium when the company enters it), Maine PFML
  0.5% + 0.5% (under 15 employees employee only), Minnesota Paid Leave 0.44% + 0.44% (small
  employer designation 0.22% employer) to $185,000, New Jersey TDI 0.19% and FLI 0.23% to
  $171,100 plus employer TDI at the company's assigned rate (0.5% if none) to $44,800, New York
  Paid Family Leave 0.432% up to $411.91 a year and disability 0.5% up to $0.60 a week, WA Cares
  0.58% (no limit; employee exemptions) and Washington Paid Leave 1.13% (employee 71.43%, the
  premium and the employee share each rounded, employer = the difference; under 50 employees no
  employer share) to $184,500. A program applies to the state the employee mainly works in; wages
  are gross less the 401(k) / section 125 amounts the program leaves out; yearly limits count the
  employee's other posted runs this year; amounts half-up to the cent per paycheck. State
  registrations gain `employees` (for the small-employer rules; not given = a large employer),
  `leaveOptions` per program (`off` — a private plan, an employer paying the employee share, or
  New York disability not collected; `employerRate` for New Jersey TDI and Hawaii's premium;
  `small` for Minnesota) and `leaveAccountId`; employees gain `leaveExempt` (e.g. WA_CARES). Pay
  lines keep each program's parameters (`leavePlan`), the year-to-date figures counted and the
  contributions (`leave`, `leaveEmployee`, `leaveEmployer`); the employee's share comes off net
  pay and the employer's is an employer tax. Posting credits each program to the state's
  registration account or 2380 State Disability and Paid Leave Payable (added only when a run
  first has contributions), kept as `stateLeave:CA_SDI`; payments use type `STATE_LEAVE` with a
  `program` (one program, or every program when neither program nor amount is given). The
  remittances table is rebuilt to allow the new type. Not built in: Oregon, Massachusetts and
  Rhode Island (their 2026 pages could not be read directly), Maryland (contributions start in
  2027); an employer paying the employee's share is not worked out (switch the program off).
- **D68 Unemployment defaults and employer charges** (2026-11-27, step 5 of the US state payroll plan).
  `payroll::us_unemployment` holds every state's and DC's 2026 unemployment wage base,
  new-employer rate (none where the state sets it by industry) and the employer charges billed on
  the same wages besides the rate (California ETT 0.1%, Delaware operations / training 0.2% +
  0.11%, DC administrative 0.2%, Hawaii E&T 0.01%, Massachusetts EMAC and Workforce Training Fund,
  Minnesota workforce fee 0.1%, Montana administrative fund 0.18%, Nevada CEP 0.05%, South Dakota
  investment / administrative fees), each with its source; `verified` is false for the states
  whose page could only be read through a search excerpt (Michigan, Rhode Island, South Carolina,
  Texas, Utah, Virginia). `GET /payroll/us/unemployment-defaults` serves them; registering a
  state fills them in on the screen and the company replaces them with its own notice — nothing
  changes a saved registration by itself. Registrations gain `sutaSurcharges` ([{name, rate}],
  at most 10, each 0–20%), added to the unemployment rate on the same capped wages; pay lines keep
  the total (`sutaSurchargeRate`). Employee unemployment contributions are programs of kind UI in
  `payroll::us_leave`: Alaska 0.5% to $54,200, New Jersey 0.425% (UI 0.3825% + workforce
  0.0425%) to $44,800, Pennsylvania 0.07% of all wages; they come off net pay like the leave
  programs but are owed with (and paid as) that state's unemployment tax. Not covered: the
  quarterly New Hampshire rate changes and West Virginia's July 2026 fee (on the company's notice),
  Massachusetts' COVID recovery assessment (on the notice), industry-specific new-employer rates.
- **D69 Local taxes** (2026-11-27, step 6 of the US state payroll plan).
  `payroll::us_local`: each company keeps its localities (`us_localities`: code, name, state and
  the locality as JSON) and each employee lists those withheld for (`employee_us_tax.localTaxes`:
  code, lives there or not, local allowances / second allowances, extra per paycheck, exempt; at
  most 6). Methods: PERCENT (resident / nonresident rate, employer rate, yearly exemption per
  allowance taken off per paycheck half-up), MONTHLY_FLAT (employee / employer amount on the first
  pay date of a month when the month's wages reach a threshold — Colorado occupational privilege
  taxes), WEEKLY_FLAT (weekly fee × 52 ÷ pay periods — West Virginia city service fees),
  PERIOD_FLAT (yearly amount ÷ pay periods rounded down, with a yearly cap — Pennsylvania LST) and
  built-in FORMULA (New York City NYS-50-T-NYC Method II; Yonkers resident 16.75% of the New York
  State withholding and nonresident 0.50% less the Method VII exclusion; Philadelphia 3.74% /
  3.43% to 30 June 2026 then 3.735% / 3.425%; Portland Metro SHS and Multnomah PFA with the Oregon
  federal-tax deduction). Localities sharing a group withhold only the largest employee amount
  (Pennsylvania EIT: the higher of the home PSD's resident and the work PSD's nonresident rate).
  Built in to add with one click (`GET /payroll/us/localities` → `catalog`): the formulas above,
  Indiana's 92 county rates (Departmental Notice #1, R47, from 1 October 2026; Boone 1.70% before),
  22 Michigan city taxes (Detroit Form 5469; Saginaw and Springfield to be checked; Lansing and
  Muskegon Heights not found), the 10 largest Ohio cities, Louisville Metro, Lexington-Fayette and
  Fayette County schools, nine Alabama occupational taxes, six West Virginia city fees, Greenwood
  Village and Glendale (CO); Denver, Kansas City, St. Louis and the Oregon transit taxes are
  marked to be checked (their pages could only be read through search excerpts). Anything else —
  other Ohio cities and school districts, Pennsylvania EIT by PSD, Kentucky — the company enters
  (`PUT|DELETE /payroll/us/localities/{code}`, permission `payroll`; a locality in use can't be
  removed). Pay lines keep `localPlan`, `localYtd`, `local`, `localEmployee`, `localEmployer`;
  the employee's amount comes off net pay, the employer's is an employer tax; posting credits each
  locality to its own account or 2390 Local Income Tax Withheld (added when a run first has local
  tax), kept as `localTax:NYC`; payments are type `LOCAL_TAX` with a `locality`. Not worked out:
  Ohio's 20-day rule and resident credits, Michigan's resident / work-city split, Yonkers'
  allocation by days worked, nonresident apportioning (the company sets the localities to match).
- **D70 State forms** (2026-11-27, step 7 of the US state payroll plan).
  `payroll::us_reports` gains per-state form figures from POSTED / PAID runs by pay date (nothing
  is filed): `GET /payroll/us/reports/state?state&year&quarter` (permission `payroll`) — the
  withholding return (the state's income tax withheld and the wages it was withheld on, by month
  of the pay date, employees, deposits recorded for that state), the unemployment wage report
  (each employee whose unemployment went to the state: gross wages, wages over the state's limit,
  taxable wages, the employer's contributions including the registration's extra charges,
  employee unemployment contributions, the rates used, employees paid for the pay period
  including the 12th of each month, deposits), and that state's disability / paid leave programs
  and local taxes (wages, employee, employer, deposits); the registration's withholding and
  unemployment account numbers head it. `GET /payroll/us/reports/state-annual?state&year` — each
  quarter, the year's totals and the W-2s with a row for the state (count, state wages and tax),
  with `balanced` when the W-2 state tax equals the tax withheld. The W-2 figures gain `box14`
  (the employee's disability / paid leave contributions by program, e.g. California SDI) and
  `locals` (boxes 18–20 per locality). Screen: US payroll tax forms → State forms. State-specific
  form layouts and electronic filing formats are not built (the figures are what each form asks).
- **D71 Utah and Massachusetts withholding** (2026-11-27).
  Built-in from the states' own publications: Utah, Publication 14 (Rev. 4/26), for pay dates
  from 1 June 2026 (the schedules apply to pay periods beginning on or after that date) — 4.45% of
  wages less the base allowance, which is reduced by 1.3% of wages over the threshold; each line
  rounded to the dollar; married filing jointly on the married schedule, head of household and
  married filing separately on the single one; earlier 2026 pay dates use a company rule.
  Massachusetts, Circular M (Rev. 12/25), percentage method from 1 January 2026 — wages less this
  paycheck's social security / Medicare / retirement-system deductions while the year's total
  stays within $2,000, less the M-4 exemption factor, 5% and 9% above $1,107,750 (the 4% surtax),
  less the head of household and blindness values, nothing below the wage floor when an exemption
  is claimed; Circular M does not mention 401(k), so 401(k) deferrals are left out as for federal
  wages. State formulas now also see the employee's year-to-date social security and Medicare
  (`PayContext.fica_ytd`, 6.2% / 1.45% of the year-to-date wages). Oregon stays a company rule:
  its 2026 formula (150-206-436, Rev. 12-31-25) adds $263 a year under $50,000 that its own
  tables do not.
- **D72 First-time-user fixes** (2026-11-27).
  Fixes from a first-time-user test of a fresh install. When payroll becomes US, the untouched
  Sri Lankan payroll accounts the baseline seeds (2210–2250, 6110, 6120: seeded name, never
  posted to) are switched off, so they leave the reports and account lists; they come back when
  payroll returns to Sri Lanka (`payroll::us_setup::tidy_country_accounts`, also run at start-up
  for a company already on US payroll). A new company's first invoice is INV-0001 (migration
  `20261127000006` resets the reference's seeded INV-9000 counter only while no invoice exists).
  The default backup folder is `Documents\TallyCheck Backups` (earlier installs keep the folder
  they saved). Creating the first administrator counts as that person's first sign-in on the
  Users screen. Screens: a posted invoice's menu has "Receive payment" and a posted bill "Pay
  bill", which open the payment form for that document with the amount still owed; the payment
  form uses plain words, starts on today's date, fills in the amount when a document is chosen
  and lists only bank and cash accounts; the payments list says what each payment was for and
  shows types and methods in words; bill dates are labelled and the bills list shows the due date;
  invoice and bill due dates follow the company's date style; posting, voiding and recording
  payments confirm with a message; customers and vendors can be edited from their lists; the
  Balance Sheet shows one Total Assets and "Total liabilities and equity"; the company setup's
  tax-number step asks a US company for its EIN; the dashboard tells an administrator when
  automatic backups are off and shows a getting-started list until anything is recorded; the
  first-run and sign-in labels are linked to their fields.
- **D73 Easier first setup** (2026-11-27).
  The company setup wizard gains two pages before the review: "Keep your books safe" (backup
  folder with Choose folder… in the desktop app, a backup password the first time, and daily
  automatic backups on — through the existing backup routes, so it needs the administrator on the
  main PC; elsewhere the page says where to do it later) and "Do you already have a business
  running?" (a new business, or an existing one — Finish then opens Move books in → Quick start).
  The administrator's dashboard shows a Getting started checklist that ticks itself off from the
  company's data: company details, opening balances (a conversion, or the new-business answer),
  first customer, first posted invoice, automatic backups on, a second active user; each open
  step has a Start button; it can be hidden and disappears when all are done. Steps the user
  cannot check (a missing right, not on the main PC) are left out.
- **D74 Baseline without company details** (2026-11-26).
  The baseline migration (`20261014000001`) no longer seeds the reference company's own signatory
  and payroll details (names, EPF / ETF number, addresses, phone); those settings start blank, as
  every fresh install already ended up after `20261020000004`. The contract test uses a made-up
  phone number. This is the one edit to an applied migration: `Db::open` replaces the recorded
  checksum of the baseline only when it is exactly the old file's SHA-384, so installations that
  applied the old file keep opening (saved settings are untouched); any other mismatch is still
  refused. Older commits in the history still contain the old file.
- **D75 Second first-time-user fixes** (2026-11-26).
  Fixes from the second first-time-user test. Taxes: a new tax code no longer guesses a payroll or
  income-tax account (e.g. 2300 Federal Income Tax Withheld) as its sales / purchase account; the
  accounts it offers to add are 2400 Tax Payable (Sales Tax Payable for a US company) and 1400 Tax
  Receivable, withholding 1450 / 2450 — outside 2300–2390, which US payroll uses — with US examples
  for a US company. Backups: `PUT /system/backup/settings` that switches automatic backups on makes
  the first copy at once when one is due and answers `firstBackup` (or `firstBackupError`); the
  wizard says where it went, and the checklist ticks backups only once a copy exists. Products: a
  new product is "Goods, not counted" unless stock counting is chosen; a post refused for short
  stock opens the app's own window with "Stop counting <product> and post" (invoice and bill
  actions no longer use browser pop-ups). Opening balances: an open invoice or bill brought in
  without a due date is due 30 days after its date, so it is not overdue the day it comes in; the
  checklist does not count the customers and invoices a conversion brought in; Quick start shows
  its check in its own words (owns / owes / worth, customers owe you, you owe suppliers), country
  examples, dates in the company style and a "Go to my dashboard" button. The wizard's review page
  wraps long values and keeps its Change buttons visible.
- **D76 Third first-time-user fixes** (2026-11-26).
  Fixes from the third first-time-user test. Backups are named `TallyCheck-backup-<stamp>.qbak`
  (copies named `cenango-accounting-…` by earlier versions are still recognised and pruned, oldest
  first by their timestamp). The setup wizard shows where the first backup was saved on the page
  itself (pop-up messages are not shown on that screen; a payroll-country problem is shown there
  too) and says that automatic copies go into a "scheduled" folder. Account pickers offer the
  usual accounts first (`lib/accountChoices.ts`): bill lines expenses other than payroll, with
  "Show all accounts" and "+ New expense account…" (added as an expense at the first free code
  from 6200); a bill's payable account liabilities other than payroll; a product's purchase
  account expenses and assets other than money and receivables; tax codes no payroll, money or
  receivable accounts. The account already chosen is always kept. A new bill starts on today's
  date and its line boxes are labelled; the customer and vendor forms have a heading and visible
  labels; the review page wraps at word boundaries; the stock window's button reads "Stop counting
  it and post".
- **D77 No SignPath** (requested 2026-11-27: SignPath cannot be used). The release workflow
  no longer sends the installer to SignPath: it builds the NSIS installer, makes the update
  signature (D45) and `latest.json`, and creates the draft GitHub release. `.signpath/`,
  `CODE_SIGNING_POLICY.md` and `docs/signpath.md` are removed. The direct installer is unsigned
  (SmartScreen warns); the Microsoft Store package is signed by the Store (D47).
- **D78 Store updates and "What's new"** (requested 2026-11-27). The Microsoft Store build asks
  the Store whether an update is waiting (`src-tauri/src/store_updates.rs`, `StoreContext`), behind
  the same `/api/system/update` routes as D45 (Administrator on the main PC): the status bar shows
  **Update to x.y.z** and Help → Check for updates offers it; installing lets the Store download it
  (its own window may ask to confirm), closes the server and database, has the Store install it and
  opens the new version. Only the Store is contacted, and offline the check stays silent. The Mac App
  Store build has no such call and says that the App Store updates it. The status bar shows the
  version (`v1.1.1`), which opens **What's new**; the first start after an update on that PC shows
  it once (`frontend/src/lib/whatsNew.ts`, written in the app so it works offline; add each
  release's points there).
- **D79 US sales tax on purchases is part of the cost** (requested 2026-11-27 after a US CPA's
  review). A standard tax code says whether tax paid on purchases can be claimed back
  (`tax_codes."purchaseTaxRecoverable"`, default yes: VAT / GST). When it can't (US sales tax), a
  bill line's, expense's or debit note's tax posts with that document's own account (and a stock
  item's cost), no purchase tax account is needed, and the tax return lists it as "tax paid in
  costs" (`taxInCosts`) without subtracting it from the tax owed. Each stored split keeps its
  treatment (`document_taxes."recoverable"`), so changing a code never rewrites saved documents.
  New codes default to not recoverable for a US company; migration `20261127000007` switches a US
  company's existing standard codes (documents already saved keep how they were posted).
- **D80 Seeing the ledger and posting to it are separate rights** (same review: the built-in
  Viewer role could post and reverse journal entries). New permission `ledger:post`: posting and
  reversing manual journal entries, bank reconciliation (start, tick, complete, undo, discard),
  stock adjustments, revaluations, bank-line transfers and the opening-balances import. `ledger`
  alone reads the ledger, transaction details and account activity. Migration `20261127000008`
  gives `ledger:post` to every role that had `ledger`, except Viewer. The screens hide those
  actions without it.
- **D81 Other fixes from the US CPA review** (2026-11-27). Credit / debit notes start with a tax
  code only when every line of the document has that code. Quick start numbers new accounts by
  kind with a subtype (bank 10xx, stock 12xx, deposits 14xx, other assets 15xx `fixed_asset`,
  cards / overdrafts 21xx, loans 27xx `long_term_liability`; conversions accept `subtype`), and
  the account form offers subtypes as a list. The cash flow report treats balances brought in
  (source CONVERSION) as opening cash (`broughtIn`), never as movements, and its default cash
  accounts leave out 10xx accounts whose subtype says otherwise and add any `bank` account. The
  trial balance shows each account's net balance as of a date and can hide empty accounts; the
  balance sheet puts the profit to date inside total equity and hides empty accounts unless asked.
  A new bill is due 30 days after its date unless changed. A US company's employee form leaves out
  EPF / NIC and calls contract labour "Contractor (not on payroll)". "Paid from" on expenses and
  pay runs and the reconciliation account list offer bank and cash accounts only. Smaller: the
  sign-in tagline, "Invoiced" on converted estimates, statement dates in the company style, and
  unknown addresses such as /invoices/new open their list. Not built (features, not fixes):
  payroll year-to-date figures from before TallyCheck, 1099s, cash-basis reports, sales tax by
  jurisdiction, check / vendor invoice numbers, SSN and address on employees.

- **D50 Security review fixes** (full review 2026-11-03; all findings fixed
  after review). Sign-in: the throttle counts an attempt *before* the password check
  (`LoginThrottle::begin`, so parallel guesses stop at 5), keys are per username **and** per side
  (this PC / other PCs: a LAN client cannot lock the owner out), at most 10,000 keys (fewest
  attempts forgotten first), usernames over 255 characters refused unlogged. An administrator's
  password reset and switching a user off cancel the user's recovery key. Windows recovery: one
  prompt at a time; whether the username exists is said only after Windows confirms. "This PC"
  (first run, Windows recovery, backup / restore, updates, Claude keys) = loopback peer **and** a
  loopback address-bar host (`extract::from_this_pc`); a key without a recorded peer is refused.
  LAN host names: exactly `PC`, `PC.local`, `PC.<USERDNSDOMAIN>` or configured names (was any
  `PC.*`). Every response has a Content-Security-Policy (`middleware::CONTENT_SECURITY_POLICY`: no
  inline or eval scripts; inline styles, data:/blob: images and blob: PDFs allowed); the printed
  documents attach their buttons with addEventListener and escape every value, the logo included;
  the server accepts only a plain base64 PNG/JPEG/WebP data URL as logo. The window navigates only
  within the server origin. CSV export prefixes `= + - @` tab CR LF unless the value is a number.
  Data: calamine 0.38 (RUSTSEC-2026-0436), zip files over 100 MB unpacked refused, XLSX/XLSB read
  cell by cell (≤ 200 columns, more refuses the file); every money / quantity field ≤ numeric(15,2)
  ("Amount is too large"); manual journal entries cannot use a source type the app writes itself.
  Permissions: `GET /settings` hides `payroll.*` without `payroll` and `integration.*` without
  `settings` (keys see neither); PAYSLIP documents need `payroll` as well as `employees`; invoice
  imports and connector syncs started by a user without `customers` report unknown customers
  instead of creating them (the hourly sync still creates them); `invoices:all` scope also applies
  to bank-line documents / payments and to INVOICES imports. Integrations: reqwest errors are
  shown without their URL (the WooCommerce secret could be in it); WooCommerce refuses addresses
  that are IP literals or resolve to private / loopback / link-local ranges and pins the checked
  address; redirects only to the same scheme, host and port; Shopify's next-page link is compared
  by parsed origin. Backup: restore accepts only the manifest, `database/accounting.db`, the storage
  key and `files/…`; Argon2 settings above 4× the app's own are refused; network (UNC) paths are
  refused for backup folders and restore files. Release: the update-signing key is only in the
  signing step (build with `createUpdaterArtifacts:false`, then `tauri signer sign`), actions
  pinned by commit, no `${{ }}` inside `run:`; `*.zip` is git-ignored.

- **D51 TallyCheck names for the Claude connector** (2026-11-03). `crates/mcp`
  builds the same server twice: `tallycheck-mcp.exe` (new, `src/bin/tallycheck-mcp.rs`) and
  `cenango-mcp.exe` (kept for existing Claude connections); both are installed (installer and MSIX).
  It reads `TALLYCHECK_API_KEY` / `TALLYCHECK_API_URL` first, then `CENANGO_API_KEY` /
  `CENANGO_API_URL`. Settings → Integrations → Claude shows the new names. Unchanged: the plugin id
  `cenango-accounting` and its `.mcp.json`, the MCP `serverInfo.name`, the identifier.

- **D52 Remaining security items** (2026-11-04). The `users` permission no
  longer equals Admin: someone who is not an Admin may only give or manage rights they hold
  themselves (no Admin role, no Admin user, no user or role with a right they lack; creating,
  changing and deleting roles likewise) — 403 "Only an administrator can give or manage rights you
  don't have yourself"; Admins are unchanged. Claude keys: only an Administrator sees and manages
  every key; others list, change and revoke only their own (someone else's is 404). Ending
  sign-ins: table `session_cutoffs` (userId, validFrom Unix seconds); tokens with `iat` before it
  are refused (CurrentUser and the 2FA steps). Set by an administrator's password or 2FA reset, a
  recovery-key or Windows reset and switching a user off. 2FA codes work once: the last step used
  per user is kept in memory and an older or equal step is refused ("This code was already used.
  Wait for the next code…"). A failed sign-in takes at least 800 ms whatever the stored hash, so
  timing doesn't reveal usernames or imported bcrypt passwords. Scheduled backups refuse a network
  (UNC) folder saved earlier (`last_error` shown under Settings → Backup). Old .xls files are
  checked before calamine reads them (≤ 5M cells, < 200 columns, malformed files refused).
  Not covered on purpose: LAN traffic is plain HTTP and installers are unsigned (§9).

- **D53 macOS app** (2026-11-06; the company has an Apple Developer account).
  One universal app (Apple silicon + Intel, macOS 11+) from the same code; `docs/macos.md`.
  Data folder `~/Library/Application Support/TallyCheckData` (`config::data_base`); a folder on a
  network volume is refused (statfs without `MNT_LOCAL`). Secret files keep their names and are
  AES-256-GCM sealed (`TCK1` | nonce | ciphertext, entropy as associated data) under a 32-byte key
  in the login Keychain (service "TallyCheck data protection", account = the random id in
  `secrets/keychain-id`), so like DPAPI they open only for that user on that Mac. Windows sign-in
  recovery is not offered (no verifier; the recovery key still works). LAN host names use the Mac's
  host name. Build: `bash tools/build_macos.sh` on a Mac, or the `macos.yml` workflow; signing and
  notarization through Tauri's `APPLE_*` variables (secrets only, never in the repo); bundle
  settings `src-tauri/tauri.macos.json` (the connector ships as the `tallycheck-mcp` sidecar).
  CI runs every check on a Mac too (`checks-macos`). Not yet: in-app updates on macOS, the Mac
  App Store, Touch ID recovery. Windows builds are unchanged.

- **D54 Publisher name** (2026-11-06, undone 2026-11-12 on request). For a few days the
  copyright holder and publisher read "Cenango Financial LLC", the company that owns the Apple
  Developer and Microsoft Partner Center accounts; everything says **Cenango (Pvt) Ltd** again
  (`LICENSE-MIT`, `NOTICE`, `PRIVACY.md`, README, `tauri.conf.json`, Help → About, notices header,
  Claude plugin owner). The Mac app's Developer ID certificate still carries the Apple account's
  name, and the Store listing explains in its certification notes that Cenango Financial LLC
  publishes the app for Cenango (Pvt) Ltd. The MSIX publisher display name stays "Cenango".

Reference quirks reproduced on purpose (verified by tests): audit actions such as
"Created an user" and "Posted a payroll" (the payroll-specific texts are unreachable in the
reference); `POST /users` returns the user without a `role` key; `POST /users/:id/reset-*`
answer 201.

## 9. Open items

- April 2026 regression: need the real employee's salary row (basic / BRA2005 / BRA2016 /
  vehicle / other split). Until then the fixture is reverse-engineered and marked as such.
  W5 computes the reverse-engineered row exactly as the reference does.
- Payroll rounding: keep D4 (exact half-up, 1-cent differences on exact ties) or reproduce
  the reference's float rounding bit for bit. Needs a decision.
- Reference quirks to decide in their wave: bills never accumulate partial AP payments; a
  part-paid bill can be voided; `POST /accounts/seed` does nothing after the payroll migration
  seeded 11 accounts.
- The copied frontend fails `npm run lint` (57 problems: 49 errors, 8 warnings, all inherited from
  the reference).
- Security advisories fixed 2026-10-28 : `npm audit fix` in `frontend/` moved
  react-router(-dom) 7.16.0 → 7.18.5, dompurify 3.4.11 → 3.4.17, form-data 4.0.5 → 4.0.6,
  follow-redirects 1.16.0 → 1.16.2, axios 1.17.0 → 1.21.0 and some build-only tools, within the
  existing version ranges; `npm audit` reports 0 in `frontend/` and the root. `cargo audit` lists
  `glib` / `proc-macro-error`, Linux (GTK) only. 2026-11-18: sqlx 0.8.6 → 0.9.1 and
  libsqlite3-sys 0.30.1 → 0.40.0 (bundled SQLite 3.54+) for RUSTSEC-2026-0459; a test checks
  the SQLite version; the old `rsa` entry (RUSTSEC-2023-0071) left Cargo.lock with it. Run both
  audits before each release.
- Fresh installs get the default chart automatically (D20); the migration itself still seeds
  only the 11 payroll accounts, as the reference did, so imports are unaffected.
- `currentBalance` on bank accounts is never updated by postings (balances come from the
    ledger); there is no Banking screen in the frontend.
- W6: no code-signing certificate is available, so installers built here are unsigned
  (`docs/installer-and-updates.md`). LAN traffic is plain HTTP (`docs/lan-mode.md`).
- The production reference server's time zone (for importing `timestamp without time zone`
  values) is unconfirmed.
