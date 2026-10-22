import { useEffect, useMemo, useState } from 'react';
import { AlertTriangle, Boxes, PackagePlus, Wrench } from 'lucide-react';
import api, { errMsg } from '../api/client';
import PageHeader from '../components/PageHeader';
import ActionMenu from '../components/ActionMenu';
import ConfirmDialog from '../components/ConfirmDialog';
import Dialog from '../components/desktop/Dialog';
import TxnLink from '../components/TxnLink';
import { useAuth } from '../contexts/AuthContext';
import { useCommand } from '../lib/commands';
import { money, amount } from '../lib/money';
import { fmtDate, todayIso } from '../lib/region';
import { lineCodes, useTaxCodes } from '../lib/taxes';
import { toast } from '../lib/toast';
import { qty, TYPE_LABEL, type ItemType } from '../lib/items';

/**
 * D38: products & services. Stock items are counted and valued at average cost: bills bring
 * stock in, posted invoices take it out (cost of goods sold), adjustments correct a count.
 */

interface Item {
  id: string; sku?: string | null; name: string; type: ItemType; description?: string | null;
  salePrice?: number | null; purchaseCost?: number | null;
  incomeAccountId?: string | null; expenseAccountId?: string | null; inventoryAccountId?: string | null; cogsAccountId?: string | null;
  taxCodeId?: string | null; reorderPoint?: number | null;
  quantityOnHand: number; stockValue: number; averageCost?: number | null; lowStock: boolean; isActive: boolean;
}
interface Account { id: string; code: string; name: string; type: string; currency?: string | null; isActive: boolean }
interface Move { id: string; date: string; quantity: number; value: number; sourceType: string; journalEntryId?: string | null; note?: string | null; number?: string | null }

const TYPE_HINT: Record<ItemType, string> = {
  INVENTORY: 'Counted and valued. Bills add stock, invoices take it out at average cost.',
  SERVICE: 'Work you sell or buy, such as delivery, repairs or consulting.',
  NON_INVENTORY: 'Goods you sell or buy without keeping count, e.g. made to order.',
};
const MOVE_LABEL: Record<string, string> = {
  BILL: 'Bought (bill)', BILL_VOID: 'Bill voided', INVOICE: 'Sold (invoice)', INVOICE_REVERSAL: 'Sale reversed', ADJUSTMENT: 'Adjustment',
};

