-- D13 (approved 2026-10-14): bills accumulate partial AP payments the way invoices do.
-- The reference has no such column; the importer back-fills it from the sum of each bill's
-- AP payments (the reference's own approach when it added invoices."amountPaid").
ALTER TABLE "bills" ADD COLUMN "amountPaid" INTEGER NOT NULL DEFAULT 0;
