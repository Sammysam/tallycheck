import { useState } from 'react';
import { Plus, Trash2 } from 'lucide-react';
import { money } from '../lib/money';
import { region } from '../lib/region';
import { freeCode, type ExistingAccount } from '../lib/conversionFiles';

/**
 * D59 quick start: books set up from a few typed figures instead of files. The person lists
 * their bank and cash balances, who owes them, whom they owe, other things they own and loans.
 * The form turns that into the same request "Move books in" sends for files: each customer /
 * supplier amount becomes one opening invoice / bill dated the cut-over day, and the difference
 * (what the business is worth) goes to 3900 Opening Balance Equity for the accountant to move
 * to the owner's capital or retained earnings.
 */

interface Row { name: string; amount: string; due?: string }
type ListKey = 'banks' | 'customers' | 'suppliers' | 'assets' | 'loans';

const LISTS: { key: ListKey; title: string; help: string; name: string; example: string; due?: boolean }[] = [
  { key: 'banks', title: 'Money in the bank and cash', help: 'Each bank account, cash box or wallet, with its balance on the cut-over day.', name: 'Account', example: 'e.g. BOC current account' },
  { key: 'customers', title: 'Customers who owe you', help: 'What each customer still owes you. One line per customer is enough. No due date? It is set to 30 days after the cut-over date.', name: 'Customer', example: 'e.g. Perera Stores', due: true },
  { key: 'suppliers', title: 'Suppliers you owe', help: 'What you still owe each supplier. No due date? It is set to 30 days after the cut-over date.', name: 'Supplier', example: 'e.g. Kandy Wholesale', due: true },
  { key: 'assets', title: 'Other things the business owns (if any)', help: 'Vehicles, equipment, stock, deposits paid… at their value today.', name: 'What', example: 'e.g. Delivery van' },
  { key: 'loans', title: 'Loans and other debts (if any)', help: 'Bank loans, credit cards, money owed to the owner…', name: 'What', example: 'e.g. Bank loan' },
];

/** Exact cents from text such as "1,234.50" or "250000"; null when it is not an amount. */
function toCents(text: string): number | null {
  const t = text.replace(/[, ]/g, '').trim();
  if (!t) return 0;
  if (!/^\d+(\.\d{1,2})?$/.test(t)) return null;
  const [i, f = ''] = t.split('.');
  return Number(i) * 100 + Number((f + '00').slice(0, 2));
}

const plain = (cents: number) => (cents / 100).toFixed(2);
/** D75: examples in the company's own country. */
const EXAMPLES: Record<string, Partial<Record<ListKey, string>>> = {
  US: { banks: 'e.g. Business checking', customers: 'e.g. Corner Cafe', suppliers: 'e.g. Main Street Supply' },
  LK: {},
  OTHER: { banks: 'e.g. Business current account', customers: 'e.g. City Traders', suppliers: 'e.g. Main Street Supply' },
};
const exampleFor = (key: ListKey, fallback: string) => {
  const c = region().country;
  return EXAMPLES[c === 'US' || c === 'LK' ? c : 'OTHER'][key] ?? fallback;
};

const blank = (): Row => ({ name: '', amount: '', due: '' });
const norm = (s: string) => s.trim().replace(/\s+/g, ' ').toLowerCase();

export interface QuickStartBody {
  accounts: Record<string, string>[];
  balances: { account: string; debit: string; credit: string }[];
  invoices: { customer: string; amount: string; dueDate: string }[];
  bills: { vendor: string; amount: string; dueDate: string }[];
  customers: never[];
  vendors: never[];
}

