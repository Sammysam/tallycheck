-- D67: state disability insurance and paid family / medical leave contributions (step 4 of the
-- US state payroll plan). The programs and their 2026 rates are built into the app
-- (payroll::us_leave); these columns hold what each company and pay line adds.

-- The company's headcount and choices per state: employees (for the programs' small-employer
-- rules; NULL = not given, treated as a large employer), options per program
-- ({"NJ_TDI": {"employerRate": 5000}, "NY_DBL": {"off": true}, "MN_PL": {"small": true}}, rates
-- in ppm) and an optional payable account for that state's programs.
ALTER TABLE "us_state_registrations" ADD COLUMN "employees" INTEGER CHECK ("employees" IS NULL OR "employees" >= 0);
ALTER TABLE "us_state_registrations" ADD COLUMN "leaveOptions" TEXT NOT NULL DEFAULT '{}'
    CHECK (json_valid("leaveOptions") AND json_type("leaveOptions") = 'object');
ALTER TABLE "us_state_registrations" ADD COLUMN "leaveAccountId" TEXT REFERENCES "accounts" ("id");

-- Programs an employee is exempt from (e.g. ["WA_CARES"] with an approved exemption).
ALTER TABLE "employee_us_tax" ADD COLUMN "leaveExempt" TEXT NOT NULL DEFAULT '[]'
    CHECK (json_valid("leaveExempt") AND json_type("leaveExempt") = 'array');

-- Each pay line keeps the programs' parameters it used, the year-to-date figures it counted
-- and its contributions (cents), plus the employee and employer totals.
ALTER TABLE "us_payroll_lines" ADD COLUMN "leavePlan" TEXT NOT NULL DEFAULT '[]'
    CHECK (json_valid("leavePlan") AND json_type("leavePlan") = 'array');
ALTER TABLE "us_payroll_lines" ADD COLUMN "leaveYtd" TEXT NOT NULL DEFAULT '{}'
    CHECK (json_valid("leaveYtd") AND json_type("leaveYtd") = 'object');
ALTER TABLE "us_payroll_lines" ADD COLUMN "leave" TEXT NOT NULL DEFAULT '[]'
    CHECK (json_valid("leave") AND json_type("leave") = 'array');
ALTER TABLE "us_payroll_lines" ADD COLUMN "leaveEmployee" INTEGER NOT NULL DEFAULT 0;
ALTER TABLE "us_payroll_lines" ADD COLUMN "leaveEmployer" INTEGER NOT NULL DEFAULT 0;

ALTER TABLE "us_payroll_runs" ADD COLUMN "leaveEmployeeTotal" INTEGER NOT NULL DEFAULT 0;
ALTER TABLE "us_payroll_runs" ADD COLUMN "leaveEmployerTotal" INTEGER NOT NULL DEFAULT 0;

-- Payments to a leave program (type STATE_LEAVE, "state" = the program, e.g. CA_SDI). SQLite
-- cannot change a CHECK, so the table is rebuilt with every row kept; nothing references it.
CREATE TABLE "us_payroll_remittances_new" (
    "id" TEXT PRIMARY KEY NOT NULL,
    "runId" TEXT NOT NULL REFERENCES "us_payroll_runs" ("id") ON DELETE CASCADE,
    "type" TEXT NOT NULL CHECK ("type" IN ('FEDERAL_INCOME_TAX', 'SOCIAL_SECURITY', 'MEDICARE', 'FUTA', 'SUTA',
                                          'RETIREMENT_401K', 'SECTION_125', 'STATE_INCOME_TAX', 'STATE_LEAVE')),
    "amount" INTEGER NOT NULL CHECK ("amount" > 0),
    "date" TEXT NOT NULL CHECK ("date" = date("date")),
    "paymentAccountId" TEXT NOT NULL REFERENCES "accounts" ("id"),
    "reference" TEXT,
    "journalEntryId" TEXT NOT NULL REFERENCES "journal_entries" ("id"),
    "createdByName" TEXT,
    "createdAt" TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    "state" TEXT,
    CHECK ("type" <> 'STATE_LEAVE' OR "state" IS NOT NULL)
) STRICT;
INSERT INTO "us_payroll_remittances_new" ("id","runId","type","amount","date","paymentAccountId","reference",
    "journalEntryId","createdByName","createdAt","state")
SELECT "id","runId","type","amount","date","paymentAccountId","reference","journalEntryId","createdByName",
    "createdAt","state" FROM "us_payroll_remittances";
DROP TABLE "us_payroll_remittances";
ALTER TABLE "us_payroll_remittances_new" RENAME TO "us_payroll_remittances";
CREATE INDEX "IDX_us_payroll_remittances_runId" ON "us_payroll_remittances" ("runId");
