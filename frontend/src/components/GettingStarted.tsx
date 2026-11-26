import { useEffect, useState } from 'react';
import { Link } from 'react-router-dom';
import { CheckCircle2, Circle } from 'lucide-react';
import api from '../api/client';

/**
 * D73: a getting-started checklist on the administrator's dashboard. Each step ticks itself off
 * from the company's own data, so it always shows what is really left to do; it disappears when
 * everything is done, or when hidden.
 */

/** Set when the setup wizard was told the business has no opening balances to bring in. */
export const NO_OPENING_BALANCES = 'tc.onboarding.noOpeningBalances';
const HIDDEN = 'tc.onboarding.hidden';

interface Item { key: string; label: string; hint: string; to: string; done: boolean }

const read = (k: string) => {
  try { return localStorage.getItem(k); } catch { return null; }
};
const write = (k: string, v: string) => {
  try { localStorage.setItem(k, v); } catch { /* private window: shown again next time */ }
};

export default function GettingStarted() {
  const [items, setItems] = useState<Item[] | null>(null);
  const [hidden, setHidden] = useState(read(HIDDEN) === '1');

  useEffect(() => {
    if (hidden) return;
    // Each check is allowed to fail (a missing right, or not on the main PC): that step is left out.
    const get = (url: string, params?: Record<string, unknown>) => api.get(url, { params }).then((r) => r.data).catch(() => undefined);
    Promise.all([
      get('/settings'),
      get('/conversions'),
      get('/customers'),
      get('/invoices', { page: 1, limit: 1 }),
      get('/system/backup'),
      get('/users'),
    ]).then(([settings, conversions, customers, invoices, backup, users]) => {
      const list: Item[] = [];
      if (settings) {
        list.push({ key: 'company', label: 'Enter your company details', hint: 'Name, address and country — printed on your invoices.', to: '/company-setup', done: !!String(settings['company.name'] ?? '').trim() });
      }
      if (conversions !== undefined) {
        const moved = Array.isArray(conversions) && conversions.some((c: { status?: string }) => c.status !== 'REVERSED');
        list.push({ key: 'opening', label: 'Bring in your opening balances', hint: 'Bank balance, what customers owe you and what you owe — or mark it as a new business.', to: '/move-books', done: moved || read(NO_OPENING_BALANCES) === '1' });
      }
      if (customers !== undefined) {
        const n = Array.isArray(customers) ? customers.length : Number(customers?.total ?? 0);
        list.push({ key: 'customer', label: 'Add your first customer', hint: 'Who you sell to.', to: '/customers', done: n > 0 });
      }
      if (invoices !== undefined) {
        list.push({ key: 'invoice', label: 'Create and post your first invoice', hint: 'Then record the payment when the money comes in.', to: '/invoices', done: Number(invoices?.total ?? 0) > 0 });
      }
      if (backup) {
        list.push({ key: 'backup', label: 'Turn on automatic backups', hint: 'A copy of your books every day, in a folder you choose.', to: '/settings?tab=backup', done: !!backup?.settings?.enabled });
      }
      if (Array.isArray(users)) {
        list.push({ key: 'user', label: 'Add a second person', hint: 'An accountant, or a second administrator who can reset your password.', to: '/users', done: users.filter((u: { isActive?: boolean }) => u.isActive !== false).length > 1 });
      }
      setItems(list);
    });
  }, [hidden]);

  if (hidden || !items || items.length === 0) return null;
  const done = items.filter((i) => i.done).length;
  if (done === items.length) return null;

  return (
    <div className="mb-3 bg-white border border-[#e0e0e0] rounded-md" aria-label="Getting started">
      <div className="px-4 py-2.5 border-b border-[#eee] flex items-center justify-between gap-3">
        <div>
          <p className="text-sm font-semibold text-gray-800">Getting started</p>
          <p className="text-xs text-gray-500">{done} of {items.length} done</p>
        </div>
        <button type="button" className="btn-link text-xs" onClick={() => { write(HIDDEN, '1'); setHidden(true); }}>Hide this list</button>
      </div>
      <div className="h-1 bg-[#f1f1f1]"><div className="h-1 bg-orange-500 transition-all" style={{ width: `${(done / items.length) * 100}%` }} /></div>
      <ul className="divide-y divide-[#f1f1f1]">
        {items.map((i) => (
          <li key={i.key} className="px-4 py-2 flex items-center gap-3">
            {i.done ? <CheckCircle2 size={18} className="text-green-600 shrink-0" aria-label="Done" /> : <Circle size={18} className="text-gray-300 shrink-0" aria-label="To do" />}
            <div className="flex-1 min-w-0">
              <p className={`text-sm ${i.done ? 'text-gray-400 line-through' : 'text-gray-800'}`}>{i.label}</p>
              {!i.done && <p className="text-xs text-gray-500">{i.hint}</p>}
            </div>
            {!i.done && <Link to={i.to} className="btn btn-secondary whitespace-nowrap">Start</Link>}
          </li>
        ))}
      </ul>
    </div>
  );
}
