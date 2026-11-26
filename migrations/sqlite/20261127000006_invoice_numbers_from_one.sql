-- D72: a new company's first invoice is INV-0001. The reference seeded its own customer's
-- starting number (INV-9000); a company that already has invoices keeps its numbering.
UPDATE "counters" SET "value" = 0
WHERE "key" = 'INV' AND "value" = 8999 AND NOT EXISTS (SELECT 1 FROM "invoices");
