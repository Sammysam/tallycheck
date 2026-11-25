-- D59: moving books from another accounting system (QuickBooks, Xero, Tally, …) on a cut-over
-- date. One row per conversion; every record it created is listed in "conversion_items" so the
-- conversion can be reversed (documents voided, the opening entry reversed — nothing deleted).

CREATE TABLE "conversions" (
    "id" TEXT PRIMARY KEY NOT NULL,
    "source" TEXT NOT NULL,
    "cutoverDate" TEXT NOT NULL CHECK ("cutoverDate" IS date("cutoverDate")),
    "status" TEXT NOT NULL DEFAULT 'ACTIVE' CHECK ("status" IN ('ACTIVE', 'REVERSED')),
    "openingEntryId" TEXT REFERENCES "journal_entries" ("id"),
    "summary" TEXT NOT NULL DEFAULT '{}' CHECK (json_valid("summary")),
    "createdById" TEXT REFERENCES "users" ("id"),
    "createdAt" TEXT NOT NULL,
    "updatedAt" TEXT NOT NULL,
    "reversedAt" TEXT
) STRICT;

CREATE TABLE "conversion_items" (
    "id" TEXT PRIMARY KEY NOT NULL,
    "conversionId" TEXT NOT NULL REFERENCES "conversions" ("id"),
    "recordType" TEXT NOT NULL CHECK ("recordType" IN ('ACCOUNT', 'CUSTOMER', 'VENDOR', 'INVOICE', 'BILL')),
    "recordId" TEXT NOT NULL,
    "externalKey" TEXT NOT NULL
) STRICT;
CREATE INDEX "IDX_conversion_items_conversion" ON "conversion_items" ("conversionId");
