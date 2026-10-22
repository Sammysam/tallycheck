/** D37: what each kind of import reads, and how a file's column headings are recognised. */

export type ImportKind = 'BANK' | 'INVOICES' | 'CUSTOMERS' | 'VENDORS' | 'OPENING';
export type DateOrder = 'DMY' | 'MDY' | 'YMD';

export interface ImportField {
  key: string;
  label: string;
  required?: boolean;
  /** Heading words that usually mean this field (lower case). */
  words: string[];
  hint?: string;
}

export interface KindInfo {
  kind: ImportKind;
  label: string;
  description: string;
  /** Any one of these permissions allows it. */
  permission: string;
  fields: ImportField[];
  sample: string[][];
}

export const KINDS: KindInfo[] = [
  {
    kind: 'BANK',
    label: 'Bank statement',
    description: 'Lines from your bank, card or PayPal statement. They wait on the Bank Transactions screen until you match each one.',
    permission: 'banking',
    fields: [
      { key: 'date', label: 'Date', required: true, words: ['date', 'txn date', 'transaction date', 'value date', 'posting date', 'posted'] },
      { key: 'description', label: 'Description', required: true, words: ['description', 'details', 'narration', 'particulars', 'memo', 'remarks', 'transaction'] },
      { key: 'reference', label: 'Reference', words: ['reference', 'ref', 'cheque', 'check no', 'doc no', 'transaction id'] },
      { key: 'amount', label: 'Amount (+ in, − out)', words: ['amount', 'net', 'value'], hint: 'One column, money out shown as minus. Or use Money in / Money out below.' },
      { key: 'moneyIn', label: 'Money in', words: ['money in', 'paid in', 'deposit', 'deposits', 'credit', 'credits', 'cr'] },
      { key: 'moneyOut', label: 'Money out', words: ['money out', 'paid out', 'withdrawal', 'withdrawals', 'debit', 'debits', 'dr'] },
    ],
    sample: [
      ['Date', 'Description', 'Reference', 'Money in', 'Money out'],
      ['05/10/2026', 'Deposit from ABC Traders', 'TRF-1001', '25,000.00', ''],
      ['06/10/2026', 'Bank charges', '', '', '250.00'],
    ],
  },
  {
    kind: 'INVOICES',
    label: 'Sales invoices',
    description: 'Sales from a shop, an online store or another system. Rows with the same invoice number become one invoice. They arrive as drafts.',
    permission: 'invoices',
    fields: [
      { key: 'number', label: 'Invoice / order number', words: ['invoice no', 'invoice number', 'invoice', 'order no', 'order number', 'order', 'number', 'no'], hint: 'Rows with the same number become one invoice.' },
      { key: 'customer', label: 'Customer', required: true, words: ['customer', 'client', 'bill to', 'buyer', 'name'] },
      { key: 'date', label: 'Date', required: true, words: ['date', 'invoice date', 'order date'] },
      { key: 'dueDate', label: 'Due date', words: ['due date', 'due'] },
      { key: 'description', label: 'Item / description', required: true, words: ['description', 'item', 'product', 'service', 'details'] },
      { key: 'quantity', label: 'Quantity', words: ['quantity', 'qty', 'units'] },
      { key: 'unitPrice', label: 'Unit price', required: true, words: ['unit price', 'price', 'rate', 'amount'] },
      { key: 'taxCode', label: 'Tax code', words: ['tax code', 'tax', 'vat', 'gst'], hint: 'The name of one of your tax codes.' },
      { key: 'account', label: 'Revenue account code', words: ['account', 'account code', 'gl'] },
    ],
    sample: [
      ['Invoice No', 'Customer', 'Date', 'Item', 'Qty', 'Price'],
      ['S-1001', 'ABC Traders', '05/10/2026', 'Tea 400g', '10', '850.00'],
      ['S-1001', 'ABC Traders', '05/10/2026', 'Delivery', '1', '500.00'],
    ],
  },
  {
    kind: 'CUSTOMERS',
    label: 'Customers',
    description: 'Your customer list from Excel or another system. Names already in the app are skipped.',
    permission: 'customers',
    fields: partyFields(),
    sample: [
      ['Name', 'Email', 'Phone', 'Address', 'Tax number'],
      ['ABC Traders', 'accounts@abc.lk', '0112 345 678', '12 Main Street, Colombo', '123456789'],
    ],
  },
  {
    kind: 'VENDORS',
    label: 'Vendors',
    description: 'Your supplier list. Names already in the app are skipped.',
    permission: 'vendors',
    fields: partyFields(),
    sample: [
      ['Name', 'Email', 'Phone', 'Address', 'Tax number'],
      ['Paper Co', 'sales@paperco.lk', '0114 222 333', '5 Mill Road, Kandy', ''],
    ],
  },
  {
    kind: 'OPENING',
    label: 'Opening balances',
    description: 'Account balances on the day you start using the app. They open as a journal entry for you to check and post.',
    permission: 'ledger',
    fields: [
      { key: 'account', label: 'Account code', required: true, words: ['account code', 'code', 'account', 'gl'] },
      { key: 'debit', label: 'Debit', words: ['debit', 'dr'] },
      { key: 'credit', label: 'Credit', words: ['credit', 'cr'] },
      { key: 'description', label: 'Note', words: ['description', 'note', 'name', 'account name'] },
    ],
    sample: [
      ['Account code', 'Account name', 'Debit', 'Credit'],
      ['1000', 'Cash', '150,000.00', ''],
      ['3000', "Owner's Equity", '', '150,000.00'],
    ],
  },
];

