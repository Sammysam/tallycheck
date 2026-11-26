-- D63: US payroll in several states. The company registers in each state where it has
-- employees (account numbers, the unemployment rate the state assigned, optional payable
-- accounts); employees have a home state and may work in more than one state; reciprocity
-- agreements between states are listed; pay lines keep each state's share and tax.

CREATE TABLE "us_state_registrations" (
    "state" TEXT PRIMARY KEY NOT NULL CHECK (length("state") = 2 AND "state" GLOB '[A-Z][A-Z]'),
    "withholdingAccountNo" TEXT,
    "unemploymentAccountNo" TEXT,
    -- The unemployment (SUTA) rate the state assigned the company, in ppm, and its wage base
    -- in cents (NULL = no limit).
    "sutaRate" INTEGER NOT NULL DEFAULT 0 CHECK ("sutaRate" BETWEEN 0 AND 1000000),
    "sutaWageBase" INTEGER CHECK ("sutaWageBase" IS NULL OR "sutaWageBase" >= 0),
    -- Optional payable accounts for this state (NULL = the US payroll role's account).
    "stateTaxAccountId" TEXT REFERENCES "accounts" ("id"),
    "sutaAccountId" TEXT REFERENCES "accounts" ("id"),
    "createdAt" TEXT NOT NULL,
    "updatedAt" TEXT NOT NULL
) STRICT;

-- Pairs of states with a reciprocity agreement: someone living in "residentState" and working
-- in "workState" pays income tax only to the home state.
CREATE TABLE "us_state_reciprocity" (
    "workState" TEXT NOT NULL CHECK (length("workState") = 2 AND "workState" GLOB '[A-Z][A-Z]'),
    "residentState" TEXT NOT NULL CHECK (length("residentState") = 2 AND "residentState" GLOB '[A-Z][A-Z]'),
    "createdAt" TEXT NOT NULL,
    PRIMARY KEY ("workState", "residentState"),
    CHECK ("workState" <> "residentState")
) STRICT;

-- As the home state: DIFFERENCE withholds, for residents working in another state, this
-- state's tax less the other state's (when more); NONE leaves it to the work state.
ALTER TABLE "us_state_tax_rules" ADD COLUMN "residentCredit" TEXT NOT NULL DEFAULT 'NONE'
    CHECK ("residentCredit" IN ('NONE', 'DIFFERENCE'));

-- The employee's home state (NULL = lives in the work state) and the other states they work
-- in, as [["CA", 2500], …] with shares in hundredths of a percent; the work state has the rest.
ALTER TABLE "employee_us_tax" ADD COLUMN "residentState" TEXT
    CHECK ("residentState" IS NULL OR (length("residentState") = 2 AND "residentState" GLOB '[A-Z][A-Z]'));
ALTER TABLE "employee_us_tax" ADD COLUMN "otherWorkStates" TEXT NOT NULL DEFAULT '[]'
    CHECK (json_valid("otherWorkStates"));

-- On each pay line: the home state, the work split, every state rule used, each state's
-- share, wages and tax, and the unemployment state with the rate and wage base used.
ALTER TABLE "us_payroll_lines" ADD COLUMN "residentState" TEXT;
ALTER TABLE "us_payroll_lines" ADD COLUMN "workSplit" TEXT NOT NULL DEFAULT '[]' CHECK (json_valid("workSplit"));
ALTER TABLE "us_payroll_lines" ADD COLUMN "stateRules" TEXT NOT NULL DEFAULT '{}' CHECK (json_valid("stateRules"));
ALTER TABLE "us_payroll_lines" ADD COLUMN "stateTaxes" TEXT NOT NULL DEFAULT '[]' CHECK (json_valid("stateTaxes"));
ALTER TABLE "us_payroll_lines" ADD COLUMN "sutaState" TEXT;
ALTER TABLE "us_payroll_lines" ADD COLUMN "sutaRateUsed" INTEGER;
ALTER TABLE "us_payroll_lines" ADD COLUMN "sutaWageBaseUsed" INTEGER;

-- A state income tax or unemployment payment is for one state.
ALTER TABLE "us_payroll_remittances" ADD COLUMN "state" TEXT;
