-- D42: US payroll (ported from the web edition's waves U1 + U2): each employee's pay and Form W-4.
-- The company's US payroll settings (pay frequency, state unemployment rate, EIN, …) are settings
-- rows under "payroll.us.*"; "payroll.country" picks Sri Lankan ('LK', the default) or US ('US')
-- payroll. Amounts are cents; "per period" amounts are taken from every paycheck.
CREATE TABLE "employee_us_tax" (
    "employeeId" TEXT PRIMARY KEY NOT NULL REFERENCES "employees" ("id") ON DELETE CASCADE,
    "workState" TEXT NOT NULL CHECK (length("workState") = 2 AND "workState" GLOB '[A-Z][A-Z]'),
    "payType" TEXT NOT NULL DEFAULT 'SALARY' CHECK ("payType" IN ('SALARY', 'HOURLY')),
    "annualSalary" INTEGER NOT NULL DEFAULT 0 CHECK ("annualSalary" >= 0),
    "hourlyRate" INTEGER NOT NULL DEFAULT 0 CHECK ("hourlyRate" >= 0),
    "w4Version" TEXT NOT NULL DEFAULT '2020' CHECK ("w4Version" IN ('2020', 'LEGACY')),
    "filingStatus" TEXT NOT NULL DEFAULT 'SINGLE'
        CHECK ("filingStatus" IN ('SINGLE', 'MARRIED_JOINTLY', 'HEAD_OF_HOUSEHOLD')),
    "multipleJobs" INTEGER NOT NULL DEFAULT 0 CHECK ("multipleJobs" IN (0, 1)),
    "dependentsCredit" INTEGER NOT NULL DEFAULT 0 CHECK ("dependentsCredit" >= 0),
    "otherIncome" INTEGER NOT NULL DEFAULT 0 CHECK ("otherIncome" >= 0),
    "deductions" INTEGER NOT NULL DEFAULT 0 CHECK ("deductions" >= 0),
    "extraWithholding" INTEGER NOT NULL DEFAULT 0 CHECK ("extraWithholding" >= 0),
    "allowances" INTEGER NOT NULL DEFAULT 0 CHECK ("allowances" BETWEEN 0 AND 99),
    "exemptFromWithholding" INTEGER NOT NULL DEFAULT 0 CHECK ("exemptFromWithholding" IN (0, 1)),
    "pretax401k" INTEGER NOT NULL DEFAULT 0 CHECK ("pretax401k" >= 0),
    "pretaxSection125" INTEGER NOT NULL DEFAULT 0 CHECK ("pretaxSection125" >= 0),
    "createdAt" TEXT NOT NULL,
    "updatedAt" TEXT NOT NULL,
    -- Head of household is only on the 2020 and later Form W-4.
    CONSTRAINT "CHK_employee_us_tax_legacy_status"
        CHECK ("w4Version" = '2020' OR "filingStatus" <> 'HEAD_OF_HOUSEHOLD')
) STRICT;
