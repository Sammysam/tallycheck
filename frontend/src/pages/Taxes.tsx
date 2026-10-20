import { useEffect, useState } from 'react';
import api, { errMsg } from '../api/client';
import PageHeader from '../components/PageHeader';
import Table from '../components/Table';
import StatusBadge from '../components/StatusBadge';
import Dialog from '../components/desktop/Dialog';
import { useCommand } from '../lib/commands';
import { region, regionFromSettings, setRegion } from '../lib/region';
import type { TaxCode } from '../lib/taxes';

/**
 * D26: the company's own tax codes — VAT, GST, sales tax, zero-rated, exempt. Nothing about any
 * country's tax law is built in: the company enters its rates and where each tax posts.
 */

interface Account { id: string; code: string; name: string; type: string; isActive: boolean }

const emptyForm = { name: '', rate: '', description: '', salesAccountId: '', purchaseAccountId: '', isActive: true };

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

  const liabilities = accounts.filter((a) => a.type === 'LIABILITY' && a.isActive);
  const assetsAndLiabilities = accounts.filter((a) => (a.type === 'ASSET' || a.type === 'LIABILITY') && a.isActive);
  const byId = (id: string) => accounts.find((a) => a.id === id);
  const label = (id: string) => {
    const a = byId(id);
    return a ? `${a.code} ${a.name}` : '—';
  };

  const startNew = () => {
    setEditing(null);
    setError('');
    // Usual tax accounts first, if the chart has them.
    const payable = accounts.find((a) => a.type === 'LIABILITY' && /tax|vat|gst/i.test(a.name));
    const receivable = accounts.find((a) => a.type === 'ASSET' && /tax|vat|gst/i.test(a.name));
    setForm({ ...emptyForm, salesAccountId: payable?.id ?? '', purchaseAccountId: receivable?.id ?? payable?.id ?? '' });
    setShowForm(true);
  };
  useCommand('new', startNew);
  useCommand('close', () => setShowForm(false));

  const startEdit = (c: TaxCode) => {
    setEditing(c.id);
    setError('');
    setForm({
      name: c.name, rate: String(c.rate), description: c.description ?? '',
      salesAccountId: c.salesAccountId, purchaseAccountId: c.purchaseAccountId, isActive: c.isActive,
    });
    setShowForm(true);
  };

  /** Adds "Tax Payable" and "Tax Receivable" to the chart if they are not there yet. */
  const createTaxAccounts = async () => {
    setBusy(true);
    setError('');
    try {
      const taken = new Set(accounts.map((a) => a.code));
      const pick = (preferred: string[]) => preferred.find((c) => !taken.has(c)) ?? '';
      const payableCode = pick(['2300', '2310', '2390']);
      const receivableCode = pick(['1400', '1410', '1490']);
      const made = await Promise.all([
        api.post('/accounts', { code: payableCode, name: 'Tax Payable', type: 'LIABILITY', subtype: 'current_liability' }),
        api.post('/accounts', { code: receivableCode, name: 'Tax Receivable', type: 'ASSET', subtype: 'current_asset' }),
      ]);
      await loadAccounts();
      setForm((f) => ({ ...f, salesAccountId: made[0].data.id, purchaseAccountId: made[1].data.id }));
    } catch (e) {
      setError(errMsg(e, 'Could not add the tax accounts'));
    } finally {
      setBusy(false);
    }
  };

  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    setBusy(true);
    setError('');
    try {
      const body = {
        name: form.name.trim(),
        rate: Number(form.rate),
        description: form.description.trim() || null,
        salesAccountId: form.salesAccountId,
        purchaseAccountId: form.purchaseAccountId,
        isActive: form.isActive,
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

  const active = codes.filter((c) => c.isActive);

  return (
    <div>
      <PageHeader
        title="Taxes"
        subtitle="Your tax codes — VAT, GST, sales tax, zero-rated or exempt — and where each posts."
        action={<button className="btn btn-primary" onClick={startNew}>+ New Tax Code</button>}
      />
      {error && <div className="mb-3 px-3 py-2 bg-red-50 border border-red-200 text-red-700 text-sm rounded">{error}</div>}

      {showForm && (
        <div className="card mb-4" style={{ maxWidth: 760 }}>
          <form onSubmit={submit} className="form-grid" style={{ gridTemplateColumns: '2fr 1fr' }} aria-label="Tax code">
            <label className="text-xs font-medium text-gray-600">
              Name (as printed on invoices)
              <input className="mt-1" required maxLength={60} placeholder="e.g. VAT 18%" value={form.name}
                onChange={(e) => setForm({ ...form, name: e.target.value })} />
            </label>
            <label className="text-xs font-medium text-gray-600">
              Rate %
              <input className="mt-1" required type="number" min="0" max="100" step="0.0001" placeholder="e.g. 18" value={form.rate}
                onChange={(e) => setForm({ ...form, rate: e.target.value })} />
            </label>
            <label className="text-xs font-medium text-gray-600">
              Tax on sales posts to (a liability)
              <select className="mt-1" required value={form.salesAccountId} onChange={(e) => setForm({ ...form, salesAccountId: e.target.value })} aria-label="Sales tax account">
                <option value="">Choose an account…</option>
                {liabilities.map((a) => <option key={a.id} value={a.id}>{a.code} — {a.name}</option>)}
              </select>
            </label>
            <label className="text-xs font-medium text-gray-600">
              Tax on purchases posts to
              <select className="mt-1" required value={form.purchaseAccountId} onChange={(e) => setForm({ ...form, purchaseAccountId: e.target.value })} aria-label="Purchase tax account">
                <option value="">Choose an account…</option>
                {assetsAndLiabilities.map((a) => <option key={a.id} value={a.id}>{a.code} — {a.name}</option>)}
              </select>
            </label>
            <label className="text-xs font-medium text-gray-600" style={{ gridColumn: '1 / -1' }}>
              Note (optional)
              <input className="mt-1" maxLength={200} value={form.description} onChange={(e) => setForm({ ...form, description: e.target.value })} />
            </label>
            {!accounts.some((a) => a.type === 'LIABILITY' && /tax|vat|gst/i.test(a.name)) && (
              <p className="text-xs text-gray-600" style={{ gridColumn: '1 / -1' }}>
                No tax accounts in the chart yet.{' '}
                <button type="button" className="btn-link" disabled={busy} onClick={createTaxAccounts}>Add Tax Payable and Tax Receivable</button>
              </p>
            )}
            <div className="flex gap-2" style={{ gridColumn: '1 / -1' }}>
              <button type="submit" className="btn btn-primary" disabled={busy}>{editing ? 'Save changes' : 'Save Tax Code'}</button>
              <button type="button" className="btn btn-secondary" onClick={() => setShowForm(false)}>Cancel</button>
            </div>
            {editing && (
              <p className="text-xs text-gray-500" style={{ gridColumn: '1 / -1' }}>
                A new rate applies to new documents only; saved invoices and bills keep the tax they were raised with.
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
          { header: 'Rate', accessor: (c) => `${c.rate}%` },
          { header: 'Sales tax account', accessor: (c) => label(c.salesAccountId) },
          { header: 'Purchase tax account', accessor: (c) => label(c.purchaseAccountId) },
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

      {active.length > 0 && (
        <div className="card mt-4" style={{ maxWidth: 760 }}>
          <h3 className="text-sm font-semibold text-gray-700 mb-1">Defaults</h3>
          <p className="text-xs text-gray-500 mb-3">New invoice, bill and expense lines start with these; each line can still be changed.</p>
          <div className="form-grid" style={{ gridTemplateColumns: '1fr 1fr' }}>
            <label className="text-xs font-medium text-gray-600">
              Sales (invoices)
              <select className="mt-1" value={defaults.sales} onChange={(e) => saveDefaults({ ...defaults, sales: e.target.value })} aria-label="Default sales tax">
                <option value="">No tax</option>
                {active.map((c) => <option key={c.id} value={c.id}>{c.name}</option>)}
              </select>
            </label>
            <label className="text-xs font-medium text-gray-600">
              Purchases (bills and expenses)
              <select className="mt-1" value={defaults.purchases} onChange={(e) => saveDefaults({ ...defaults, purchases: e.target.value })} aria-label="Default purchase tax">
                <option value="">No tax</option>
                {active.map((c) => <option key={c.id} value={c.id}>{c.name}</option>)}
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
            Delete <strong>{deleting.name}</strong>? A code that is already on documents cannot be deleted — switch it off instead.
          </p>
        </Dialog>
      )}
    </div>
  );
}
