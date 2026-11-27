-- D79: US sales tax paid to vendors cannot be claimed back (unlike VAT / GST). A standard tax
-- code says whether tax on purchases is recoverable; when it is not, that tax is part of the
-- cost (it posts to the bill line's, expense's or debit note's own account) and the tax return
-- does not subtract it. Each stored split keeps how its tax was treated, so changing the code
-- later never rewrites a saved document.
ALTER TABLE "tax_codes" ADD COLUMN "purchaseTaxRecoverable" INTEGER NOT NULL DEFAULT 1
  CHECK ("purchaseTaxRecoverable" IN (0, 1));
ALTER TABLE "document_taxes" ADD COLUMN "recoverable" INTEGER NOT NULL DEFAULT 1
  CHECK ("recoverable" IN (0, 1));

-- A US company's existing codes: purchase tax is not recoverable from now on (documents
-- already saved keep the treatment they were saved with).
UPDATE "tax_codes" SET "purchaseTaxRecoverable" = 0
 WHERE "kind" = 'STANDARD'
   AND (SELECT "value" FROM "settings" WHERE "key" = 'region.country') = 'US';
