-- D26 (requested 2026-10-20): taxes for any country — VAT, GST, sales tax. The company defines
-- its own tax codes; nothing about any country's tax law is built in.
--
-- "rate" is the percentage in ten-thousandths, so it stays exact: 18% = 180000,
-- 8.875% = 88750, 0% = 0. Output tax on sales posts to "salesAccountId" (a liability, e.g.
-- Tax Payable); input tax on purchases posts to "purchaseAccountId" (e.g. Tax Receivable, or the
-- same control account).
CREATE TABLE "tax_codes" (
  "id"                TEXT    NOT NULL PRIMARY KEY,
  "name"              TEXT    NOT NULL UNIQUE,
  "rate"              INTEGER NOT NULL CHECK ("rate" BETWEEN 0 AND 1000000),
  "description"       TEXT,
  "salesAccountId"    TEXT    NOT NULL REFERENCES "accounts" ("id"),
  "purchaseAccountId" TEXT    NOT NULL REFERENCES "accounts" ("id"),
  "isActive"          INTEGER NOT NULL DEFAULT 1 CHECK ("isActive" IN (0, 1)),
  "createdAt"         TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  "updatedAt"         TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;

-- Each line keeps the amount as typed; "taxAmount" is its tax, rounded half-up per line.
-- With "pricesIncludeTax" the amount already contains the tax; otherwise the tax is added.
ALTER TABLE "invoice_lines" ADD COLUMN "taxCodeId" TEXT REFERENCES "tax_codes" ("id");
ALTER TABLE "invoice_lines" ADD COLUMN "taxAmount" INTEGER NOT NULL DEFAULT 0;
ALTER TABLE "invoices" ADD COLUMN "taxAmount" INTEGER NOT NULL DEFAULT 0;
ALTER TABLE "invoices" ADD COLUMN "pricesIncludeTax" INTEGER NOT NULL DEFAULT 0 CHECK ("pricesIncludeTax" IN (0, 1));

ALTER TABLE "bill_lines" ADD COLUMN "taxCodeId" TEXT REFERENCES "tax_codes" ("id");
ALTER TABLE "bill_lines" ADD COLUMN "taxAmount" INTEGER NOT NULL DEFAULT 0;
ALTER TABLE "bills" ADD COLUMN "taxAmount" INTEGER NOT NULL DEFAULT 0;
ALTER TABLE "bills" ADD COLUMN "pricesIncludeTax" INTEGER NOT NULL DEFAULT 0 CHECK ("pricesIncludeTax" IN (0, 1));

-- An expense amount is what was paid, tax included.
ALTER TABLE "expenses" ADD COLUMN "taxCodeId" TEXT REFERENCES "tax_codes" ("id");
ALTER TABLE "expenses" ADD COLUMN "taxAmount" INTEGER NOT NULL DEFAULT 0;

-- A credit or debit note amount is the gross reduction, tax included.
ALTER TABLE "credit_notes" ADD COLUMN "taxCodeId" TEXT REFERENCES "tax_codes" ("id");
ALTER TABLE "credit_notes" ADD COLUMN "taxAmount" INTEGER NOT NULL DEFAULT 0;
