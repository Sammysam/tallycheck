# Importing the existing PostgreSQL data (one time)

`accounting-import` copies the old system's database and stored files into a **new, empty** data folder, then reconciles the two. It never writes to the old database: it uses a read-only, repeatable-read transaction. It never writes into a live installation.

## Before you start

1. **The old server's time zone.** The old tables store `timestamp without time zone`, which is the wall-clock time of the server process. The importer needs that offset, for example `+05:30` (Sri Lanka, with no daylight saving since 2006) or `+00:00` (UTC). It refuses to guess. Check it on the old server with `date` or with the `TZ` of the Node process. **Still to be confirmed.**
2. **A copy of the stored files.** For the local storage driver, copy the `STORAGE_LOCAL_PATH` folder. For S3, sync the bucket to a folder with read-only credentials, for example `aws s3 sync s3://<bucket> D:\import\files`. The files stay encrypted; nothing is decrypted on disk.
3. **The old file-encryption key** (`STORAGE_ENCRYPTION_KEY` from the old server's `.env`). The administrator types it in. It is never copied from the `.env` by this project:

   ```
   set ACCOUNTING_DATA_DIR=D:\import\data
   accounting-server import-storage-key
   ```

4. A database connection the old server allows, or a dump restored into any PostgreSQL (`pg_restore -d scratch dump.backup`). Put the password in `PGPASSWORD`, not in the command line.

## Running it

```
set PGPASSWORD=...
accounting-import --source postgres://user@host:5432/accounting_db --source-tz +05:30 ^
                  --target D:\import\data --files D:\import\files
```

Then read `D:\import\data\import-report.md`. The command exits with an error unless the report has **no unexplained discrepancy**. Only then move the folder into place: replace `%LOCALAPPDATA%\CenangoAccounting` while the app is closed. Then start the app and make a backup.

## What it does

* **Inventory.** It lists every source table and column (`information_schema`) and matches them to this app's tables by name; the names are identical (`docs/schema-differences.md`). It reports any source table or column not imported. TypeORM's `migrations` bookkeeping is skipped.
* **Conversion.**
  * `numeric` money and quantities become exact hundredths. A value with more than two decimals is an error, never silently rounded.
  * `boolean` becomes 0 or 1, and `bigint` and `integer` become integers.
  * `timestamp` becomes ISO UTC with milliseconds, using `--source-tz`; `date`, `jsonb`, enums and uuids are copied as text.
  * Primary keys are kept, so every reference survives: employees, salaries, payroll lines, invoices, payments, documents and journal entries.
* **No re-posting.** Journal entries and lines, including reversed ones, are copied as they are. No posting logic runs.
* **One transaction.** Either everything is imported or nothing is. Foreign keys are checked at commit.
* **Seeds.** The fresh folder's seeded roles, accounts and counters are replaced by the source's own rows. Settings are merged, with the source winning.
* **D13.** Each bill's `amountPaid` is filled from its AP payments.
* **Idempotency.** The target must be empty, so importing twice is refused. To redo an import, delete the target folder and run again.
* **Files.** Each file a record refers to (photos and documents) is copied with a content-type sidecar from the record. It is then decrypted with the installed key, and the size is compared with the record.

## The reconciliation report

| Check | Discrepancy when |
|---|---|
| Rows per table | Any count differs (settings may only grow) |
| Every money/quantity column summed on both sides | Any sum differs by a cent |
| Ledger debit and credit totals per account | Any account differs |
| Journal entries | Any entry does not balance |
| SQLite integrity and foreign keys | Not `ok`, or a broken reference |
| Payroll | A line does not recompute from its inputs and the run's rate snapshot, or a run's totals differ from its lines. One-cent exact-tie lines (D4) are listed but not counted as errors. |
| Files | A file is missing, cannot be decrypted, or has the wrong size |

## Proven against the reference

`tests/contract/run-all.sh` drives the full contract scenario through the reference API into PostgreSQL, imports that database and its stored files with this tool, and starts this app on the result. `tests/contract/side_by_side.mjs` then compares 127 reads between the reference and the imported app: every list and record, every document download by hash, and timestamps exactly. All match except the trial balance, account balances and reports, which differ by the approved D1/D13 rules.
