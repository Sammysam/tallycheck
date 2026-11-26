-- D65: a dollar amount of exemptions on the employee's state certificate, deducted besides the
-- standard deduction (Mississippi Form 89-350 line 6: $6,000 single, $12,000 married, $1,500 per
-- dependent, and so on). Cents a year.
ALTER TABLE "employee_us_tax" ADD COLUMN "stateAnnualDeduction" INTEGER NOT NULL DEFAULT 0
    CHECK ("stateAnnualDeduction" >= 0);