export default function Items() {
  const { hasPermission } = useAuth();
  const canEdit = hasPermission('invoices') || hasPermission('bills');
  const canAdjust = hasPermission('ledger');
  const [items, setItems] = useState<Item[]>([]);
  const [accounts, setAccounts] = useState<Account[]>([]);
  const [type, setType] = useState<'' | ItemType>('');
  const [lowOnly, setLowOnly] = useState(false);
  const [showInactive, setShowInactive] = useState(false);
  const [editing, setEditing] = useState<Item | 'new' | null>(null);
  const [adjusting, setAdjusting] = useState<Item | null>(null);
  const [history, setHistory] = useState<Item | null>(null);
  const [deleting, setDeleting] = useState<Item | null>(null);

  const load = () => api.get('/items').then((r) => setItems(r.data));
  useEffect(() => {
    load();
    api.get('/accounts').then((r) => setAccounts(r.data));
  }, []);
  useCommand('new', () => { if (canEdit) setEditing('new'); });

  const shown = useMemo(() => items.filter((i) => (showInactive || i.isActive) && (!type || i.type === type) && (!lowOnly || i.lowStock)), [items, type, lowOnly, showInactive]);
  const stock = items.filter((i) => i.type === 'INVENTORY');
  const value = stock.reduce((s, i) => s + i.stockValue, 0);
  const low = items.filter((i) => i.lowStock).length;

  const toggleActive = async (i: Item) => {
    try {
      await api.patch(`/items/${i.id}`, { isActive: !i.isActive });
      toast(`${i.name} is now ${i.isActive ? 'inactive' : 'active'}.`);
      load();
    } catch (e) { toast(errMsg(e, 'Could not change it'), 'error'); }
  };
  const remove = async () => {
    if (!deleting) return;
    try {
      await api.delete(`/items/${deleting.id}`);
      toast(`${deleting.name} deleted.`);
      load();
    } catch (e) { toast(errMsg(e, 'Could not delete it'), 'error'); }
    setDeleting(null);
  };

  return (
    <div>
      <PageHeader
        title="Products & Services"
        subtitle="What you sell and buy. Stock items are counted and valued at average cost."
        action={canEdit && <button className="btn btn-primary" onClick={() => setEditing('new')}>+ New product or service</button>}
      />

      <div className="grid gap-3 mb-3" style={{ gridTemplateColumns: 'repeat(auto-fill, minmax(200px, 1fr))' }}>
        <div className="card !mb-0"><div className="text-xs text-gray-500 flex items-center gap-1.5"><Boxes size={14} /> Stock value</div><div className="text-lg font-semibold" data-testid="stock-value">{money(value)}</div><div className="text-xs text-gray-500">{stock.length} stock item{stock.length === 1 ? '' : 's'}, at average cost</div></div>
        <button type="button" className={`card !mb-0 text-left ${low ? 'hover:border-amber-400' : ''}`} onClick={() => low && setLowOnly(true)}>
          <div className="text-xs text-gray-500 flex items-center gap-1.5"><AlertTriangle size={14} /> Low on stock</div>
          <div className={`text-lg font-semibold ${low ? 'text-amber-700' : ''}`}>{low}</div>
          <div className="text-xs text-gray-500">{low ? 'At or below the reorder point — click to see them' : 'Nothing to reorder'}</div>
        </button>
        <div className="card !mb-0"><div className="text-xs text-gray-500 flex items-center gap-1.5"><Wrench size={14} /> Services & non-stock</div><div className="text-lg font-semibold">{items.length - stock.length}</div><div className="text-xs text-gray-500">Not counted</div></div>
      </div>

      <div className="card !py-2.5 mb-3 flex flex-wrap items-center gap-3 text-sm">
        <label className="flex items-center gap-2 text-xs text-gray-600">Type
          <select value={type} onChange={(e) => setType(e.target.value as '' | ItemType)} aria-label="Type">
            <option value="">All</option>
            {(Object.keys(TYPE_LABEL) as ItemType[]).map((t) => <option key={t} value={t}>{TYPE_LABEL[t]}</option>)}
          </select>
        </label>
        <label className="flex items-center gap-1.5 text-xs text-gray-700"><input type="checkbox" checked={lowOnly} onChange={(e) => setLowOnly(e.target.checked)} /> Low on stock only</label>
        <label className="flex items-center gap-1.5 text-xs text-gray-700"><input type="checkbox" checked={showInactive} onChange={(e) => setShowInactive(e.target.checked)} /> Show inactive</label>
      </div>

      {shown.length === 0 ? (
        <div className="card text-center py-10 text-sm text-gray-600">
          <PackagePlus size={28} className="mx-auto mb-2 text-gray-400" />
          {items.length ? 'Nothing matches these filters.' : 'No products or services yet. Add the things you sell, then pick them on invoices and bills.'}
        </div>
      ) : (
        <div className="rounded-md border border-[#e0e0e0] bg-white overflow-x-auto">
          <table className="min-w-full text-sm" data-testid="items-table">
            <thead className="bg-[#fafafa] text-xs text-gray-600"><tr className="text-left">
              <th className="px-3 py-2">Name</th><th className="px-3">Type</th>
              <th className="px-3 text-right">Sale price</th><th className="px-3 text-right">Cost</th>
              <th className="px-3 text-right">On hand</th><th className="px-3 text-right">Stock value</th><th className="px-3" /><th className="px-3" />
            </tr></thead>
            <tbody>
              {shown.map((i) => (
                <tr key={i.id} className={`border-t border-[#f0f0f0] ${i.isActive ? '' : 'text-gray-400'}`}>
                  <td className="px-3 py-2">
                    <button type="button" className="font-medium text-left hover:underline" onClick={() => (i.type === 'INVENTORY' ? setHistory(i) : canEdit && setEditing(i))}>{i.name}</button>
                    {i.sku && <span className="ml-2 text-xs text-gray-500">{i.sku}</span>}
                  </td>
                  <td className="px-3">{TYPE_LABEL[i.type]}</td>
                  <td className="px-3 text-right tabular-nums">{i.salePrice != null ? amount(i.salePrice) : '—'}</td>
                  <td className="px-3 text-right tabular-nums">{i.averageCost != null ? amount(i.averageCost) : i.purchaseCost != null ? amount(i.purchaseCost) : '—'}</td>
                  <td className="px-3 text-right tabular-nums">{i.type === 'INVENTORY' ? qty(i.quantityOnHand) : ''}</td>
                  <td className="px-3 text-right tabular-nums">{i.type === 'INVENTORY' ? amount(i.stockValue) : ''}</td>
                  <td className="px-3">
                    {i.lowStock && <span className="text-[11px] px-1.5 py-0.5 rounded border bg-amber-50 border-amber-200 text-amber-800">Low stock</span>}
                    {!i.isActive && <span className="text-[11px] px-1.5 py-0.5 rounded border bg-gray-50 border-gray-200 text-gray-600">Inactive</span>}
                  </td>
                  <td className="px-3 text-right">
                    <ActionMenu items={[
                      ...(canEdit ? [{ label: 'Edit…', onSelect: () => setEditing(i) }] : []),
                      ...(i.type === 'INVENTORY' && canAdjust && i.isActive ? [{ label: 'Adjust stock…', onSelect: () => setAdjusting(i) }] : []),
                      ...(i.type === 'INVENTORY' ? [{ label: 'Stock history…', onSelect: () => setHistory(i) }] : []),
                      ...(canEdit ? [{ label: i.isActive ? 'Make inactive' : 'Make active', onSelect: () => toggleActive(i) }] : []),
                      ...(canEdit ? [{ label: 'Delete…', danger: true, onSelect: () => setDeleting(i) }] : []),
                    ]} />
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}

      {editing && <ItemDialog item={editing === 'new' ? null : editing} accounts={accounts} onClose={() => setEditing(null)} onSaved={() => { setEditing(null); load(); }} />}
      {adjusting && <AdjustDialog item={adjusting} accounts={accounts} onClose={() => setAdjusting(null)} onSaved={() => { setAdjusting(null); load(); }} />}
      {history && <HistoryDialog item={history} onClose={() => setHistory(null)} />}
      {deleting && (
        <ConfirmDialog title="Delete product or service" confirmLabel="Delete" danger onConfirm={remove} onCancel={() => setDeleting(null)}>
          <p>Delete <b>{deleting.name}</b>?</p>
          <p className="text-gray-600">Only possible while no invoice, bill or stock movement uses it. Otherwise make it inactive.</p>
        </ConfirmDialog>
      )}
    </div>
  );
}

const numOrNull = (v: string) => (v.trim() === '' ? null : Number(v));

function ItemDialog({ item, accounts, onClose, onSaved }: { item: Item | null; accounts: Account[]; onClose: () => void; onSaved: () => void }) {
  const taxCodes = lineCodes(useTaxCodes(), item?.taxCodeId ?? undefined);
  const byType = (t: string) => accounts.filter((a) => a.isActive && !a.currency && a.type === t);
  const code = (c: string) => accounts.find((a) => a.code === c)?.id ?? '';
  const [f, setF] = useState({
    name: item?.name ?? '', sku: item?.sku ?? '', type: (item?.type ?? 'INVENTORY') as ItemType, description: item?.description ?? '',
    salePrice: item?.salePrice != null ? String(item.salePrice) : '', purchaseCost: item?.purchaseCost != null ? String(item.purchaseCost) : '',
    incomeAccountId: item?.incomeAccountId ?? code('4000'), expenseAccountId: item?.expenseAccountId ?? '',
    inventoryAccountId: item?.inventoryAccountId ?? code('1200'), cogsAccountId: item?.cogsAccountId ?? code('5000'),
    taxCodeId: item?.taxCodeId ?? '', reorderPoint: item?.reorderPoint != null ? String(item.reorderPoint) : '',
  });
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(false);
  const set = (patch: Partial<typeof f>) => setF((x) => ({ ...x, ...patch }));
  const stock = f.type === 'INVENTORY';
  const moved = !!item && item.type === 'INVENTORY' && (item.quantityOnHand > 0 || item.stockValue > 0);

  const save = async () => {
    setBusy(true);
    setError('');
    const body = {
      name: f.name, sku: f.sku || null, type: f.type, description: f.description || null,
      salePrice: numOrNull(f.salePrice), purchaseCost: numOrNull(f.purchaseCost),
      incomeAccountId: f.incomeAccountId || null, expenseAccountId: stock ? null : f.expenseAccountId || null,
      inventoryAccountId: stock ? f.inventoryAccountId || null : null, cogsAccountId: stock ? f.cogsAccountId || null : null,
      taxCodeId: f.taxCodeId || null, reorderPoint: stock ? numOrNull(f.reorderPoint) : null,
    };
    try {
      if (item) await api.patch(`/items/${item.id}`, body);
      else await api.post('/items', body);
      toast(`${f.name.trim()} saved.`);
      onSaved();
    } catch (e) {
      setError(errMsg(e, 'Could not save'));
    } finally {
      setBusy(false);
    }
  };

  const select = (label: string, key: keyof typeof f, list: Account[], blank: string, disabled = false) => (
    <label className="text-xs font-medium text-gray-600">{label}
      <select className="mt-1 w-full" value={f[key]} disabled={disabled} onChange={(e) => set({ [key]: e.target.value } as Partial<typeof f>)} aria-label={label}>
        <option value="">{blank}</option>
        {list.map((a) => <option key={a.id} value={a.id}>{a.code} – {a.name}</option>)}
      </select>
    </label>
  );

  return (
    <Dialog title={item ? `Edit ${item.name}` : 'New product or service'} width={620} onClose={() => !busy && onClose()}
      buttons={<>
        <button className="btn btn-primary" disabled={busy || !f.name.trim()} onClick={save}>{busy ? 'Saving…' : 'Save'}</button>
        <button className="btn btn-secondary" disabled={busy} onClick={onClose}>Cancel</button>
      </>}>
      {error && <div className="mb-3 px-3 py-2 rounded border border-red-200 bg-red-50 text-sm text-red-700" role="alert">{error}</div>}
      <div className="grid grid-cols-3 gap-2 mb-3">
        {(Object.keys(TYPE_LABEL) as ItemType[]).map((t) => (
          <button key={t} type="button" disabled={moved && t !== 'INVENTORY'} onClick={() => set({ type: t })} data-testid={`item-type-${t}`}
            className={`text-left p-2 rounded border text-xs ${f.type === t ? 'border-orange-500 bg-orange-50 ring-1 ring-orange-400' : 'border-[#e5e7eb] hover:border-orange-300'} disabled:opacity-40`}>
            <div className="font-semibold text-sm text-gray-900">{TYPE_LABEL[t]}</div>
            <div className="text-gray-600 mt-0.5">{TYPE_HINT[t]}</div>
          </button>
        ))}
      </div>
      <div className="grid gap-2" style={{ gridTemplateColumns: '2fr 1fr' }}>
        <label className="text-xs font-medium text-gray-600">Name
          <input className="mt-1 w-full" value={f.name} onChange={(e) => set({ name: e.target.value })} autoFocus aria-label="Name" />
        </label>
        <label className="text-xs font-medium text-gray-600">SKU / code
          <input className="mt-1 w-full" value={f.sku} onChange={(e) => set({ sku: e.target.value })} placeholder="optional" aria-label="SKU" />
        </label>
      </div>
      <label className="text-xs font-medium text-gray-600 block mt-2">Description on invoices
        <input className="mt-1 w-full" value={f.description} onChange={(e) => set({ description: e.target.value })} placeholder="optional — the name is used if blank" aria-label="Description" />
      </label>
      <div className="grid grid-cols-3 gap-2 mt-2">
        <label className="text-xs font-medium text-gray-600">Sale price
          <input className="mt-1 w-full" type="number" min="0" step="0.01" value={f.salePrice} onChange={(e) => set({ salePrice: e.target.value })} aria-label="Sale price" />
        </label>
        <label className="text-xs font-medium text-gray-600">Purchase cost
          <input className="mt-1 w-full" type="number" min="0" step="0.01" value={f.purchaseCost} onChange={(e) => set({ purchaseCost: e.target.value })} aria-label="Purchase cost" />
        </label>
        <label className="text-xs font-medium text-gray-600">Tax on sales
          <select className="mt-1 w-full" value={f.taxCodeId} onChange={(e) => set({ taxCodeId: e.target.value })} aria-label="Tax code">
            <option value="">No tax</option>
            {taxCodes.map((c) => <option key={c.id} value={c.id}>{c.name}</option>)}
          </select>
        </label>
      </div>
      <div className="grid grid-cols-2 gap-2 mt-2">
        {select('Income account (sales)', 'incomeAccountId', byType('REVENUE'), 'Choose…')}
        {stock
          ? <label className="text-xs font-medium text-gray-600">Reorder when at or below
              <input className="mt-1 w-full" type="number" min="0" step="any" value={f.reorderPoint} onChange={(e) => set({ reorderPoint: e.target.value })} placeholder="optional" aria-label="Reorder point" />
            </label>
          : select('Purchase account (bills)', 'expenseAccountId', [...byType('EXPENSE'), ...byType('ASSET')], 'Choose on each bill')}
        {stock && select('Inventory account', 'inventoryAccountId', byType('ASSET'), 'Choose…', moved)}
        {stock && select('Cost of goods sold account', 'cogsAccountId', byType('EXPENSE'), 'Choose…')}
      </div>
      {stock && !item && <p className="text-xs text-gray-500 mt-3">Stock starts at 0. Post a bill for it, or use <b>Adjust stock</b> to enter what you already have.</p>}
    </Dialog>
  );
}

function AdjustDialog({ item, accounts, onClose, onSaved }: { item: Item; accounts: Account[]; onClose: () => void; onSaved: () => void }) {
  const usable = accounts.filter((a) => a.isActive && !a.currency && a.id !== item.inventoryAccountId);
  const [date, setDate] = useState(todayIso());
  const [counted, setCounted] = useState('');
  const [unitCost, setUnitCost] = useState(item.averageCost != null ? String(item.averageCost) : item.purchaseCost != null ? String(item.purchaseCost) : '');
  const [accountId, setAccountId] = useState(accounts.find((a) => a.code === '5000')?.id ?? '');
  const [note, setNote] = useState('');
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(false);
  const change = counted.trim() === '' ? 0 : Math.round((Number(counted) - item.quantityOnHand) * 100) / 100;

  const save = async () => {
    setBusy(true);
    setError('');
    try {
      await api.post(`/items/${item.id}/adjust`, {
        date, quantity: change, accountId, note: note || undefined, unitCost: change > 0 && unitCost !== '' ? Number(unitCost) : undefined,
      });
      toast(`Stock of ${item.name} is now ${qty(Number(counted))}.`);
      onSaved();
    } catch (e) {
      setError(errMsg(e, 'Could not adjust the stock'));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog title={`Adjust stock — ${item.name}`} width={480} onClose={() => !busy && onClose()}
      buttons={<>
        <button className="btn btn-primary" disabled={busy || !change || !accountId || !date} onClick={save}>{busy ? 'Saving…' : 'Adjust stock'}</button>
        <button className="btn btn-secondary" disabled={busy} onClick={onClose}>Cancel</button>
      </>}>
      {error && <div className="mb-3 px-3 py-2 rounded border border-red-200 bg-red-50 text-sm text-red-700" role="alert">{error}</div>}
      <p className="text-sm text-gray-700 mb-3">On hand now: <b>{qty(item.quantityOnHand)}</b>, worth {money(item.stockValue)}.</p>
      <div className="grid grid-cols-2 gap-2">
        <label className="text-xs font-medium text-gray-600">Date
          <input className="mt-1 w-full" type="date" value={date} onChange={(e) => setDate(e.target.value)} aria-label="Date" />
        </label>
        <label className="text-xs font-medium text-gray-600">Quantity you counted
          <input className="mt-1 w-full" type="number" min="0" step="any" value={counted} onChange={(e) => setCounted(e.target.value)} autoFocus aria-label="Counted quantity" />
        </label>
      </div>
      {change !== 0 && <p className={`text-xs mt-2 ${change > 0 ? 'text-green-700' : 'text-amber-700'}`}>{change > 0 ? `${qty(change)} more than the books` : `${qty(-change)} fewer than the books`}</p>}
      {change > 0 && (
        <label className="text-xs font-medium text-gray-600 block mt-2">Cost of one unit
          <input className="mt-1 w-full" type="number" min="0" step="0.01" value={unitCost} onChange={(e) => setUnitCost(e.target.value)} aria-label="Unit cost" />
        </label>
      )}
      <label className="text-xs font-medium text-gray-600 block mt-2">{change > 0 ? 'Where the extra stock comes from' : 'Where the difference goes'}
        <select className="mt-1 w-full" value={accountId} onChange={(e) => setAccountId(e.target.value)} aria-label="Adjustment account">
          <option value="">Choose…</option>
          {usable.map((a) => <option key={a.id} value={a.id}>{a.code} – {a.name}</option>)}
        </select>
        <span className="text-[11px] text-gray-500 font-normal">Usually 5000 Cost of Goods Sold for losses and counts; an equity account for opening stock.</span>
      </label>
      <label className="text-xs font-medium text-gray-600 block mt-2">Note
        <input className="mt-1 w-full" value={note} onChange={(e) => setNote(e.target.value)} placeholder="e.g. Year-end stock count" aria-label="Note" />
      </label>
    </Dialog>
  );
}

function HistoryDialog({ item, onClose }: { item: Item; onClose: () => void }) {
  const [moves, setMoves] = useState<Move[] | null>(null);
  useEffect(() => { api.get(`/items/${item.id}/moves`).then((r) => setMoves(r.data)).catch(() => setMoves([])); }, [item.id]);
  return (
    <Dialog title={`Stock history — ${item.name}`} width={640} onClose={onClose} buttons={<button className="btn btn-secondary" onClick={onClose}>Close</button>}>
      <p className="text-sm text-gray-700 mb-2">On hand <b>{qty(item.quantityOnHand)}</b> · value {money(item.stockValue)}{item.averageCost != null && <> · average cost {money(item.averageCost)}</>}</p>
      <div className="max-h-[360px] overflow-auto border rounded">
        <table className="min-w-full text-sm" data-testid="stock-history">
          <thead className="bg-gray-50 sticky top-0 text-xs text-gray-600"><tr className="text-left">
            <th className="px-2 py-1.5">Date</th><th className="px-2">What</th><th className="px-2">Document</th><th className="px-2 text-right">Quantity</th><th className="px-2 text-right">Value</th>
          </tr></thead>
          <tbody>
            {moves === null && <tr><td colSpan={5} className="px-2 py-3 text-gray-500">Loading…</td></tr>}
            {moves?.length === 0 && <tr><td colSpan={5} className="px-2 py-3 text-gray-500">No stock movements yet.</td></tr>}
            {moves?.map((m) => (
              <tr key={m.id} className="border-t border-[#f3f3f3]">
                <td className="px-2 py-1 whitespace-nowrap">{fmtDate(m.date)}</td>
                <td className="px-2">{MOVE_LABEL[m.sourceType] ?? m.sourceType}{m.note && <span className="text-xs text-gray-500"> · {m.note}</span>}</td>
                <td className="px-2"><TxnLink entryId={m.journalEntryId}>{m.number ?? (m.journalEntryId ? 'Journal entry' : '—')}</TxnLink></td>
                <td className={`px-2 text-right tabular-nums ${m.quantity < 0 ? 'text-red-700' : 'text-green-700'}`}>{m.quantity > 0 ? '+' : ''}{qty(m.quantity)}</td>
                <td className="px-2 text-right tabular-nums">{amount(m.value)}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </Dialog>
  );
}
