-- D27 (requested 2026-10-21): stacked taxes and withholding tax, for any country.
--
-- A tax code is now one of three kinds:
--   STANDARD     one rate, posting to its own sales/purchase tax accounts (as in D26);
--   GROUP        several STANDARD codes charged together (e.g. state + county + city sales tax);
--                a component may be compound: charged on the net plus the components before it.
--                A group's own accounts are not used; each component posts to its own.
--   WITHHOLDING  tax a payer holds back from a payment and pays to the tax office: from a
--                customer's payment (salesAccountId — an asset, the credit we can claim) or from
--                our payment to a vendor (purchaseAccountId — a liability, owed to the tax office).
ALTER TABLE "tax_codes" ADD COLUMN "kind" TEXT NOT NULL DEFAULT 'STANDARD' CHECK ("kind" IN ('STANDARD', 'GROUP', 'WITHHOLDING'));

CREATE TABLE "tax_code_components" (
  "groupId"     TEXT    NOT NULL REFERENCES "tax_codes" ("id") ON DELETE CASCADE,
  "componentId" TEXT    NOT NULL REFERENCES "tax_codes" ("id"),
  "position"    INTEGER NOT NULL,
  "isCompound"  INTEGER NOT NULL DEFAULT 0 CHECK ("isCompound" IN (0, 1)),
  PRIMARY KEY ("groupId", "componentId")
) STRICT;

-- The tax of every taxed line, split by the STANDARD codes that make it up (one row for a
-- standard code, one per component for a group). Postings and the tax return read this, so a
-- later change to a rate or a group never rewrites a saved document's tax.
CREATE TABLE "document_taxes" (
  "id"         TEXT    NOT NULL PRIMARY KEY,
  "sourceType" TEXT    NOT NULL CHECK ("sourceType" IN ('INVOICE_LINE', 'BILL_LINE', 'EXPENSE', 'CREDIT_NOTE')),
  "sourceId"   TEXT    NOT NULL,
  "taxCodeId"  TEXT    NOT NULL REFERENCES "tax_codes" ("id"),
  "taxAmount"  INTEGER NOT NULL
) STRICT;
CREATE INDEX "IDX_document_taxes_source" ON "document_taxes" ("sourceType", "sourceId");

-- Documents taxed before this migration had one standard code per line: copy them over.
INSERT INTO "document_taxes" ("id", "sourceType", "sourceId", "taxCodeId", "taxAmount")
  SELECT lower(hex(randomblob(16))), 'INVOICE_LINE', "id", "taxCodeId", "taxAmount" FROM "invoice_lines" WHERE "taxCodeId" IS NOT NULL
  UNION ALL SELECT lower(hex(randomblob(16))), 'BILL_LINE', "id", "taxCodeId", "taxAmount" FROM "bill_lines" WHERE "taxCodeId" IS NOT NULL
  UNION ALL SELECT lower(hex(randomblob(16))), 'EXPENSE', "id", "taxCodeId", "taxAmount" FROM "expenses" WHERE "taxCodeId" IS NOT NULL
  UNION ALL SELECT lower(hex(randomblob(16))), 'CREDIT_NOTE', "id", "taxCodeId", "taxAmount" FROM "credit_notes" WHERE "taxCodeId" IS NOT NULL;

-- Withholding on payments: the amount held back counts towards settling the invoice or bill.
ALTER TABLE "payments" ADD COLUMN "withholdingTaxCodeId" TEXT REFERENCES "tax_codes" ("id");
ALTER TABLE "payments" ADD COLUMN "withholdingAmount" INTEGER NOT NULL DEFAULT 0 CHECK ("withholdingAmount" >= 0);
