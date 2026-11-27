-- D80: "ledger" lets a role see the ledger; posting and reversing journal entries, bank
-- reconciliation, stock adjustments, revaluations and bank transfers need "ledger:post" as well.
-- Every role that could post before keeps that ability, except the built-in Viewer role, which
-- was meant to be read-only.
UPDATE "roles"
   SET "permissions" = json_insert("permissions", '$[#]', 'ledger:post')
 WHERE "name" <> 'Viewer'
   AND EXISTS (SELECT 1 FROM json_each("roles"."permissions") WHERE "value" = 'ledger')
   AND NOT EXISTS (SELECT 1 FROM json_each("roles"."permissions") WHERE "value" = 'ledger:post');
