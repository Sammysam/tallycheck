/**
 * D59: reading the files another accounting system exports (QuickBooks, Xero, Tally, our own
 * template) for "Move books in". Reports from those systems have title lines above the
 * headings, group headings (a customer's name on its own row above that customer's invoices)
 * and "Total" rows; all of that is handled here, so the screen only maps columns.
 */
import { guessMapping, type ImportField } from './importFields';

export type SectionKey = 'accounts' | 'balances' | 'invoices' | 'bills' | 'customers' | 'vendors';

export interface Section {
  key: SectionKey;
  label: string;
  description: string;
  /** Where to find it in QuickBooks Online (the most common source). */
  where: string;
  fields: ImportField[];
  /** Field filled from a group heading when no column holds it (open invoices grouped by customer). */
  groupField?: string;
  sample: string[][];
  optional?: boolean;
}

const party = (who: 'customer' | 'vendor'): ImportField[] => [
  { key: 'name', label: 'Name', required: true, words: ['name', who, who === 'vendor' ? 'supplier' : 'client', 'display name', 'company', 'company name', 'full name'] },
  { key: 'email', label: 'Email', words: ['email', 'e-mail', 'mail'] },
  { key: 'phone', label: 'Phone', words: ['phone', 'phone numbers', 'mobile', 'telephone', 'tel'] },
  { key: 'address', label: 'Address', words: ['billing address', 'address', 'street', 'city'] },
  { key: 'taxId', label: 'Tax number', words: ['tax number', 'tax id', 'vat no', 'vat number', 'tin', 'gstin', 'tax reg no'] },
];

const openDoc = (who: 'customer' | 'vendor'): ImportField[] => [
  { key: 'number', label: who === 'customer' ? 'Invoice number' : 'Bill number', words: ['num', 'no', 'number', 'invoice no', 'invoice number', 'bill no', 'ref no', 'reference', 'invoice', 'bill'] },
  { key: who, label: who === 'customer' ? 'Customer' : 'Vendor', words: who === 'customer' ? ['customer', 'client', 'name', 'customer name'] : ['vendor', 'supplier', 'payee', 'name', 'vendor name'], hint: 'Leave it if the report lists each name on its own row above its documents.' },
  { key: 'date', label: 'Date', required: true, words: ['date', 'txn date', 'invoice date', 'bill date', 'transaction date'] },
  { key: 'dueDate', label: 'Due date', words: ['due date', 'due'] },
  { key: 'amount', label: 'Amount still unpaid', required: true, words: ['open balance', 'balance due', 'balance', 'amount due', 'outstanding', 'unpaid', 'open amount', 'amount'] },
];

export const SECTIONS: Section[] = [
  {
    key: 'balances',
    label: 'Trial balance',
    description: 'Every account’s balance on the cut-over date. It becomes the opening entry.',
    where: 'Reports → Trial Balance, dated the cut-over date → Export to Excel',
    fields: [
      { key: 'account', label: 'Account', required: true, words: ['account', 'account name', 'name', 'ledger', 'particulars', 'account code'] },
      { key: 'debit', label: 'Debit', words: ['debit', 'dr', 'debits'] },
      { key: 'credit', label: 'Credit', words: ['credit', 'cr', 'credits'] },
    ],
    sample: [
      ['Account', 'Debit', 'Credit'],
      ['Checking', '50,000.00', ''],
      ['Accounts Receivable (A/R)', '1,500.00', ''],
      ['Office Equipment', '10,000.00', ''],
      ['Accounts Payable (A/P)', '', '800.00'],
      ["Owner's Equity", '', '60,000.00'],
      ['Sales', '', '700.00'],
    ],
  },
  {
    key: 'accounts',
    label: 'Chart of accounts',
    description: 'The account list with each account’s type. Without it, the accounts come from the trial balance and you choose the types.',
    where: 'Settings → Chart of accounts → Run report → Export to Excel',
    optional: true,
    fields: [
      { key: 'account', label: 'Account', required: true, words: ['account', 'account name', 'name', 'full name'] },
      { key: 'number', label: 'Account number', words: ['account number', 'account no', 'number', 'code', 'account code', 'acct num'] },
      { key: 'type', label: 'Type', words: ['type', 'account type', 'category', 'group', 'class'] },
    ],
    sample: [
      ['Account', 'Type'],
      ['Checking', 'Bank'],
      ['Accounts Receivable (A/R)', 'Accounts receivable (A/R)'],
      ['Office Equipment', 'Fixed Assets'],
      ['Accounts Payable (A/P)', 'Accounts payable (A/P)'],
      ["Owner's Equity", 'Equity'],
      ['Sales', 'Income'],
    ],
  },
  {
    key: 'invoices',
    label: 'Unpaid invoices',
    description: 'Customer invoices not fully paid on the cut-over date, with what is still owed on each.',
    where: 'Reports → Open Invoices (or A/R Ageing Detail) → Export to Excel',
    optional: true,
    fields: openDoc('customer'),
    groupField: 'customer',
    sample: [
      ['Num', 'Customer', 'Date', 'Due date', 'Open balance'],
      ['1001', 'Lanka Traders', '15/09/2026', '15/10/2026', '1,000.00'],
      ['1002', 'Ceylon Tea', '20/10/2026', '19/11/2026', '500.00'],
    ],
  },
  {
    key: 'bills',
    label: 'Unpaid bills',
    description: 'Supplier bills not fully paid on the cut-over date, with what is still owed on each.',
    where: 'Reports → Unpaid Bills (or A/P Ageing Detail) → Export to Excel',
    optional: true,
    fields: openDoc('vendor'),
    groupField: 'vendor',
    sample: [
      ['Num', 'Vendor', 'Date', 'Due date', 'Open balance'],
      ['B-77', 'Colombo Packaging', '01/10/2026', '31/10/2026', '800.00'],
    ],
  },
  {
    key: 'customers',
    label: 'Customers',
    description: 'The full customer list with contact details (customers on unpaid invoices are added anyway).',
    where: 'Sales → Customers → Export to Excel',
    optional: true,
    fields: party('customer'),
    sample: [['Name', 'Email', 'Phone', 'Address'], ['Lanka Traders', 'accounts@lanka.example', '011 234 5678', '12 Main Street, Colombo']],
  },
  {
    key: 'vendors',
    label: 'Vendors',
    description: 'The full supplier list with contact details.',
    where: 'Expenses → Vendors → Export to Excel',
    optional: true,
    fields: party('vendor'),
    sample: [['Name', 'Email', 'Phone', 'Address'], ['Colombo Packaging', 'sales@packaging.example', '011 765 4321', '5 Mill Road, Colombo']],
  },
];

