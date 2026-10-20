-- D18 (requested 2026-10-20): a payment can be voided. Its journal entry is reversed, the
-- invoice or bill gets the amount back as outstanding, and the payment row stays, marked VOID.
-- The reference had no way to undo a payment.
ALTER TABLE "payments" ADD COLUMN "status" TEXT NOT NULL DEFAULT 'POSTED' CHECK ("status" IN ('POSTED', 'VOID'));
