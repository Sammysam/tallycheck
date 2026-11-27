-- D83: payroll paid before the company started using TallyCheck in the same year (another
-- provider or by hand), entered per employee and pay date (or one row per month or quarter).
-- It never reaches the ledger (the money is already in the opening balances); it counts towards
-- each employee's year-to-date wage limits (social security, FUTA, state unemployment,
-- Additional Medicare) and is included in Forms 941, 940, W-2 and the state forms, as already
-- deposited by the previous provider. Amounts are cents.
CREATE TABLE "us_payroll_prior" (
  "id"                     TEXT    NOT NULL PRIMARY KEY,
  "employeeId"             TEXT    NOT NULL REFERENCES "employees" ("id"),
  "payDate"                TEXT    NOT NULL CHECK ("payDate" = date("payDate")),
  "workState"              TEXT    NOT NULL CHECK (length("workState") = 2),
  "sutaState"              TEXT    NOT NULL CHECK (length("sutaState") = 2),
  "gross"                  INTEGER NOT NULL DEFAULT 0 CHECK ("gross" >= 0),
  "pretax401k"             INTEGER NOT NULL DEFAULT 0 CHECK ("pretax401k" >= 0),
  "pretaxSection125"       INTEGER NOT NULL DEFAULT 0 CHECK ("pretaxSection125" >= 0),
  "incomeTaxWages"         INTEGER NOT NULL DEFAULT 0 CHECK ("incomeTaxWages" >= 0),
  "federalIncomeTax"       INTEGER NOT NULL DEFAULT 0 CHECK ("federalIncomeTax" >= 0),
  "socialSecurityWages"    INTEGER NOT NULL DEFAULT 0 CHECK ("socialSecurityWages" >= 0),
  "socialSecurityEmployee" INTEGER NOT NULL DEFAULT 0 CHECK ("socialSecurityEmployee" >= 0),
  "socialSecurityEmployer" INTEGER NOT NULL DEFAULT 0 CHECK ("socialSecurityEmployer" >= 0),
  "medicareWages"          INTEGER NOT NULL DEFAULT 0 CHECK ("medicareWages" >= 0),
  "medicareEmployee"       INTEGER NOT NULL DEFAULT 0 CHECK ("medicareEmployee" >= 0),
  "medicareEmployer"       INTEGER NOT NULL DEFAULT 0 CHECK ("medicareEmployer" >= 0),
  "additionalMedicareWages" INTEGER NOT NULL DEFAULT 0 CHECK ("additionalMedicareWages" >= 0),
  "additionalMedicare"     INTEGER NOT NULL DEFAULT 0 CHECK ("additionalMedicare" >= 0),
  "futaWages"              INTEGER NOT NULL DEFAULT 0 CHECK ("futaWages" >= 0),
  "futa"                   INTEGER NOT NULL DEFAULT 0 CHECK ("futa" >= 0),
  "sutaWages"              INTEGER NOT NULL DEFAULT 0 CHECK ("sutaWages" >= 0),
  "suta"                   INTEGER NOT NULL DEFAULT 0 CHECK ("suta" >= 0),
  "stateIncomeTaxWages"    INTEGER NOT NULL DEFAULT 0 CHECK ("stateIncomeTaxWages" >= 0),
  "stateIncomeTax"         INTEGER NOT NULL DEFAULT 0 CHECK ("stateIncomeTax" >= 0),
  "notes"                  TEXT,
  "createdAt"              TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
  "updatedAt"              TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
) STRICT;
CREATE INDEX "IDX_us_payroll_prior_employee" ON "us_payroll_prior" ("employeeId", "payDate");