export const sectionInfo = (k: SectionKey) => SECTIONS.find((s) => s.key === k)!;

export const SOURCES = ['QuickBooks Online', 'QuickBooks Desktop', 'Xero', 'Zoho Books', 'Sage', 'TallyPrime', 'Excel / other system'];

const nonEmpty = (r: string[]) => r.filter((c) => (c ?? '').trim() !== '').length;

/** Whether a cell looks like data rather than a heading: an amount or a date. */
const looksLikeData = (c: string) => /^[\s$£€₹(+-]*[\d,]+(\.\d+)?\)?\s*$/.test(c) || /^\d{1,4}[/.-]\d{1,2}[/.-]\d{1,4}$/.test(c.trim());

/**
 * The row holding the column headings: of the top 20 rows, the one where the most fields find
 * a heading (rows with amounts or dates in them are data, not headings). Ties go to the first.
 */
export function findHeaderRow(rows: string[][], fields: ImportField[]): number {
  let best = -1;
  let bestScore = 0;
  for (let i = 0; i < Math.min(rows.length, 20); i++) {
    const cells = (rows[i] ?? []).map((c) => c ?? '');
    if (cells.some((c) => c.trim() !== '' && looksLikeData(c))) continue;
    const score = Object.keys(guessMapping(fields, cells)).length;
    if (score > bestScore) { best = i; bestScore = score; }
  }
  return best >= 0 ? best : Math.max(0, rows.findIndex((r) => nonEmpty(r) >= 2));
}

/**
 * The heading row and the column for each field. Reports often leave the first heading blank
 * (QuickBooks: ",Debit,Credit" and ",Date,Num,…"): a required field still unmatched then takes
 * that blank first column.
 */
export function guessColumns(section: Section, rows: string[][]): { headerRow: number; mapping: Record<string, number> } {
  const headerRow = findHeaderRow(rows, section.fields);
  const headings = (rows[headerRow] ?? []).map((c) => c ?? '');
  const mapping = guessMapping(section.fields, headings);
  const missing = section.fields.filter((f) => f.required && mapping[f.key] === undefined);
  if (missing.length === 1 && headings[0]?.trim() === '' && !Object.values(mapping).includes(0)) {
    mapping[missing[0].key] = 0;
  }
  return { headerRow, mapping };
}

const isTotal = (s: string) => /^\s*(total|grand total|net income|net profit)\b/i.test(s);

/**
 * The data rows as {field: text}. Skips blank rows, "Total …" rows and the report's own
 * footer; a row with a single filled cell is a group heading whose text fills `groupField`
 * for the rows under it when no column is mapped to that field.
 */