export default function QuickStartForm({ existing, busy, onBack, onCheck }: {
  existing: ExistingAccount[];
  busy: boolean;
  onBack: () => void;
  onCheck: (body: QuickStartBody) => void;
}) {
  const [lists, setLists] = useState<Record<ListKey, Row[]>>({
    banks: [blank()], customers: [blank()], suppliers: [blank()], assets: [], loans: [],
  });
  const set = (key: ListKey, i: number, patch: Partial<Row>) =>
    setLists((l) => ({ ...l, [key]: l[key].map((r, j) => (j === i ? { ...r, ...patch } : r)) }));
  const add = (key: ListKey) => setLists((l) => ({ ...l, [key]: [...l[key], blank()] }));
  const remove = (key: ListKey, i: number) => setLists((l) => ({ ...l, [key]: l[key].filter((_, j) => j !== i) }));

  const filled = (key: ListKey) => lists[key].filter((r) => r.name.trim() || r.amount.trim());
  const problems: string[] = [];
  const total = (key: ListKey) => filled(key).reduce((sum, r) => sum + (toCents(r.amount) ?? 0), 0);
  for (const l of LISTS) {
    const seen = new Set<string>();
    for (const r of filled(l.key)) {
      const c = toCents(r.amount);
      if (!r.name.trim()) problems.push(`${l.title}: a line has an amount but no name`);
      else if (c === null) problems.push(`${l.title}: "${r.amount}" for ${r.name} is not an amount (use digits, e.g. 25000.00)`);
      else if (c === 0) problems.push(`${l.title}: ${r.name} has no amount`);
      if (r.name.trim() && l.key !== 'customers' && l.key !== 'suppliers') {
        if (seen.has(norm(r.name))) problems.push(`${l.title}: ${r.name} is listed twice`);
        seen.add(norm(r.name));
      }
    }
  }
  const owns = total('banks') + total('customers') + total('assets');
  const owes = total('suppliers') + total('loans');
  const worth = owns - owes;
  const nothing = LISTS.every((l) => filled(l.key).length === 0);

  const build = (): QuickStartBody => {
    const active = existing.filter((a) => a.isActive);
    const taken = new Set(existing.map((a) => a.code));
    const accounts: Record<string, string>[] = [];
    const balances: QuickStartBody['balances'] = [];
    const named = (key: ListKey, prefix: string, type: 'ASSET' | 'LIABILITY', side: 'debit' | 'credit') => {
      for (const r of filled(key)) {
        const k = `${prefix}: ${r.name.trim()}`;
        const same = active.find((a) => norm(a.name) === norm(r.name) && a.type === type);
        if (same) accounts.push({ key: k, accountId: same.id });
        else {
          const code = freeCode(type, taken);
          taken.add(code);
          accounts.push({ key: k, code, name: r.name.trim(), type });
        }
        const c = plain(toCents(r.amount) ?? 0);
        balances.push({ account: k, debit: side === 'debit' ? c : '', credit: side === 'credit' ? c : '' });
      }
    };
    named('banks', 'Bank', 'ASSET', 'debit');
    named('assets', 'Asset', 'ASSET', 'debit');
    named('loans', 'Loan', 'LIABILITY', 'credit');
    const byCode = (code: string) => active.find((a) => a.code === code);
    if (total('customers') > 0) {
      accounts.push({ key: 'Owed by customers', accountId: byCode('1100')?.id ?? '' });
      balances.push({ account: 'Owed by customers', debit: plain(total('customers')), credit: '' });
    }
    if (total('suppliers') > 0) {
      accounts.push({ key: 'Owed to suppliers', accountId: byCode('2000')?.id ?? '' });
      balances.push({ account: 'Owed to suppliers', debit: '', credit: plain(total('suppliers')) });
    }
    if (worth !== 0) {
      const opening = byCode('3900');
      accounts.push(opening ? { key: 'Opening balance equity', accountId: opening.id } : { key: 'Opening balance equity', code: '3900', name: 'Opening Balance Equity', type: 'EQUITY' });
      balances.push({ account: 'Opening balance equity', debit: worth < 0 ? plain(-worth) : '', credit: worth > 0 ? plain(worth) : '' });
    }
    return {
      accounts,
      balances,
      invoices: filled('customers').map((r) => ({ customer: r.name.trim(), amount: plain(toCents(r.amount) ?? 0), dueDate: r.due ?? '' })),
      bills: filled('suppliers').map((r) => ({ vendor: r.name.trim(), amount: plain(toCents(r.amount) ?? 0), dueDate: r.due ?? '' })),
      customers: [],
      vendors: [],
    };
  };

  return (
    <section className="card">
      <h3 className="text-sm font-semibold text-gray-800 mb-1">Your figures on the cut-over day</h3>
      <p className="text-xs text-gray-600 mb-3">
        No files needed. Fill in what you know; leave out what doesn’t apply. You can add more customers, suppliers and
        accounts later in TallyCheck as usual.
      </p>
      <div className="space-y-4">
        {LISTS.map((l) => (
          <fieldset key={l.key} aria-label={l.title}>
            <legend className="text-sm font-semibold text-gray-800">{l.title}</legend>
            <p className="text-xs text-gray-500 mb-1">{l.help}</p>
            <table className="text-sm">
              <tbody>
                {lists[l.key].map((r, i) => (
                  <tr key={i}>
                    <td className="pr-2 py-0.5">
                      <input className="w-64" placeholder={exampleFor(l.key, l.example)} value={r.name} aria-label={`${l.title} ${l.name} ${i + 1}`} onChange={(e) => set(l.key, i, { name: e.target.value })} />
                    </td>
                    <td className="pr-2">
                      <input className="w-36 text-right" inputMode="decimal" placeholder="0.00" value={r.amount} aria-label={`${l.title} amount ${i + 1}`} onChange={(e) => set(l.key, i, { amount: e.target.value })} />
                    </td>
                    {l.due && (
                      <td className="pr-2 text-xs text-gray-500">
                        due <input type="date" className="text-xs" value={r.due ?? ''} aria-label={`${l.title} due date ${i + 1}`} onChange={(e) => set(l.key, i, { due: e.target.value })} />
                      </td>
                    )}
                    <td>
                      <button type="button" className="p-1 text-gray-400 hover:text-red-600" aria-label="Remove line" onClick={() => remove(l.key, i)}><Trash2 size={14} /></button>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
            <button type="button" className="mt-1 text-xs text-blue-700 hover:underline inline-flex items-center gap-1" onClick={() => add(l.key)}>
              <Plus size={12} /> Add a line
            </button>
          </fieldset>
        ))}
      </div>

      <div className="mt-4 rounded border border-gray-200 px-3 py-2 text-sm max-w-md">
        <div className="flex justify-between"><span>The business owns</span><span>{money(owns / 100)}</span></div>
        <div className="flex justify-between"><span>The business owes</span><span>{money(owes / 100)}</span></div>
        <div className="flex justify-between font-semibold border-t border-gray-200 mt-1 pt-1"><span>So it is worth</span><span>{money(worth / 100)}</span></div>
        <p className="text-xs text-gray-500 mt-1">This is the owner’s share of the business. TallyCheck keeps it in an account called Opening Balance Equity (3900); your accountant can later move it to the owner’s capital or retained earnings.</p>
      </div>

      {problems.length > 0 && (
        <ul className="mt-3 text-xs text-red-700 space-y-0.5">{problems.map((p, i) => <li key={i}>{p}</li>)}</ul>
      )}
      <div className="mt-3 flex gap-2">
        <button className="btn btn-secondary" onClick={onBack}>Back</button>
        <button className="btn btn-primary" disabled={busy || nothing || problems.length > 0} onClick={() => onCheck(build())}>
          {busy ? 'Checking…' : 'Next: check everything'}
        </button>
      </div>
    </section>
  );
}
