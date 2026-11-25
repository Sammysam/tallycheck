-- D60: estimates / quotes. An estimate posts nothing to the ledger; it is priced like an invoice
-- (lines, discounts, tax codes, currency) and can be turned into a DRAFT invoice once, which it
-- then links to ("invoiceId", status CONVERTED).

CREATE TABLE "estimates" (
    "id" TEXT PRIMARY KEY NOT NULL,
    "estimateNumber" TEXT NOT NULL UNIQUE,
    "customerId" TEXT REFERENCES "customers" ("id"),
    "date" TEXT NOT NULL CHECK (date("date") IS "date"),
    "expiryDate" TEXT CHECK (date("expiryDate") IS "expiryDate"),
    "status" TEXT NOT NULL DEFAULT 'DRAFT'
        CHECK ("status" IN ('DRAFT', 'SENT', 'ACCEPTED', 'DECLINED', 'CONVERTED')),
    "notes" TEXT,
    "billToName" TEXT,
    "billToAddress" TEXT,
    "paymentOptions" TEXT,
    "signatoryName" TEXT,
    "signatoryTitle" TEXT,
    "paymentTerms" TEXT,
    "totalAmount" INTEGER NOT NULL DEFAULT 0,
    "taxAmount" INTEGER NOT NULL DEFAULT 0,
    "pricesIncludeTax" INTEGER NOT NULL DEFAULT 0 CHECK ("pricesIncludeTax" IN (0, 1)),
    "currency" TEXT,
    "exchangeRate" INTEGER NOT NULL DEFAULT 100000000 CHECK ("exchangeRate" > 0),
    "invoiceId" TEXT REFERENCES "invoices" ("id"),
    "createdById" TEXT REFERENCES "users" ("id"),
    "createdAt" TEXT NOT NULL,
    "updatedAt" TEXT NOT NULL,
    CHECK (("status" = 'CONVERTED') = ("invoiceId" IS NOT NULL))
) STRICT;
CREATE INDEX "IDX_estimates_customer" ON "estimates" ("customerId");

CREATE TABLE "estimate_lines" (
    "id" TEXT PRIMARY KEY NOT NULL,
    "estimateId" TEXT NOT NULL REFERENCES "estimates" ("id") ON DELETE CASCADE,
    "description" TEXT NOT NULL,
    "quantity" INTEGER NOT NULL DEFAULT 100,
    "unitPrice" INTEGER NOT NULL,
    "amount" INTEGER NOT NULL,
    "accountId" TEXT REFERENCES "accounts" ("id"),
    "taxCodeId" TEXT REFERENCES "tax_codes" ("id"),
    "taxAmount" INTEGER NOT NULL DEFAULT 0,
    "itemId" TEXT REFERENCES "items" ("id"),
    "discountAmount" INTEGER NOT NULL DEFAULT 0
) STRICT;
CREATE INDEX "IDX_estimate_lines_estimate" ON "estimate_lines" ("estimateId");
