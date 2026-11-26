-- D69: local (city / county) taxes, step 6 of the US state payroll plan.

-- The company's localities: code (e.g. NYC, PHL, OH-COLUMBUS) and the locality as JSON (method,
-- rates in ppm, flat amounts and thresholds in cents, formula, payable account, source).
CREATE TABLE "us_localities" (
    "code" TEXT PRIMARY KEY NOT NULL CHECK (length("code") BETWEEN 1 AND 20 AND "code" NOT GLOB '*[^A-Z0-9-]*'),
    "name" TEXT NOT NULL CHECK (length("name") BETWEEN 1 AND 120),
    "state" TEXT NOT NULL CHECK (length("state") = 2),
    "data" TEXT NOT NULL CHECK (json_valid("data") AND json_type("data") = 'object'),
    "createdAt" TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    "updatedAt" TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;

-- The localities an employee is withheld for: [{code, resident, allowances, secondAllowances,
-- extra (cents), exempt}].
ALTER TABLE "employee_us_tax" ADD COLUMN "localTaxes" TEXT NOT NULL DEFAULT '[]'
    CHECK (json_valid("localTaxes") AND json_type("localTaxes") = 'array');

-- Pay lines keep the localities used, the year-to-date figures and the amounts.
ALTER TABLE "us_payroll_lines" ADD COLUMN "localPlan" TEXT NOT NULL DEFAULT '[]'
    CHECK (json_valid("localPlan") AND json_type("localPlan") = 'array');
ALTER TABLE "us_payroll_lines" ADD COLUMN "localYtd" TEXT NOT NULL DEFAULT '{}'
    CHECK (json_valid("localYtd") AND json_type("localYtd") = 'object');
ALTER TABLE "us_payroll_lines" ADD COLUMN "local" TEXT NOT NULL DEFAULT '[]'
    CHECK (json_valid("local") AND json_type("local") = 'array');
ALTER TABLE "us_payroll_lines" ADD COLUMN "localEmployee" INTEGER NOT NULL DEFAULT 0;
ALTER TABLE "us_payroll_lines" ADD COLUMN "localEmployer" INTEGER NOT NULL DEFAULT 0;
ALTER TABLE "us_payroll_runs" ADD COLUMN "localEmployeeTotal" INTEGER NOT NULL DEFAULT 0;
ALTER TABLE "us_payroll_runs" ADD COLUMN "localEmployerTotal" INTEGER NOT NULL DEFAULT 0;

-- Payments to a locality (type LOCAL_TAX, "state" = the locality code). The table is rebuilt
-- with every row kept, as in 20261127000003.
CREATE TABLE "us_payroll_remittances_new" (
    "id" TEXT PRIMARY KEY NOT NULL,
    "runId" TEXT NOT NULL REFERENCES "us_payroll_runs" ("id") ON DELETE CASCADE,
    "type" TEXT NOT NULL CHECK ("type" IN ('FEDERAL_INCOME_TAX', 'SOCIAL_SECURITY', 'MEDICARE', 'FUTA', 'SUTA',
                                          'RETIREMENT_401K', 'SECTION_125', 'STATE_INCOME_TAX', 'STATE_LEAVE',
                                          'LOCAL_TAX')),
    "amount" INTEGER NOT NULL CHECK ("amount" > 0),
    "date" TEXT NOT NULL CHECK ("date" = date("date")),
    "paymentAccountId" TEXT NOT NULL REFERENCES "accounts" ("id"),
    "reference" TEXT,
    "journalEntryId" TEXT NOT NULL REFERENCES "journal_entries" ("id"),
    "createdByName" TEXT,
    "createdAt" TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    "state" TEXT,
    CHECK ("type" NOT IN ('STATE_LEAVE', 'LOCAL_TAX') OR "state" IS NOT NULL)
) STRICT;
INSERT INTO "us_payroll_remittances_new" ("id","runId","type","amount","date","paymentAccountId","reference",
    "journalEntryId","createdByName","createdAt","state")
SELECT "id","runId","type","amount","date","paymentAccountId","reference","journalEntryId","createdByName",
    "createdAt","state" FROM "us_payroll_remittances";
DROP TABLE "us_payroll_remittances";
ALTER TABLE "us_payroll_remittances_new" RENAME TO "us_payroll_remittances";
CREATE INDEX "IDX_us_payroll_remittances_runId" ON "us_payroll_remittances" ("runId");
