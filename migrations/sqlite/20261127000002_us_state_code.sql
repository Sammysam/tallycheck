-- D66: the state withholding form's own status code or rate-table letter (Alabama A4 "0" / "MS",
-- Connecticut CT-W4 withholding code, New Jersey NJ-W4 rate table, ...). NULL = taken from the
-- filing status.
ALTER TABLE "employee_us_tax" ADD COLUMN "stateCode" TEXT
    CHECK ("stateCode" IS NULL OR (length("stateCode") BETWEEN 1 AND 8));
