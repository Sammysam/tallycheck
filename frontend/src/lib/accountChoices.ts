/**
 * D76: which accounts a picker offers first. A newcomer should not be offered Accounts
 * Receivable, the bank or a payroll liability as the account for a utility bill; every list
 * keeps a "Show all accounts" way out, and the account already chosen is always kept.
 */

export interface PickAccount { id: string; code: string; name: string; type: string; isActive?: boolean }

/** Accounts payroll posts to by itself (salaries, payroll taxes and what is withheld). */
export const PAYROLL = /withh|payroll|income tax|social security|medicare|futa|suta|unemployment|epf|etf|paye|stamp duty|401|section 125|disability|paid leave|local income|salar|wage|staff loan/i;
export const isPayroll = (a: PickAccount) => PAYROLL.test(a.name);

/** Cash, bank and receivable accounts (codes below 1200, or named so). */
const isMoneyOrReceivable = (a: PickAccount) =>
  a.type === 'ASSET' && (Number(a.code) < 1200 || /receivable|bank|cash|clearing|paypal|stripe/i.test(a.name));

/** What a bill line or a product's purchases usually go to: expenses, never payroll. */
export const isSpending = (a: PickAccount) => a.type === 'EXPENSE' && !isPayroll(a);

/** What a bill may be owed on: liabilities other than payroll's own. */
export const isPayable = (a: PickAccount) => a.type === 'LIABILITY' && !isPayroll(a);

/** Assets a purchase may go to (equipment, prepaid, stock), not money or receivables. */
export const isPurchasedAsset = (a: PickAccount) => a.type === 'ASSET' && !isMoneyOrReceivable(a);

/** The suggested accounts, plus the one already chosen even if it is not suggested. */
export function suggested<T extends PickAccount>(all: T[], ok: (a: T) => boolean, chosen?: string | null): T[] {
  return all.filter((a) => (a.isActive !== false && ok(a)) || a.id === chosen);
}

/** A free code for a new expense account: 6200, 6300 … 6900, then 6210, 6220 … */
export function freeExpenseCode(taken: Set<string>): string {
  for (let c = 6200; c <= 6900; c += 100) if (!taken.has(String(c))) return String(c);
  for (let c = 6200; c <= 6990; c += 10) if (!taken.has(String(c))) return String(c);
  for (let c = 6000; c <= 6999; c++) if (!taken.has(String(c))) return String(c);
  return '';
}
