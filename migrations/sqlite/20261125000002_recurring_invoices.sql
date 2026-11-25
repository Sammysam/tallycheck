-- D61: recurring invoices. A schedule holds a copy of an invoice (the template, JSON) and makes a
-- DRAFT invoice on every due date while the app runs; dates missed while it was closed are made
-- one by one when it next runs. "recurring_invoice_items" lists every invoice a schedule made,
-- one per date, so a date is never made twice.

CREATE TABLE "recurring_invoices" (
    "id" TEXT PRIMARY KEY NOT NULL,
    "name" TEXT NOT NULL,
    "sourceInvoiceId" TEXT REFERENCES "invoices" ("id") ON DELETE SET NULL,
    "template" TEXT NOT NULL CHECK (json_valid("template")),
    "frequency" TEXT NOT NULL CHECK ("frequency" IN ('WEEKLY', 'MONTHLY', 'QUARTERLY', 'YEARLY')),
    "every" INTEGER NOT NULL DEFAULT 1 CHECK ("every" BETWEEN 1 AND 99),
    "startDate" TEXT NOT NULL CHECK (date("startDate") IS "startDate"),
    "endDate" TEXT CHECK (date("endDate") IS "endDate"),
    "nextDate" TEXT CHECK (date("nextDate") IS "nextDate"),
    -- How many dates of the schedule have passed (made, or skipped while paused): "nextDate" is
    -- date number "dateIndex" counted from "startDate", so months never drift (31 Jan, 28 Feb, 31 Mar).
    "dateIndex" INTEGER NOT NULL DEFAULT 0 CHECK ("dateIndex" >= 0),
    "status" TEXT NOT NULL DEFAULT 'ACTIVE' CHECK ("status" IN ('ACTIVE', 'PAUSED', 'ENDED')),
    "lastError" TEXT,
    "lastRunAt" TEXT,
    "createdById" TEXT REFERENCES "users" ("id"),
    "createdAt" TEXT NOT NULL,
    "updatedAt" TEXT NOT NULL
) STRICT;

CREATE TABLE "recurring_invoice_items" (
    "recurringId" TEXT NOT NULL REFERENCES "recurring_invoices" ("id") ON DELETE CASCADE,
    "occurrenceDate" TEXT NOT NULL CHECK (date("occurrenceDate") IS "occurrenceDate"),
    "invoiceId" TEXT NOT NULL REFERENCES "invoices" ("id") ON DELETE CASCADE,
    PRIMARY KEY ("recurringId", "occurrenceDate")
) STRICT;
CREATE INDEX "IDX_recurring_invoice_items_invoice" ON "recurring_invoice_items" ("invoiceId");
