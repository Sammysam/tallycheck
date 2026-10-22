-- D38: products & services, stock at average cost, and line discounts on invoices.
--
-- An INVENTORY item keeps its quantity on hand (hundredths, like invoice quantities) and the
-- value of that stock (cents). Its average cost is value / quantity. Every change is a row in
-- "stock_moves" whose value is exactly what was posted to the item's inventory account, so the
-- items' stock value always equals the inventory account in the ledger.
CREATE TABLE "items" (
    "id" TEXT PRIMARY KEY NOT NULL,
    "sku" TEXT,
    "name" TEXT NOT NULL CHECK (length(trim("name")) > 0),
    "type" TEXT NOT NULL CHECK ("type" IN ('INVENTORY', 'SERVICE', 'NON_INVENTORY')),
    "description" TEXT,
    "salePrice" INTEGER CHECK ("salePrice" IS NULL OR "salePrice" >= 0),
    "purchaseCost" INTEGER CHECK ("purchaseCost" IS NULL OR "purchaseCost" >= 0),
    "incomeAccountId" TEXT REFERENCES "accounts" ("id"),
    "expenseAccountId" TEXT REFERENCES "accounts" ("id"),
    "inventoryAccountId" TEXT REFERENCES "accounts" ("id"),
    "cogsAccountId" TEXT REFERENCES "accounts" ("id"),
    "taxCodeId" TEXT REFERENCES "tax_codes" ("id"),
    "reorderPoint" INTEGER CHECK ("reorderPoint" IS NULL OR "reorderPoint" >= 0),
    "quantityOnHand" INTEGER NOT NULL DEFAULT 0 CHECK ("quantityOnHand" >= 0),
    "stockValue" INTEGER NOT NULL DEFAULT 0 CHECK ("stockValue" >= 0),
    "isActive" INTEGER NOT NULL DEFAULT 1 CHECK ("isActive" IN (0, 1)),
    "createdAt" TEXT NOT NULL,
    "updatedAt" TEXT NOT NULL,
    CHECK ("type" = 'INVENTORY' OR ("quantityOnHand" = 0 AND "stockValue" = 0)),
    CHECK ("type" <> 'INVENTORY' OR ("inventoryAccountId" IS NOT NULL AND "cogsAccountId" IS NOT NULL)),
    CHECK ("quantityOnHand" > 0 OR "stockValue" = 0)
) STRICT;
CREATE UNIQUE INDEX "UQ_items_name" ON "items" (lower(trim("name")));
CREATE UNIQUE INDEX "UQ_items_sku" ON "items" (lower(trim("sku"))) WHERE "sku" IS NOT NULL;

CREATE TABLE "stock_moves" (
    "id" TEXT PRIMARY KEY NOT NULL,
    "itemId" TEXT NOT NULL REFERENCES "items" ("id"),
    "date" TEXT NOT NULL CHECK (date("date") IS NOT NULL),
    "quantity" INTEGER NOT NULL CHECK ("quantity" <> 0),
    "value" INTEGER NOT NULL,
    "sourceType" TEXT NOT NULL CHECK ("sourceType" IN ('BILL', 'BILL_VOID', 'INVOICE', 'INVOICE_REVERSAL', 'ADJUSTMENT')),
    "sourceId" TEXT,
    "journalEntryId" TEXT REFERENCES "journal_entries" ("id"),
    "note" TEXT,
    "createdAt" TEXT NOT NULL
) STRICT;
CREATE INDEX "IDX_stock_moves_item" ON "stock_moves" ("itemId", "date");
CREATE INDEX "IDX_stock_moves_source" ON "stock_moves" ("sourceType", "sourceId");

ALTER TABLE "invoice_lines" ADD COLUMN "itemId" TEXT REFERENCES "items" ("id");
-- The discount on the line (what the customer is let off, in the invoice's currency). "amount"
-- is what is charged after it: quantity × price − discount.
ALTER TABLE "invoice_lines" ADD COLUMN "discountAmount" INTEGER NOT NULL DEFAULT 0 CHECK ("discountAmount" >= 0);
ALTER TABLE "bill_lines" ADD COLUMN "itemId" TEXT REFERENCES "items" ("id");
