import { useEffect, useState } from 'react';
import api, { errMsg } from '../api/client';
import PageHeader from '../components/PageHeader';
import Table from '../components/Table';
import StatusBadge from '../components/StatusBadge';
import Dialog from '../components/desktop/Dialog';
import { useCommand } from '../lib/commands';
import { region, regionFromSettings, setRegion } from '../lib/region';
import type { TaxCode, TaxKind } from '../lib/taxes';

/**
 * D26/D27: the company's own tax codes. Nothing about any country's tax law is built in:
 * - Standard: one rate (VAT, GST, sales tax, zero-rated, exempt);
 * - Group: several standard codes charged together (state + county + city), optionally compound;
 * - Withholding: tax a payer holds back from a payment for the tax office.
 */

interface Account { id: string; code: string; name: string; type: string; isActive: boolean }
interface Part { taxCodeId: string; isCompound: boolean }

const KIND_LABEL: Record<TaxKind, string> = { STANDARD: 'Standard', GROUP: 'Group', WITHHOLDING: 'Withholding' };
const emptyForm = {
  name: '', kind: 'STANDARD' as TaxKind, rate: '', description: '', salesAccountId: '', purchaseAccountId: '',
  isActive: true, components: [] as Part[],
};

export default function Taxes() {
  const [codes, setCodes] = useState<TaxCode[]>([]);
  const [accounts, setAccounts] = useState<Account[]>([]);
  const [form, setForm] = useState(emptyForm);
  const [editing, setEditing] = useState<string | null>(null);
  const [showForm, setShowForm] = useState(false);
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(false);
  const [deleting, setDeleting] = useState<TaxCode | null>(null);
  const [defaults, setDefaults] = useState({ sales: region().defaultSalesTax, purchases: region().defaultPurchaseTax });

  const load = () => api.get('/tax-codes').then((r) => setCodes(r.data));
  const loadAccounts = () => api.get('/accounts').then((r) => setAccounts(r.data));
  useEffect(() => {
    load();
    loadAccounts();
  }, []);

  const active = (type: string) => accounts.filter((a) => a.type === type && a.isActive);
  const byId = (id: string) => accounts.find((a) => a.id === id);
  const label = (id: string) => {
    const a = byId(id);
    return a ? `${a.code} ${a.name}` : '—';
  };
  const codeName = (id: string) => codes.find((c) => c.id === id)?.name ?? '?';
  const standards = codes.filter((c) => c.kind === 'STANDARD' && c.id !== editing);

  /** Accounts for a kind: [sales-side choices, purchase-side choices]. */
  const choices = (kind: TaxKind): [Account[], Account[]] =>
    kind === 'WITHHOLDING'
      ? [active('ASSET'), active('LIABILITY')]
      : [active('LIABILITY'), [...active('ASSET'), ...active('LIABILITY')]];

  const guess = (kind: TaxKind) => {
    const [sales, purchase] = choices(kind);
    const match = (list: Account[], re: RegExp) => list.find((a) => re.test(a.name))?.id ?? '';
    return kind === 'WITHHOLDING'
      ? { salesAccountId: match(sales, /withh|wht/i), purchaseAccountId: match(purchase, /withh|wht/i) }
      : { salesAccountId: match(sales, /tax|vat|gst/i), purchaseAccountId: match(purchase, /tax|vat|gst/i) };
  };

  const startNew = () => {
    setEditing(null);
    setError('');
    setForm({ ...emptyForm, ...guess('STANDARD') });
    setShowForm(true);
  };
  useCommand('new', startNew);
  useCommand('close', () => setShowForm(false));

  const chooseKind = (kind: TaxKind) => setForm((f) => ({ ...f, kind, ...guess(kind), components: [] }));

  const startEdit = (c: TaxCode) => {
    setEditing(c.id);
    setError('');
    setForm({
      name: c.name, kind: c.kind, rate: String(c.rate), description: c.description ?? '',
      salesAccountId: c.salesAccountId, purchaseAccountId: c.purchaseAccountId, isActive: c.isActive,
      components: c.components,
    });
    setShowForm(true);
  };

  /** Adds the accounts a kind posts to, if the chart does not have them yet. */
  const createAccounts = async () => {
    setBusy(true);
    setError('');
    try {
      const taken = new Set(accounts.map((a) => a.code));
      const pick = (preferred: string[]) => preferred.find((c) => !taken.has(c)) ?? '';
      const pair = form.kind === 'WITHHOLDING'
        ? [
          { code: pick(['1450', '1460', '1470']), name: 'Withholding Tax Receivable', type: 'ASSET', subtype: 'current_asset' },
          { code: pick(['2350', '2360', '2370']), name: 'Withholding Tax Payable', type: 'LIABILITY', subtype: 'current_liability' },
        ]
        : [
          { code: pick(['2300', '2310', '2390']), name: 'Tax Payable', type: 'LIABILITY', subtype: 'current_liability' },
          { code: pick(['1400', '1410', '1490']), name: 'Tax Receivable', type: 'ASSET', subtype: 'current_asset' },
        ];
      const made = await Promise.all(pair.map((a) => api.post('/accounts', a)));
      await loadAccounts();
      const [first, second] = made.map((r) => r.data.id);
      setForm((f) => (f.kind === 'WITHHOLDING'
        ? { ...f, salesAccountId: first, purchaseAccountId: second }
        : { ...f, salesAccountId: first, purchaseAccountId: second }));
    } catch (e) {
      setError(errMsg(e, 'Could not add the tax accounts'));
    } finally {
      setBusy(false);
    }
  };

  const togglePart = (id: string, on: boolean) =>
    setForm((f) => ({
      ...f,
      components: on ? [...f.components, { taxCodeId: id, isCompound: false }] : f.components.filter((p) => p.taxCodeId !== id),
    }));
  const setCompound = (id: string, on: boolean) =>
    setForm((f) => ({ ...f, components: f.components.map((p) => (p.taxCodeId === id ? { ...p, isCompound: on } : p)) }));

  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    setBusy(true);
    setError('');
    try {
      const common = { name: form.name.trim(), description: form.description.trim() || null, isActive: form.isActive };
      const body = form.kind === 'GROUP'
        ? { ...common, kind: form.kind, components: form.components }
        : {
          ...common, kind: form.kind, rate: Number(form.rate),
          salesAccountId: form.salesAccountId, purchaseAccountId: form.purchaseAccountId,
        };
      if (editing) await api.patch(`/tax-codes/${editing}`, body);
      else await api.post('/tax-codes', body);
      setShowForm(false);
      load();
    } catch (err) {
      setError(errMsg(err, 'Could not save the tax code'));
    } finally {
      setBusy(false);
    }
  };

  const toggleActive = (c: TaxCode) =>
    api.patch(`/tax-codes/${c.id}`, { isActive: !c.isActive }).then(load).catch((e) => setError(errMsg(e, 'Could not change it')));

  const saveDefaults = async (next: { sales: string; purchases: string }) => {
    setDefaults(next);
    try {
      const r = await api.patch('/settings', { 'tax.defaultSales': next.sales, 'tax.defaultPurchases': next.purchases });
      setRegion(regionFromSettings(r.data));
    } catch (e) {
      setError(errMsg(e, 'Could not save the default tax codes'));
    }
  };

  const lineCodes = codes.filter((c) => c.isActive && c.kind !== 'WITHHOLDING');
  const [salesChoices, purchaseChoices] = choices(form.kind);
  const hasAccounts = form.kind === 'WITHHOLDING'
    ? accounts.some((a) => /withh|wht/i.test(a.name))
    : accounts.some((a) => a.type === 'LIABILITY' && /tax|vat|gst/i.test(a.name));

  return (
    <div>
      <PageHeader
        title="Taxes"
        subtitle="Your tax codes — VAT, GST, sales tax, groups of taxes and withholding tax — and where each posts."
        action={<button className="btn btn-primary" onClick={startNew}>+ New Tax Code</button>}
      />
      {error && <div className="mb-3 px-3 py-2 bg-red-50 border border-red-200 text-red-700 text-sm rounded">{error}</div>}

      {showForm && (
        <div className="card mb-4" style={{ maxWidth: 820 }}>
          <form onSubmit={submit} className="form-grid" style={{ gridTemplateColumns: '2fr 1fr 1fr' }} aria-label="Tax code">
            <label className="text-xs font-medium text-gray-600">
              Name (as printed on invoices)
              <input className="mt-1" required maxLength={60} placeholder="e.g. VAT 18%" value={form.name}
                onChange={(e) => setForm({ ...form, name: e.target.value })} />
            </label>
            <label className="text-xs font-medium text-gray-600">
              Kind
              <select className="mt-1" value={form.kind} disabled={!!editing} onChange={(e) => chooseKind(e.target.value as TaxKind)} aria-label="Kind">
                <option value="STANDARD">Standard — one rate</option>
                <option value="GROUP">Group — several taxes together</option>
                <option value="WITHHOLDING">Withholding — held back from payments</option>
              </select>
            </label>
            {form.kind !== 'GROUP' ? (
              <label className="text-xs font-medium text-gray-600">
                Rate %
                <input className="mt-1" required type="number" min="0" max="100" step="0.0001" placeholder="e.g. 18" value={form.rate}
                  onChange={(e) => setForm({ ...form, rate: e.target.value })} />
              </label>
            ) : <span />}

            {form.kind === 'GROUP' ? (
              <div style={{ gridColumn: '1 / -1' }}>
                <p className="text-xs font-medium text-gray-600 mb-1">Taxes in this group, in the order they are charged</p>
                {standards.length < 2 && <p className="text-xs text-gray-500">Create at least two standard tax codes first.</p>}
                <div className="space-y-1">
                  {standards.map((c) => {
                    const part = form.components.find((p) => p.taxCodeId === c.id);
                    const position = form.components.findIndex((p) => p.taxCodeId === c.id);
                    return (
                      <div key={c.id} className="flex items-center gap-4 text-sm">
                        <label className="flex items-center gap-2 w-64">
                          <input type="checkbox" checked={!!part} onChange={(e) => togglePart(c.id, e.target.checked)} aria-label={`Include ${c.name}`} />
                          {part ? <span className="text-xs text-gray-500 w-4">{position + 1}.</span> : <span className="w-4" />}
                          {c.name} <span className="text-gray-500">({c.rate}%)</span>
                        </label>
                        {part && position > 0 && (
                          <label className="flex items-center gap-2 text-xs text-gray-600">
                            <input type="checkbox" checked={part.isCompound} onChange={(e) => setCompound(c.id, e.target.checked)} />
                            Charged on the price plus the taxes above it (compound)
                          </label>
                        )}
                      </div>
                    );
                  })}
                </div>
              </div>
            ) : (
              <>
                <label className="text-xs font-medium text-gray-600" style={{ gridColumn: 'span 1' }}>
                  {form.kind === 'WITHHOLDING' ? 'Withheld by customers goes to (an asset)' : 'Tax on sales goes to (a liability)'}
                  <select className="mt-1" required value={form.salesAccountId} onChange={(e) => setForm({ ...form, salesAccountId: e.target.value })} aria-label="Sales tax account">
                    <option value="">Choose an account…</option>
                    {salesChoices.map((a) => <option key={a.id} value={a.id}>{a.code} — {a.name}</option>)}
                  </select>
                </label>
                <label className="text-xs font-medium text-gray-600" style={{ gridColumn: 'span 2' }}>
                  {form.kind === 'WITHHOLDING' ? 'Withheld from vendors goes to (a liability)' : 'Tax on purchases goes to'}
                  <select className="mt-1" required value={form.purchaseAccountId} onChange={(e) => setForm({ ...form, purchaseAccountId: e.target.value })} aria-label="Purchase tax account">
                    <option value="">Choose an account…</option>
                    {purchaseChoices.map((a) => <option key={a.id} value={a.id}>{a.code} — {a.name}</option>)}
                  </select>
                </label>
                {!hasAccounts && (
                  <p className="text-xs text-gray-600" style={{ gridColumn: '1 / -1' }}>
                    No {form.kind === 'WITHHOLDING' ? 'withholding' : ''} tax accounts in the chart yet.{' '}
                    <button type="button" className="btn-link" disabled={busy} onClick={createAccounts}>
                      {form.kind === 'WITHHOLDING' ? 'Add Withholding Tax Receivable and Payable' : 'Add Tax Payable and Tax Receivable'}
                    </button>
                  </p>
                )}
              </>
            )}
            <label className="text-xs font-medium text-gray-600" style={{ gridColumn: '1 / -1' }}>
              Note (optional)
              <input className="mt-1" maxLength={200} value={form.description} onChange={(e) => setForm({ ...form, description: e.target.value })} />
            </label>
            <div className="flex gap-2" style={{ gridColumn: '1 / -1' }}>
              <button type="submit" className="btn btn-primary" disabled={busy}>{editing ? 'Save changes' : 'Save Tax Code'}</button>
              <button type="button" className="btn btn-secondary" onClick={() => setShowForm(false)}>Cancel</button>
            </div>
            {editing && (
              <p className="text-xs text-gray-500" style={{ gridColumn: '1 / -1' }}>
                Changes apply to new documents only; saved invoices and bills keep the tax they were raised with.
              </p>
            )}
          </form>
        </div>
      )}

      <Table
        data={codes}
        emptyMessage="No tax codes yet. Add one for each rate you charge or pay — or none if you are not registered for tax."
        columns={[
          { header: 'Name', accessor: 'name' },
          { header: 'Kind', accessor: (c) => KIND_LABEL[c.kind] ?? c.kind },
          { header: 'Rate', accessor: (c) => `${c.rate}%` },
          { header: 'Posts to', accessor: (c) => (c.kind === 'GROUP'
            ? c.components.map((p) => `${codeName(p.taxCodeId)}${p.isCompound ? ' (compound)' : ''}`).join(' + ')
            : `${label(c.salesAccountId)} / ${label(c.purchaseAccountId)}`) },
          { header: 'Status', accessor: (c) => <StatusBadge status={c.isActive ? 'ACTIVE' : 'LEFT'} /> },
          { header: 'Actions', accessor: (c) => (
            <span className="space-x-3">
              <button className="btn-link" onClick={() => startEdit(c)}>Edit</button>
              <button className="btn-link" onClick={() => toggleActive(c)}>{c.isActive ? 'Switch off' : 'Switch on'}</button>
              <button className="btn-link" style={{ color: '#dc2626' }} onClick={() => setDeleting(c)}>Delete</button>
            </span>
          ) },
        ]}
      />

      {lineCodes.length > 0 && (
        <div className="card mt-4" style={{ maxWidth: 820 }}>
          <h3 className="text-sm font-semibold text-gray-700 mb-1">Defaults</h3>
          <p className="text-xs text-gray-500 mb-3">New invoice, bill and expense lines start with these; each line can still be changed.</p>
          <div className="form-grid" style={{ gridTemplateColumns: '1fr 1fr' }}>
            <label className="text-xs font-medium text-gray-600">
              Sales (invoices)
              <select className="mt-1" value={defaults.sales} onChange={(e) => saveDefaults({ ...defaults, sales: e.target.value })} aria-label="Default sales tax">
                <option value="">No tax</option>
                {lineCodes.map((c) => <option key={c.id} value={c.id}>{c.name}</option>)}
              </select>
            </label>
            <label className="text-xs font-medium text-gray-600">
              Purchases (bills and expenses)
              <select className="mt-1" value={defaults.purchases} onChange={(e) => saveDefaults({ ...defaults, purchases: e.target.value })} aria-label="Default purchase tax">
                <option value="">No tax</option>
                {lineCodes.map((c) => <option key={c.id} value={c.id}>{c.name}</option>)}
              </select>
            </label>
          </div>
        </div>
      )}

      {deleting && (
        <Dialog
          title="Delete tax code"
          onClose={() => setDeleting(null)}
          buttons={
            <>
              <button type="button" className="btn btn-primary" onClick={() =>
                api.delete(`/tax-codes/${deleting.id}`).then(load).catch((e) => setError(errMsg(e, 'Could not delete it'))).finally(() => setDeleting(null))
              }>Delete</button>
              <button type="button" className="btn btn-secondary" onClick={() => setDeleting(null)}>Cancel</button>
            </>
          }
        >
          <p className="text-sm text-gray-800">
            Delete <strong>{deleting.name}</strong>? A code that is already on documents or in a group cannot be deleted —
            switch it off instead.
          </p>
        </Dialog>
      )}
    </div>
  );
}
