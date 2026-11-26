-- D68: employer charges a state bills on its unemployment wages besides the assigned rate
-- (e.g. California ETT 0.1%): [{"name": text, "rate": ppm}] on the state registration, and the
-- total of them a pay line used (ppm; NULL on lines made before D68).
ALTER TABLE "us_state_registrations" ADD COLUMN "sutaSurcharges" TEXT NOT NULL DEFAULT '[]'
    CHECK (json_valid("sutaSurcharges") AND json_type("sutaSurcharges") = 'array');
ALTER TABLE "us_payroll_lines" ADD COLUMN "sutaSurchargeRate" INTEGER
    CHECK ("sutaSurchargeRate" IS NULL OR "sutaSurchargeRate" >= 0);