export function rowsOf(section: Section, rows: string[][], headerRow: number, mapping: Record<string, number>): Record<string, string>[] {
  const out: Record<string, string>[] = [];
  let group = '';
  const groupMapped = section.groupField !== undefined && mapping[section.groupField] !== undefined;
  for (const r of rows.slice(headerRow + 1)) {
    const filled = nonEmpty(r);
    if (filled === 0) continue;
    const first = (r.find((c) => (c ?? '').trim() !== '') ?? '').trim();
    if (isTotal(first)) continue;
    if (filled === 1 && section.groupField && !groupMapped) {
      group = first;
      continue;
    }
    const row: Record<string, string> = {};
    for (const [k, i] of Object.entries(mapping)) row[k] = (r[i] ?? '').trim();
    if (section.groupField && !groupMapped) row[section.groupField] = group;
    // A report's own footer ("Accrual basis Monday, 17 November 2026 …") has no money in it.
    if (Object.values(row).every((v) => v === '')) continue;
    // Money rows only: a footer split at its commas puts words in the amount columns.
    const amount = (v?: string) => !!v && looksLikeData(v);
    if (section.key === 'balances' && !amount(row.debit) && !amount(row.credit)) continue;
    if ((section.key === 'invoices' || section.key === 'bills') && !amount(row.amount)) continue;
    out.push(row);
  }
  return out;
}

export type AccountType = 'ASSET' | 'LIABILITY' | 'EQUITY' | 'REVENUE' | 'EXPENSE';

/** Our account type from another system's type names (QuickBooks, Xero, Tally groups). */
export function typeFrom(text: string): AccountType | '' {
  const t = text.toLowerCase();
  if (!t.trim()) return '';
  if (/(receivable|bank|cash|asset|inventory|stock|deposit|prepaid|current asset|debtors|fixed|equipment|undeposited)/.test(t)) return 'ASSET';
  if (/(payable|credit card|liabilit|loan|creditors|tax payable|accrued|overdraft)/.test(t)) return 'LIABILITY';
  if (/(equity|capital|retained|drawings|owner|reserves)/.test(t)) return 'EQUITY';
  if (/(income|revenue|sales|other income)/.test(t)) return 'REVENUE';
  if (/(expense|cost of|cogs|purchase|direct cost|indirect cost)/.test(t)) return 'EXPENSE';
  return '';
}

/** "1000 Checking" / "1000 · Checking" → code 1000, name Checking. */
export function splitNumber(account: string): { code: string; name: string } {
  const m = account.trim().match(/^(\d{3,6})\s*(?:[·:\-–]\s*)?(.+)$/);
  return m ? { code: m[1], name: m[2].trim() } : { code: '', name: account.trim() };
}

export interface ExistingAccount { id: string; code: string; name: string; type: string; isActive: boolean }

const norm = (s: string) => s.toLowerCase().replace(/[^a-z0-9]+/g, ' ').trim();

/** The TallyCheck account an old account most likely is, or none (create it). */
export function suggestAccount(key: string, typeText: string, accounts: ExistingAccount[]): ExistingAccount | undefined {
  const active = accounts.filter((a) => a.isActive);
  const { code, name } = splitNumber(key);
  const n = norm(name);
  const byName = active.find((a) => norm(a.name) === n);
  if (byName) return byName;
  if (code) {
    const byCode = active.find((a) => a.code === code && norm(a.name) === n);
    if (byCode) return byCode;
  }
  const both = `${n} ${typeText.toLowerCase()}`;
  if (/receivable|debtors|\ba r\b/.test(both)) return active.find((a) => a.code === '1100');
  if (/payable|creditors|\ba p\b/.test(both) && !/tax|vat|gst|salar|wage|payroll|credit card/.test(both)) return active.find((a) => a.code === '2000');
  return undefined;
}

const RANGE: Record<AccountType, [number, number]> = {
  ASSET: [1000, 1999], LIABILITY: [2000, 2999], EQUITY: [3000, 3999], REVENUE: [4000, 4999], EXPENSE: [5000, 9999],
};

/** A free code in the type's range (asset 1xxx … expense 5xxx–9xxx), stepping by 10. */
export function freeCode(type: AccountType | '', taken: Set<string>): string {
  const [lo, hi] = RANGE[type || 'ASSET'];
  for (let c = lo + 10; c <= hi; c += 10) if (!taken.has(String(c))) return String(c);
  for (let c = lo; c <= hi; c++) if (!taken.has(String(c))) return String(c);
  return '';
}

export function sampleCsv(s: Section): string {
  const cell = (v: string) => (/[",\n]/.test(v) ? `"${v.replace(/"/g, '""')}"` : v);
  return '﻿' + s.sample.map((r) => r.map(cell).join(',')).join('\r\n') + '\r\n';
}
