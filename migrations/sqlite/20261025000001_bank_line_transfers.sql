-- D39: a bank line can also be matched to a transfer between two of the company's own accounts
-- (a journal entry: a Shopify payout to the bank, cash banked…), and an imported line can carry a
-- suggested action (record as an expense on / transfer to a given account), e.g. a Shopify fee.
-- SQLite cannot change a CHECK, so the table (new in D37, nothing refers to it) is rebuilt.
CREATE TABLE "bank_lines_new" (
    "id" TEXT PRIMARY KEY NOT NULL,
    "importId" TEXT REFERENCES "imports" ("id"),
    "accountId" TEXT NOT NULL REFERENCES "accounts" ("id"),
    "date" TEXT NOT NULL CHECK (date("date") IS NOT NULL),
    "description" TEXT NOT NULL,
    "reference" TEXT,
    "amount" INTEGER NOT NULL CHECK ("amount" <> 0),
    "status" TEXT NOT NULL DEFAULT 'UNMATCHED' CHECK ("status" IN ('UNMATCHED', 'MATCHED', 'IGNORED')),
    "matchType" TEXT CHECK ("matchType" IS NULL OR "matchType" IN ('PAYMENT', 'EXPENSE', 'TRANSFER')),
    "matchId" TEXT,
    "hintAction" TEXT CHECK ("hintAction" IS NULL OR "hintAction" IN ('EXPENSE', 'TRANSFER')),
    "hintAccountId" TEXT REFERENCES "accounts" ("id"),
    "createdAt" TEXT NOT NULL,
    "updatedAt" TEXT NOT NULL,
    CHECK (("status" = 'MATCHED') = ("matchType" IS NOT NULL AND "matchId" IS NOT NULL)),
    CHECK (("hintAction" IS NULL) = ("hintAccountId" IS NULL))
) STRICT;
INSERT INTO "bank_lines_new" ("id","importId","accountId","date","description","reference","amount","status","matchType","matchId","createdAt","updatedAt")
SELECT "id","importId","accountId","date","description","reference","amount","status","matchType","matchId","createdAt","updatedAt" FROM "bank_lines";
DROP TABLE "bank_lines";
ALTER TABLE "bank_lines_new" RENAME TO "bank_lines";
CREATE INDEX "IDX_bank_lines_account" ON "bank_lines" ("accountId", "status", "date");
