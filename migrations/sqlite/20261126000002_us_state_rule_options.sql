-- D64: what the built-in 2026 state rules (and company rules) need beyond D43's flat / brackets
-- method, found in the states' own withholding guides:
--   rounding to whole dollars (North Carolina, Idaho), a head-of-household standard deduction
--   (North Carolina, Iowa, Louisiana), a second kind of allowance with its own amount (Illinois
--   basic $2,925 + additional $1,000; Indiana personal $1,000 + dependent $1,500);
-- and on the employee's state certificate: head of household, the second allowances, a dollar
-- amount that replaces the standard deduction (Colorado DR 0004 line 2), an annual dollar
-- credit (Iowa IA W-4 line 7) and an elected percentage (Arizona A-4).

ALTER TABLE "us_state_tax_rules" ADD COLUMN "rounding" TEXT NOT NULL DEFAULT 'CENT'
    CHECK ("rounding" IN ('CENT', 'DOLLAR'));
ALTER TABLE "us_state_tax_rules" ADD COLUMN "standardDeductionHeadOfHousehold" INTEGER
    CHECK ("standardDeductionHeadOfHousehold" IS NULL OR "standardDeductionHeadOfHousehold" >= 0);
ALTER TABLE "us_state_tax_rules" ADD COLUMN "secondAllowanceDeduction" INTEGER NOT NULL DEFAULT 0
    CHECK ("secondAllowanceDeduction" >= 0);
ALTER TABLE "us_state_tax_rules" ADD COLUMN "secondAllowanceCredit" INTEGER NOT NULL DEFAULT 0
    CHECK ("secondAllowanceCredit" >= 0);

-- Head of household on the state certificate (the status column only allows SINGLE / MARRIED).
ALTER TABLE "employee_us_tax" ADD COLUMN "stateHeadOfHousehold" INTEGER NOT NULL DEFAULT 0
    CHECK ("stateHeadOfHousehold" IN (0, 1));
ALTER TABLE "employee_us_tax" ADD COLUMN "stateSecondAllowances" INTEGER NOT NULL DEFAULT 0
    CHECK ("stateSecondAllowances" BETWEEN 0 AND 99);
-- Cents a year; NULL = use the state's standard deduction.
ALTER TABLE "employee_us_tax" ADD COLUMN "stateDeductionOverride" INTEGER
    CHECK ("stateDeductionOverride" IS NULL OR "stateDeductionOverride" >= 0);
ALTER TABLE "employee_us_tax" ADD COLUMN "stateAnnualCredit" INTEGER NOT NULL DEFAULT 0
    CHECK ("stateAnnualCredit" >= 0);
-- ppm; NULL = the state's default.
ALTER TABLE "employee_us_tax" ADD COLUMN "stateElectedRate" INTEGER
    CHECK ("stateElectedRate" IS NULL OR "stateElectedRate" BETWEEN 0 AND 1000000);