function partyFields(): ImportField[] {
  return [
    { key: 'name', label: 'Name', required: true, words: ['name', 'customer', 'vendor', 'supplier', 'company', 'business name'] },
    { key: 'email', label: 'Email', words: ['email', 'e-mail', 'mail'] },
    { key: 'phone', label: 'Phone', words: ['phone', 'mobile', 'telephone', 'tel', 'contact no'] },
    { key: 'address', label: 'Address', words: ['address', 'street', 'city'] },
    { key: 'taxId', label: 'Tax number', words: ['tax number', 'tax id', 'vat no', 'vat number', 'tin', 'brn'] },
  ];
}

export const kindInfo = (k: ImportKind) => KINDS.find((x) => x.kind === k)!;

const clean = (s: string) => s.toLowerCase().replace(/[^a-z0-9 ]+/g, ' ').replace(/\s+/g, ' ').trim();

/** Picks a column for each field from the headings: exact words first, then headings that contain them. */
export function guessMapping(fields: ImportField[], headings: string[]): Record<string, number> {
  const h = headings.map(clean);
  const used = new Set<number>();
  const out: Record<string, number> = {};
  for (const pass of ['exact', 'contains'] as const) {
    for (const f of fields) {
      if (out[f.key] !== undefined) continue;
      for (const w of f.words) {
        const i = h.findIndex((x, j) => !used.has(j) && x !== '' && (pass === 'exact' ? x === w : (w.length > 2 && x.includes(w))));
        if (i >= 0) { out[f.key] = i; used.add(i); break; }
      }
    }
  }
  // A statement with Money in / Money out columns has no single amount column.
  if (out.moneyIn !== undefined || out.moneyOut !== undefined) {
    if (out.amount !== undefined && (out.amount === out.moneyIn || out.amount === out.moneyOut)) delete out.amount;
  }
  return out;
}

/** The day / month order the dates in a column use, from the values themselves. */
export function guessDateOrder(values: string[], fallback: DateOrder): DateOrder {
  let dmy = false, mdy = false, ymd = false;
  for (const v of values) {
    const m = v.trim().match(/^(\d{1,4})[/.\- ](\d{1,2})[/.\- ](\d{1,4})/);
    if (!m) continue;
    const [a, b] = [Number(m[1]), Number(m[2])];
    if (m[1].length === 4) ymd = true;
    else if (a > 12) dmy = true;
    else if (b > 12) mdy = true;
  }
  if (ymd && !dmy && !mdy) return 'YMD';
  if (dmy && !mdy) return 'DMY';
  if (mdy && !dmy) return 'MDY';
  return fallback;
}

export function sampleCsv(k: KindInfo): string {
  const cell = (v: string) => (/[",\n]/.test(v) ? `"${v.replace(/"/g, '""')}"` : v);
  return '﻿' + k.sample.map((r) => r.map(cell).join(',')).join('\r\n') + '\r\n';
}

/** Column letters as Excel shows them: A, B, … Z, AA. */
export function columnLetter(i: number): string {
  let s = '';
  for (let n = i + 1; n > 0; n = Math.floor((n - 1) / 26)) s = String.fromCharCode(65 + ((n - 1) % 26)) + s;
  return s;
}
