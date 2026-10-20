-- D30 (requested 2026-10-21): invoices and bills in a foreign currency.
--
-- The books stay in the company's (home) currency. A document in another currency keeps its own
-- amounts (totalAmount, amountPaid, amountCredited, lines, tax) in that currency, plus:
--   "currency"      the ISO code; NULL means the home currency;
--   "exchangeRate"  home units per one foreign unit, in hundred-millionths (1.0 = 100000000);
--   "baseTotal", "baseAmountPaid", "baseAmountCredited"  the same figures in the home currency,
--                   as booked: what the ledger holds for the document.
-- A payment of a foreign document converts at its own rate; the difference from the booked
-- amount it settles is a realised exchange gain or loss. Credit and debit notes use the
-- document's rate. Home-currency documents simply carry rate 1 and base = their own figures.
ALTER TABLE "invoices" ADD COLUMN "currency" TEXT;
ALTER TABLE "invoices" ADD COLUMN "exchangeRate" INTEGER NOT NULL DEFAULT 100000000 CHECK ("exchangeRate" > 0);
ALTER TABLE "invoices" ADD COLUMN "baseTotal" INTEGER NOT NULL DEFAULT 0;
ALTER TABLE "invoices" ADD COLUMN "baseAmountPaid" INTEGER NOT NULL DEFAULT 0;
ALTER TABLE "invoices" ADD COLUMN "baseAmountCredited" INTEGER NOT NULL DEFAULT 0;
UPDATE "invoices" SET "baseTotal" = "totalAmount", "baseAmountPaid" = "amountPaid", "baseAmountCredited" = "amountCredited";

ALTER TABLE "bills" ADD COLUMN "currency" TEXT;
ALTER TABLE "bills" ADD COLUMN "exchangeRate" INTEGER NOT NULL DEFAULT 100000000 CHECK ("exchangeRate" > 0);
ALTER TABLE "bills" ADD COLUMN "baseTotal" INTEGER NOT NULL DEFAULT 0;
ALTER TABLE "bills" ADD COLUMN "baseAmountPaid" INTEGER NOT NULL DEFAULT 0;
ALTER TABLE "bills" ADD COLUMN "baseAmountCredited" INTEGER NOT NULL DEFAULT 0;
UPDATE "bills" SET "baseTotal" = "totalAmount", "baseAmountPaid" = "amountPaid", "baseAmountCredited" = "amountCredited";

-- A payment's "amount" and "withholdingAmount" are in the document's currency. "exchangeRate" is
-- the rate on the payment date; "baseAmount" is the money received or paid in the home currency;
-- "baseSettled" is the booked home amount it cleared off the receivable or payable;
-- "exchangeDifference" = baseSettled − (baseAmount + home value of the tax withheld): positive is
-- a loss on a customer payment (a gain on a vendor payment).
ALTER TABLE "payments" ADD COLUMN "exchangeRate" INTEGER NOT NULL DEFAULT 100000000 CHECK ("exchangeRate" > 0);
ALTER TABLE "payments" ADD COLUMN "baseAmount" INTEGER NOT NULL DEFAULT 0;
ALTER TABLE "payments" ADD COLUMN "baseSettled" INTEGER NOT NULL DEFAULT 0;
ALTER TABLE "payments" ADD COLUMN "exchangeDifference" INTEGER NOT NULL DEFAULT 0;
UPDATE "payments" SET "baseAmount" = "amount", "baseSettled" = "amount" + "withholdingAmount";

-- A note's "amount" is in the document's currency; "baseAmount" is its booked home value.
ALTER TABLE "credit_notes" ADD COLUMN "baseAmount" INTEGER NOT NULL DEFAULT 0;
UPDATE "credit_notes" SET "baseAmount" = "amount";
