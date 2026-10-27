-- D43: US pay runs, their lines, tax deposits and state income tax rules (the web edition's
-- migrations 005 + 006, waves U3 and U5b). Separate from the Sri Lankan "payroll_runs" tables,
-- which are monthly and EPF/ETF/PAYE-shaped. Amounts are cents; hours are hundredths of an hour;
-- rates are parts per million. The accounts US payroll posts to are chosen in the US payroll
-- settings (payroll.us.account.*); a posted run keeps the ones it used in "accountsSnapshot", so
-- paying it and depositing its taxes clear the same accounts even if the settings change later.
CREATE TABLE "us_payroll_runs" (
    "id" TEXT PRIMARY KEY NOT NULL,
    "runNumber" TEXT NOT NULL UNIQUE,
    "periodStart" TEXT NOT NULL CHECK ("periodStart" = date("periodStart")),
    "periodEnd" TEXT NOT NULL CHECK ("periodEnd" = date("periodEnd")),
    "payDate" TEXT NOT NULL CHECK ("payDate" = date("payDate")),
    "payFrequency" TEXT NOT NULL CHECK ("payFrequency" IN ('WEEKLY', 'BIWEEKLY', 'SEMIMONTHLY', 'MONTHLY')),
    "status" TEXT NOT NULL DEFAULT 'DRAFT' CHECK ("status" IN ('DRAFT', 'POSTED', 'PAID', 'VOID')),
    "notes" TEXT,
    -- SUTA rate / wage base and FUTA credit reduction in force when the run was made.
    "settingsSnapshot" TEXT NOT NULL DEFAULT '{}' CHECK (json_valid("settingsSnapshot")),
    -- The accounts the posting used (role -> account id); '{}' until posted.
    "accountsSnapshot" TEXT NOT NULL DEFAULT '{}' CHECK (json_valid("accountsSnapshot")),
    "employeeCount" INTEGER NOT NULL DEFAULT 0,
    "grossTotal" INTEGER NOT NULL DEFAULT 0,
    "pretax401kTotal" INTEGER NOT NULL DEFAULT 0,
    "pretaxSection125Total" INTEGER NOT NULL DEFAULT 0,
    "federalIncomeTaxTotal" INTEGER NOT NULL DEFAULT 0,
    "socialSecurityEmployeeTotal" INTEGER NOT NULL DEFAULT 0,
    "socialSecurityEmployerTotal" INTEGER NOT NULL DEFAULT 0,
    "medicareEmployeeTotal" INTEGER NOT NULL DEFAULT 0,
    "additionalMedicareTotal" INTEGER NOT NULL DEFAULT 0,
    "medicareEmployerTotal" INTEGER NOT NULL DEFAULT 0,
    "futaTotal" INTEGER NOT NULL DEFAULT 0,
    "sutaTotal" INTEGER NOT NULL DEFAULT 0,
    "netPayTotal" INTEGER NOT NULL DEFAULT 0,
    "employerTaxesTotal" INTEGER NOT NULL DEFAULT 0,
    "stateIncomeTaxTotal" INTEGER NOT NULL DEFAULT 0,
    "journalEntryId" TEXT REFERENCES "journal_entries" ("id"),
    "paymentJournalEntryId" TEXT REFERENCES "journal_entries" ("id"),
    "paymentAccountId" TEXT REFERENCES "accounts" ("id"),
    "paidDate" TEXT CHECK ("paidDate" IS NULL OR "paidDate" = date("paidDate")),
    "postedAt" TEXT,
    "paidAt" TEXT,
    "createdById" TEXT REFERENCES "users" ("id") ON DELETE SET NULL,
    "createdByName" TEXT,
    "createdAt" TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    "updatedAt" TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    CONSTRAINT "CHK_us_payroll_runs_period" CHECK ("periodStart" <= "periodEnd")
) STRICT;
CREATE INDEX "IDX_us_payroll_runs_payDate" ON "us_payroll_runs" ("payDate");

