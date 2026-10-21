-- D33: a bill can be owed on any payable (liability) account, not only 2000 Accounts Payable,
-- and a payment can settle a payable account directly (no bill behind it).
ALTER TABLE "bills" ADD COLUMN "payableAccountId" TEXT REFERENCES "accounts" ("id");
ALTER TABLE "payments" ADD COLUMN "payableAccountId" TEXT REFERENCES "accounts" ("id");

-- D34: an account (a USD bank account) may be kept in a foreign currency. Its ledger lines carry
-- the foreign amount (signed: debit positive, credit negative) beside the home-currency debit /
-- credit. NULL on every line of a home-currency account.
ALTER TABLE "accounts" ADD COLUMN "currency" TEXT CHECK ("currency" IS NULL OR length("currency") = 3);
ALTER TABLE "journal_lines" ADD COLUMN "foreignAmount" INTEGER;
