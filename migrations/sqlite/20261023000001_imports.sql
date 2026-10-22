-- D37: import from file. Each import is remembered with what it created, so the same outside
-- record (a bank line, an invoice number) is never imported twice and an import can be undone
-- while its records are untouched.
CREATE TABLE "imports" (
    "id" TEXT PRIMARY KEY NOT NULL,
    "kind" TEXT NOT NULL CHECK ("kind" IN ('BANK', 'INVOICES', 'CUSTOMERS', 'VENDORS')),
    "source" TEXT NOT NULL,
    "fileName" TEXT,
    "accountId" TEXT REFERENCES "accounts" ("id"),
    "created" INTEGER NOT NULL DEFAULT 0,
    "skipped" INTEGER NOT NULL DEFAULT 0,
    "status" TEXT NOT NULL DEFAULT 'ACTIVE' CHECK ("status" IN ('ACTIVE', 'UNDONE')),
    "createdById" TEXT REFERENCES "users" ("id"),
    "createdAt" TEXT NOT NULL,
    "updatedAt" TEXT NOT NULL
) STRICT;

CREATE TABLE "import_items" (
    "id" TEXT PRIMARY KEY NOT NULL,
    "importId" TEXT NOT NULL REFERENCES "imports" ("id"),
    "kind" TEXT NOT NULL CHECK ("kind" IN ('BANK', 'INVOICES', 'CUSTOMERS', 'VENDORS')),
    "externalKey" TEXT NOT NULL,
    "recordType" TEXT NOT NULL CHECK ("recordType" IN ('BANK_LINE', 'INVOICE', 'CUSTOMER', 'VENDOR')),
    "recordId" TEXT NOT NULL,
    UNIQUE ("kind", "externalKey")
) STRICT;
CREATE INDEX "IDX_import_items_import" ON "import_items" ("importId");

-- Bank statement lines waiting to be matched. "amount" is in the account's own currency, in
-- cents: positive = money in, negative = money out.
CREATE TABLE "bank_lines" (
    "id" TEXT PRIMARY KEY NOT NULL,
    "importId" TEXT REFERENCES "imports" ("id"),
    "accountId" TEXT NOT NULL REFERENCES "accounts" ("id"),
    "date" TEXT NOT NULL CHECK (date("date") IS NOT NULL),
    "description" TEXT NOT NULL,
    "reference" TEXT,
    "amount" INTEGER NOT NULL CHECK ("amount" <> 0),
    "status" TEXT NOT NULL DEFAULT 'UNMATCHED' CHECK ("status" IN ('UNMATCHED', 'MATCHED', 'IGNORED')),
    "matchType" TEXT CHECK ("matchType" IS NULL OR "matchType" IN ('PAYMENT', 'EXPENSE')),
    "matchId" TEXT,
    "createdAt" TEXT NOT NULL,
    "updatedAt" TEXT NOT NULL,
    CHECK (("status" = 'MATCHED') = ("matchType" IS NOT NULL AND "matchId" IS NOT NULL))
) STRICT;
CREATE INDEX "IDX_bank_lines_account" ON "bank_lines" ("accountId", "status", "date");

-- Saved column layouts ("Commercial Bank CSV"), so the next file from the same place is one click.
CREATE TABLE "import_layouts" (
    "id" TEXT PRIMARY KEY NOT NULL,
    "kind" TEXT NOT NULL CHECK ("kind" IN ('BANK', 'INVOICES', 'CUSTOMERS', 'VENDORS', 'OPENING')),
    "name" TEXT NOT NULL,
    "layout" TEXT NOT NULL CHECK (json_valid("layout")),
    "updatedAt" TEXT NOT NULL,
    UNIQUE ("kind", "name")
) STRICT;
