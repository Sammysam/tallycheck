-- D25 (requested 2026-10-20): the app is sold to any company, so no company's own details
-- ship with it. The baseline seeded the original customer's signatory and payroll details
-- (company name, EPF/ETF numbers, addresses, contact person, phone); on a fresh install —
-- no users yet — they are blanked so each customer enters their own. An installation that
-- is already in use keeps what is saved there: that is its owner's data.
UPDATE "settings" SET "value" = '', "updatedAt" = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
WHERE "key" IN (
    'signatoryName', 'signatoryTitle',
    'payroll.companyName', 'payroll.epfRegNo', 'payroll.etfRegNo', 'payroll.careOf',
    'payroll.epfAddress', 'payroll.etfAddress', 'payroll.telephone'
  )
  AND NOT EXISTS (SELECT 1 FROM "users");