CREATE TABLE "us_payroll_lines" (
    "id" TEXT PRIMARY KEY NOT NULL,
    "runId" TEXT NOT NULL REFERENCES "us_payroll_runs" ("id") ON DELETE CASCADE,
    "employeeId" TEXT REFERENCES "employees" ("id") ON DELETE SET NULL,
    "employeeCode" TEXT NOT NULL,
    "employeeName" TEXT NOT NULL,
    "workState" TEXT NOT NULL,
    "payType" TEXT NOT NULL CHECK ("payType" IN ('SALARY', 'HOURLY')),
    "annualSalary" INTEGER NOT NULL DEFAULT 0,
    "hourlyRate" INTEGER NOT NULL DEFAULT 0,
    "hours" INTEGER NOT NULL DEFAULT 0 CHECK ("hours" >= 0),
    "overtimeHours" INTEGER NOT NULL DEFAULT 0 CHECK ("overtimeHours" >= 0),
    "regularPay" INTEGER NOT NULL DEFAULT 0,
    "overtimePay" INTEGER NOT NULL DEFAULT 0,
    "bonus" INTEGER NOT NULL DEFAULT 0 CHECK ("bonus" >= 0),
    "otherEarnings" INTEGER NOT NULL DEFAULT 0 CHECK ("otherEarnings" >= 0),
    "gross" INTEGER NOT NULL DEFAULT 0,
    "pretax401k" INTEGER NOT NULL DEFAULT 0 CHECK ("pretax401k" >= 0),
    "pretaxSection125" INTEGER NOT NULL DEFAULT 0 CHECK ("pretaxSection125" >= 0),
    -- The Form W-4 used (as the employee's record stood when the line was made or last worked out).
    "w4Snapshot" TEXT NOT NULL DEFAULT '{}' CHECK (json_valid("w4Snapshot")),
    -- The state rule and certificate the line was worked out with (rule null = no rule yet).
    "stateRule" TEXT CHECK ("stateRule" IS NULL OR json_valid("stateRule")),
    "stateCertificate" TEXT NOT NULL DEFAULT '{}' CHECK (json_valid("stateCertificate")),
    "stateIncomeTaxWages" INTEGER NOT NULL DEFAULT 0,
    "stateIncomeTax" INTEGER NOT NULL DEFAULT 0,
    -- Wages paid earlier in the calendar year (posted runs), for the wage-base limits.
    "ytdSocialSecurityWages" INTEGER NOT NULL DEFAULT 0,
    "ytdMedicareWages" INTEGER NOT NULL DEFAULT 0,
    "ytdFutaWages" INTEGER NOT NULL DEFAULT 0,
    "ytdSutaWages" INTEGER NOT NULL DEFAULT 0,
    "incomeTaxWages" INTEGER NOT NULL DEFAULT 0,
    "federalIncomeTax" INTEGER NOT NULL DEFAULT 0,
    "socialSecurityWages" INTEGER NOT NULL DEFAULT 0,
    "socialSecurityEmployee" INTEGER NOT NULL DEFAULT 0,
    "socialSecurityEmployer" INTEGER NOT NULL DEFAULT 0,
    "medicareWages" INTEGER NOT NULL DEFAULT 0,
    "medicareEmployee" INTEGER NOT NULL DEFAULT 0,
    "additionalMedicare" INTEGER NOT NULL DEFAULT 0,
    "medicareEmployer" INTEGER NOT NULL DEFAULT 0,
    "futaWages" INTEGER NOT NULL DEFAULT 0,
    "futa" INTEGER NOT NULL DEFAULT 0,
    "sutaWages" INTEGER NOT NULL DEFAULT 0,
    "suta" INTEGER NOT NULL DEFAULT 0,
    "employeeTaxes" INTEGER NOT NULL DEFAULT 0,
    "netPay" INTEGER NOT NULL DEFAULT 0,
    "employerTaxes" INTEGER NOT NULL DEFAULT 0,
    "costToEmployer" INTEGER NOT NULL DEFAULT 0,
    "notes" TEXT,
    CONSTRAINT "UQ_us_payroll_lines_run_employee" UNIQUE ("runId", "employeeId")
) STRICT;
CREATE INDEX "IDX_us_payroll_lines_runId" ON "us_payroll_lines" ("runId");
CREATE INDEX "IDX_us_payroll_lines_employeeId" ON "us_payroll_lines" ("employeeId");

CREATE TABLE "us_payroll_remittances" (
    "id" TEXT PRIMARY KEY NOT NULL,
    "runId" TEXT NOT NULL REFERENCES "us_payroll_runs" ("id") ON DELETE CASCADE,
    "type" TEXT NOT NULL CHECK ("type" IN ('FEDERAL_INCOME_TAX', 'SOCIAL_SECURITY', 'MEDICARE', 'FUTA', 'SUTA',
                                          'RETIREMENT_401K', 'SECTION_125', 'STATE_INCOME_TAX')),
    "amount" INTEGER NOT NULL CHECK ("amount" > 0),
    "date" TEXT NOT NULL CHECK ("date" = date("date")),
    "paymentAccountId" TEXT NOT NULL REFERENCES "accounts" ("id"),
    "reference" TEXT,
    -- A Form 941 deposit pays federal income tax, social security and Medicare in one journal
    -- entry: one row per part, sharing the entry.
    "journalEntryId" TEXT NOT NULL REFERENCES "journal_entries" ("id"),
    "createdByName" TEXT,
    "createdAt" TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;
CREATE INDEX "IDX_us_payroll_remittances_runId" ON "us_payroll_remittances" ("runId");

-- State income tax withholding from rules each company enters (the app ships no state's rates).
-- Brackets: JSON [[upper cents or null, rate ppm], …] on annual taxable income.
CREATE TABLE "us_state_tax_rules" (
    "state" TEXT PRIMARY KEY NOT NULL CHECK (length("state") = 2 AND "state" GLOB '[A-Z][A-Z]'),
    "method" TEXT NOT NULL CHECK ("method" IN ('NONE', 'FLAT', 'BRACKETS')),
    "flatRate" INTEGER NOT NULL DEFAULT 0 CHECK ("flatRate" BETWEEN 0 AND 1000000),
    "brackets" TEXT NOT NULL DEFAULT '[]' CHECK (json_valid("brackets")),
    "bracketsMarried" TEXT CHECK ("bracketsMarried" IS NULL OR json_valid("bracketsMarried")),
    "standardDeductionSingle" INTEGER NOT NULL DEFAULT 0 CHECK ("standardDeductionSingle" >= 0),
    "standardDeductionMarried" INTEGER NOT NULL DEFAULT 0 CHECK ("standardDeductionMarried" >= 0),
    "allowanceDeduction" INTEGER NOT NULL DEFAULT 0 CHECK ("allowanceDeduction" >= 0),
    "allowanceCredit" INTEGER NOT NULL DEFAULT 0 CHECK ("allowanceCredit" >= 0),
    "excludes401k" INTEGER NOT NULL DEFAULT 1 CHECK ("excludes401k" IN (0, 1)),
    "notes" TEXT,
    "updatedAt" TEXT NOT NULL
) STRICT;

-- The employee's state withholding certificate.
ALTER TABLE "employee_us_tax" ADD COLUMN "stateFilingStatus" TEXT NOT NULL DEFAULT 'SINGLE'
    CHECK ("stateFilingStatus" IN ('SINGLE', 'MARRIED'));
ALTER TABLE "employee_us_tax" ADD COLUMN "stateAllowances" INTEGER NOT NULL DEFAULT 0
    CHECK ("stateAllowances" BETWEEN 0 AND 99);
ALTER TABLE "employee_us_tax" ADD COLUMN "stateExtraWithholding" INTEGER NOT NULL DEFAULT 0
    CHECK ("stateExtraWithholding" >= 0);
ALTER TABLE "employee_us_tax" ADD COLUMN "stateExempt" INTEGER NOT NULL DEFAULT 0
    CHECK ("stateExempt" IN (0, 1));
