# SQLite vs. reference PostgreSQL schema

The baseline migration `migrations/sqlite/20261014000001_baseline.sql` translates reference
migrations `1717400000000` … `1717400021000`. Table names, column names, column order and
NOT NULL rules are identical — verified by `database` tests against
`crates/database/tests/fixtures/reference_pg_*.txt`, which were exported from a PostgreSQL
database built by the reference's own migrations. Every reference foreign key (19, with the
same ON DELETE rule) and unique constraint (11) has an equivalent here.

## Type mapping

| PostgreSQL | SQLite (`STRICT`) | Notes |
|---|---|---|
| `uuid` (`uuid_generate_v4()`) | `TEXT` | Generated in Rust (`uuid` v4). Seed rows use a v4 SQL expression. |
| `numeric(15,2)` money | `INTEGER` cents | `"totalAmount" = 8385000` means 83,850.00. The API renders TypeORM's `"83850.00"` strings. |
| `numeric(15,2)` quantity, `numeric(5,2)` daysWorked | `INTEGER` hundredths | Default 1 → `100`, default 30 → `3000`. |
| `timestamp` (without time zone) | `TEXT` ISO-8601 UTC, ms, `Z` | Reference values are wall-clock in the server's session time zone; the importer must convert using the VPS time zone (to be confirmed — believed UTC). |
| `date` | `TEXT` `YYYY-MM-DD` | `CHECK (date(col) IS col)` rejects impossible dates. |
| enum types | `TEXT` + `CHECK (col IN (…))` | Same value sets; `PAYSLIP` included in document categories. |
| `jsonb` | `TEXT` + `json_valid` | `roles.permissions` must also be a JSON array. |
| `boolean` | `INTEGER` 0/1 + `CHECK` | |
| `bigint` | `INTEGER` | |
| `varchar(7)` `payroll_runs.period` | `TEXT` + `GLOB '[0-9][0-9][0-9][0-9]-[0-1][0-9]'` | |

## Behavioural differences

- **Case-insensitive unique employee ID.** SQLite `lower()` folds ASCII only; PostgreSQL folds
  Unicode. Staff numbers are ASCII in practice (`CEN-001`); the importer reports any clash.
- **Text ordering.** PostgreSQL sorts with the database collation; SQLite sorts bytewise. This
  affects `ORDER BY username` (audit user filter) only when names differ in case or contain
  non-ASCII letters.
- **`ILIKE` search (W2 invoices).** Will be implemented as `LIKE` with ASCII case folding.
- **Constraint names.** PostgreSQL's generated names (`FK_e1496…`) are not reproduced; SQLite
  inline constraints are anonymous. Named indexes keep their reference names where they exist
  in the reference migrations (`UQ_employees_employeeId_ci`, `UQ_payroll_runs_period_live`,
  `IDX_*`).
- **Migration bookkeeping** lives in `_sqlx_migrations`, not TypeORM's `migrations` table.

## Additions (stricter than the reference)

Approved deviation D6 — foreign keys where the reference had an id column but no FK:

| Column | References | ON DELETE |
|---|---|---|
| `journal_entries.reversalOfId` | `journal_entries` | no action |
| `invoices.journalEntryId`, `bills.journalEntryId`, `expenses.journalEntryId`, `payments.journalEntryId` | `journal_entries` | no action |
| `invoices.createdById` | `users` | SET NULL |
| `payments.invoiceId` / `billId` / `customerId` / `vendorId` | invoices / bills / customers / vendors | no action |
| `employees.userId` | `users` | SET NULL |
| `employee_documents.replacesId` / `uploadedById` | `employee_documents` / `users` | SET NULL |
| `employee_salaries.createdById` | `users` | SET NULL |
| `payroll_runs.journalEntryId` / `paymentJournalEntryId` / `paymentAccountId` / `createdById` | journal_entries / journal_entries / accounts / users | no action / no action / no action / SET NULL |
| `payroll_lines.salaryId` | `employee_salaries` | SET NULL |
| `payroll_remittances.paymentAccountId` / `journalEntryId` | accounts / journal_entries | no action |

`audit_logs.userId` deliberately has no FK: the log must outlive user records.

Integrity triggers and checks:

- `journal_entries`: no DELETE; UPDATE only `status` POSTED → REVERSED (+ `updatedAt`).
- `journal_lines`: no UPDATE, no DELETE; `debit >= 0`, `credit >= 0`.
- `journal_entries.reversalOfId` unique: an entry can be reversed once.
- `audit_logs`: append-only (no UPDATE, no DELETE).

Extra indexes (performance only): `journal_entries(date)`, `journal_entries(sourceType, sourceId)`,
`journal_lines(journalEntryId)`, `journal_lines(accountId)`, `invoices(customerId)`,
`invoices(date)`, `invoice_lines(invoiceId)`, `bills(vendorId)`, `bill_lines(billId)`,
`payments(invoiceId)`, `payments(billId)`, `users(roleId)`, `accounts(parentId)`,
`audit_logs(userId)`.

Tables with no reference counterpart (D37, import from file): `imports` (one per import: kind,
source, file, counts, ACTIVE / UNDONE), `import_items` (what each import created; `(kind,
externalKey)` unique so an outside record comes in once), `bank_lines` (imported statement lines,
amount in the account's currency, signed; MATCHED needs a match type and id) and `import_layouts`
(saved column mappings, JSON).

D38 adds `items` (products & services; quantity on hand in hundredths and stock value in cents for
stock items, both never negative) and `stock_moves` (every change in stock with the value posted
to the inventory account), plus `invoice_lines.itemId`, `invoice_lines.discountAmount` (NOT NULL,
default 0) and `bill_lines.itemId`.

D39 rebuilds `bank_lines` with `matchType` TRANSFER (a journal entry) and a suggested action
(`hintAction` EXPENSE / TRANSFER with `hintAccountId`). Shopify settings are rows in `settings`
(`integration.shopify.*`); the access key is not in the database.

## Seeds

Same as the reference migrations: roles Admin / Accountant / Viewer (Admin holds all 17 keys,
Accountant all but `users`), counter `INV` = 8999, signatory settings, payroll rates and
statutory company details, and the 11 payroll accounts (1300, 2200–2260, 6100–6120). As in the
reference, the base chart of accounts (1000 Cash, 1100 AR, 2000 AP, …) is **not** seeded by a
migration; in the reference `POST /accounts/seed` only runs on an empty table, so it never runs
after the payroll migration. Production data arrives through the importer (W6).
