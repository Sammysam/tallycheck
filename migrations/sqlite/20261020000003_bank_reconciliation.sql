-- D22 (requested 2026-10-20): bank reconciliation. The reference had none.
-- A reconciliation matches the ledger lines of one asset account (cash or bank) to a bank
-- statement: the user ticks the lines the statement shows until the cleared balance equals the
-- statement's closing balance, then completes it. Journal lines stay immutable; which lines are
-- cleared, and by which reconciliation, is recorded beside them.
CREATE TABLE "bank_reconciliations" (
  "id"               TEXT    NOT NULL PRIMARY KEY,
  "accountId"        TEXT    NOT NULL REFERENCES "accounts" ("id"),
  "statementDate"    TEXT    NOT NULL CHECK (date("statementDate") IS "statementDate"),
  "statementBalance" INTEGER NOT NULL,
  -- The previous completed reconciliation's statement balance (0 for the first one).
  "openingBalance"   INTEGER NOT NULL DEFAULT 0,
  "status"           TEXT    NOT NULL DEFAULT 'DRAFT' CHECK ("status" IN ('DRAFT', 'COMPLETED')),
  "completedAt"      TEXT,
  "completedById"    TEXT    REFERENCES "users" ("id") ON DELETE SET NULL,
  "createdById"      TEXT    REFERENCES "users" ("id") ON DELETE SET NULL,
  "createdAt"        TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  "updatedAt"        TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;
CREATE INDEX "IDX_bank_reconciliations_accountId" ON "bank_reconciliations" ("accountId");
-- At most one reconciliation in progress per account.
CREATE UNIQUE INDEX "UQ_bank_reconciliations_one_draft" ON "bank_reconciliations" ("accountId") WHERE "status" = 'DRAFT';

-- A journal line is cleared by at most one reconciliation (draft ticks included).
CREATE TABLE "reconciled_lines" (
  "journalLineId"    TEXT NOT NULL PRIMARY KEY REFERENCES "journal_lines" ("id"),
  "reconciliationId" TEXT NOT NULL REFERENCES "bank_reconciliations" ("id") ON DELETE CASCADE
) STRICT;
CREATE INDEX "IDX_reconciled_lines_reconciliationId" ON "reconciled_lines" ("reconciliationId");
