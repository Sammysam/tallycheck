-- D21 (requested 2026-10-20): customer credit notes and vendor debit notes. The reference had
-- none; the only way to reduce an invoice was to void it whole.
--   AR (credit note, CN-...): debit a revenue account, credit 1100 AR, against a posted invoice.
--   AP (debit note,  DN-...): debit 2000 AP, credit an expense/asset account, against a posted bill.
-- A note never exceeds what is still owed, and can be voided (its entry is reversed).
CREATE TABLE "credit_notes" (
  "id"             TEXT    NOT NULL PRIMARY KEY,
  "noteNumber"     TEXT    NOT NULL UNIQUE,
  "type"           TEXT    NOT NULL CHECK ("type" IN ('AR', 'AP')),
  "invoiceId"      TEXT    REFERENCES "invoices" ("id"),
  "billId"         TEXT    REFERENCES "bills" ("id"),
  "date"           TEXT    NOT NULL CHECK (date("date") IS "date"),
  "reason"         TEXT    NOT NULL,
  "amount"         INTEGER NOT NULL CHECK ("amount" > 0),
  "accountId"      TEXT    NOT NULL REFERENCES "accounts" ("id"),
  "status"         TEXT    NOT NULL DEFAULT 'POSTED' CHECK ("status" IN ('POSTED', 'VOID')),
  "journalEntryId" TEXT    REFERENCES "journal_entries" ("id"),
  "createdById"    TEXT    REFERENCES "users" ("id") ON DELETE SET NULL,
  "createdAt"      TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  "updatedAt"      TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  CHECK (("type" = 'AR' AND "invoiceId" IS NOT NULL AND "billId" IS NULL)
      OR ("type" = 'AP' AND "billId" IS NOT NULL AND "invoiceId" IS NULL))
) STRICT;
CREATE INDEX "IDX_credit_notes_invoiceId" ON "credit_notes" ("invoiceId");
CREATE INDEX "IDX_credit_notes_billId" ON "credit_notes" ("billId");

-- What posted notes have taken off each document; outstanding = total - paid - credited.
ALTER TABLE "invoices" ADD COLUMN "amountCredited" INTEGER NOT NULL DEFAULT 0;
ALTER TABLE "bills" ADD COLUMN "amountCredited" INTEGER NOT NULL DEFAULT 0;
