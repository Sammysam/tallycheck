import { useEffect, useState } from 'react';
import api from '../api/client';
import PageHeader from '../components/PageHeader';
import { useCommand } from '../lib/commands';
import Table from '../components/Table';
import StatusBadge from '../components/StatusBadge';
import Pagination from '../components/Pagination';
import { money, moneyIn } from '../lib/money';
import CurrencyPicker from '../components/CurrencyPicker';
import { region } from '../lib/region';
import { lineCodes, summarize, useTaxCodes } from '../lib/taxes';
import TxnLink from '../components/TxnLink';

interface Bill { id: string; billNumber: string; vendor: { name: string }; date: string; totalAmount: number; status: string; currency?: string | null }

const uid = () => `${Date.now()}-${Math.random().toString(36).slice(2)}`;
const emptyLine = () => ({ _key: uid(), description: '', quantity: 1, unitPrice: 0, accountId: '', taxCodeId: region().defaultPurchaseTax });

export default function Bills() {
  const [rows, setRows] = useState<Bill[]>([]);
  const [page, setPage] = useState(1);
  const [totalPages, setTotalPages] = useState(1);
  const [total, setTotal] = useState(0);
  const limit = 10;

  const [vendors, setVendors] = useState<any[]>([]);
  const [accounts, setAccounts] = useState<any[]>([]);
  const [showForm, setShowForm] = useState(false);
  // Ctrl+N / Esc and the toolbar, via the desktop shell.
  useCommand('new', () => setShowForm(true));
  useCommand('close', () => setShowForm(false));
  const [form, setForm] = useState({ vendorId: '', date: '', dueDate: '', pricesIncludeTax: false, currency: '', exchangeRate: '', payableAccountId: '', lines: [emptyLine()] });
  const taxCodes = useTaxCodes();
  // D26: what the bill will total, worked out as the server will.
  const summary = summarize(
    form.lines.map((l) => ({ amount: Math.round((Number(l.quantity) || 0) * (Number(l.unitPrice) || 0) * 100) / 100, taxCodeId: l.taxCodeId })),
    form.pricesIncludeTax,
  );
  const [error, setError] = useState('');

  const load = (p = page) =>
    api.get('/bills', { params: { page: p, limit } }).then((r) => {
      setRows(r.data.data);
      setTotal(r.data.total);
      setTotalPages(r.data.totalPages);
    });

  useEffect(() => { load(); }, [page]);
  useEffect(() => {
    api.get('/vendors').then((r) => setVendors(r.data));
    // D33: a bill line may be an expense, an asset bought on credit or a liability; the bill is
    // owed on a payable (liability) account, Accounts Payable by default.
    api.get('/accounts').then((r) => setAccounts(r.data.filter((a: any) => !a.currency)));
  }, []);

  const LINE_GROUPS: [string, string][] = [['EXPENSE', 'Expenses'], ['ASSET', 'Assets'], ['LIABILITY', 'Liabilities']];
  const payables = accounts.filter((a) => a.type === 'LIABILITY');

  const setLine = (i: number, field: string, val: any) => {
    const lines = form.lines.map((l, idx) => idx === i ? { ...l, [field]: val } : l);
    setForm({ ...form, lines });
  };

  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    setError('');
    try {
      const { lines, ...rest } = form;
      const { currency, exchangeRate, ...doc } = rest;
      await api.post('/bills', {
        ...doc,
        currency: currency || undefined,
        exchangeRate: currency ? Number(exchangeRate) : undefined,
        payableAccountId: doc.payableAccountId || undefined,
        lines: lines.map(({ _key, taxCodeId, ...l }) => ({ ...l, taxCodeId: taxCodeId || undefined })),
      });
      setShowForm(false);
      setForm({ vendorId: '', date: '', dueDate: '', pricesIncludeTax: false, currency: '', exchangeRate: '', payableAccountId: '', lines: [emptyLine()] });
      load(1); setPage(1);
    } catch (err: any) {
      setError(err?.response?.data?.message ?? 'Failed to save bill');
    }
  };

  const action = async (id: string, ep: string) => {
    try { await api.post(`/bills/${id}/${ep}`); load(); }
    catch (err: any) { alert(err?.response?.data?.message ?? 'Action failed'); }
  };


  return (
    <div>
      <PageHeader title="Bills" action={<button onClick={() => setShowForm(!showForm)} className="btn btn-primary">+ New Bill</button>} />

      {error && <div className="mb-4 px-4 py-2.5 bg-red-50 border border-red-200 text-red-700 text-sm rounded-lg">{error}</div>}

      {showForm && (
        <div className="card mb-5">
          <h3 className="text-sm font-semibold text-gray-700 mb-3">New Bill</h3>
          <form onSubmit={submit} className="space-y-4">
            <div className="form-grid" style={{ gridTemplateColumns: '1fr 1fr 1fr' }}>
              <select required value={form.vendorId} onChange={(e) => setForm({ ...form, vendorId: e.target.value })}>
                <option value="">Select Vendor</option>
                {vendors.map((v) => <option key={v.id} value={v.id}>{v.name}</option>)}
              </select>
              <input required type="date" value={form.date} onChange={(e) => setForm({ ...form, date: e.target.value })} />
              <input type="date" value={form.dueDate} onChange={(e) => setForm({ ...form, dueDate: e.target.value })} />
            </div>
            <div style={{ maxWidth: 520 }}>
              <CurrencyPicker compact currency={form.currency} rate={form.exchangeRate}
                onChange={(currency, exchangeRate) => setForm({ ...form, currency, exchangeRate })} />
            </div>
            <div>
              <div className="grid gap-2 mb-1" style={{ gridTemplateColumns: taxCodes.length ? '3fr 80px 100px 2fr 1.4fr' : '3fr 80px 100px 2fr' }}>
                <span className="text-xs text-gray-500 font-medium">Description</span>
                <span className="text-xs text-gray-500 font-medium">Qty</span>
                <span className="text-xs text-gray-500 font-medium">Unit Price</span>
                <span className="text-xs text-gray-500 font-medium">Expense Account</span>
                {taxCodes.length > 0 && <span className="text-xs text-gray-500 font-medium">Tax</span>}
              </div>
              {form.lines.map((line, i) => (
                <div key={line._key} className="grid gap-2 mb-2" style={{ gridTemplateColumns: taxCodes.length ? '3fr 80px 100px 2fr 1.4fr' : '3fr 80px 100px 2fr' }}>
                  <input required placeholder="Item description" value={line.description} onChange={(e) => setLine(i, 'description', e.target.value)} />
                  <input required type="number" min="0.001" step="any" value={line.quantity} onChange={(e) => setLine(i, 'quantity', Number(e.target.value))} />
                  <input required type="number" min="0" step="0.01" value={line.unitPrice || ''} onChange={(e) => setLine(i, 'unitPrice', Number(e.target.value))} />
                  <select required value={line.accountId} onChange={(e) => setLine(i, 'accountId', e.target.value)} aria-label={`Account for line ${i + 1}`}>
                    <option value="">Account</option>
                    {LINE_GROUPS.map(([type, label]) => (
                      <optgroup key={type} label={label}>
                        {accounts.filter((a) => a.type === type).map((a) => <option key={a.id} value={a.id}>{a.code} – {a.name}</option>)}
                      </optgroup>
                    ))}
                  </select>
                  {taxCodes.length > 0 && (
                    <select value={line.taxCodeId} onChange={(e) => setLine(i, 'taxCodeId', e.target.value)} aria-label={`Tax for line ${i + 1}`}>
                      <option value="">No tax</option>
                      {lineCodes(taxCodes).map((c) => <option key={c.id} value={c.id}>{c.name}</option>)}
                    </select>
                  )}
                </div>
              ))}
              <button type="button" className="btn-link" onClick={() => setForm({ ...form, lines: [...form.lines, emptyLine()] })}>+ Add line</button>
              <label className="text-xs font-medium text-gray-600 block mt-3" style={{ maxWidth: 520 }}>
                Owed on (payable account)
                <select className="mt-1" value={form.payableAccountId} onChange={(e) => setForm({ ...form, payableAccountId: e.target.value })} aria-label="Payable account">
                  <option value="">2000 – Accounts Payable (default)</option>
                  {payables.filter((a) => a.code !== '2000').map((a) => <option key={a.id} value={a.id}>{a.code} – {a.name}</option>)}
                </select>
                <span className="text-gray-500 font-normal">Choose a separate payable, such as Telephone Payable, to see it on its own line in the Balance Sheet.</span>
              </label>
            </div>
            <div className="flex items-center justify-between gap-4 text-sm">
              {taxCodes.length > 0 ? (
                <label className="flex items-center gap-2">
                  <input type="checkbox" checked={form.pricesIncludeTax} onChange={(e) => setForm({ ...form, pricesIncludeTax: e.target.checked })} />
                  Prices include tax
                </label>
              ) : <span />}
              <span className="tabular-nums text-gray-700">
                {summary.tax !== 0 && <>Subtotal {moneyIn(summary.subtotal, form.currency)} · Tax {moneyIn(summary.tax, form.currency)} · </>}
                <strong>Total {moneyIn(summary.total, form.currency)}</strong>
                {form.currency && Number(form.exchangeRate) > 0 && (
                  <span className="text-xs text-gray-500"> ≈ {money(Math.round(summary.total * Number(form.exchangeRate) * 100) / 100)}</span>
                )}
              </span>
            </div>
            <div className="flex gap-2 pt-1">
              <button type="submit" className="btn btn-primary">Save Bill</button>
              <button type="button" onClick={() => setShowForm(false)} className="btn btn-secondary">Cancel</button>
            </div>
          </form>
        </div>
      )}

      <Table data={rows} columns={[
        { header: '#',       accessor: (r) => <TxnLink entryId={(r as { journalEntryId?: string }).journalEntryId}>{r.billNumber}</TxnLink> },
        { header: 'Vendor',  accessor: (r) => r.vendor?.name ?? '—' },
        { header: 'Date',    accessor: 'date' },
        { header: 'Amount',  accessor: (r) => moneyIn(Number(r.totalAmount), r.currency) },
        { header: 'Status',  accessor: (r) => <StatusBadge status={r.status} /> },
        { header: 'Actions', accessor: (r) => (
          <div className="flex gap-2">
            {r.status === 'DRAFT'  && <button onClick={() => action(r.id, 'post')} className="btn-link">Post</button>}
            {r.status === 'POSTED' && <button onClick={() => action(r.id, 'void')} className="btn-link" style={{ color: '#dc2626' }}>Void</button>}
          </div>
        )},
      ]} />
      <Pagination page={page} totalPages={totalPages} total={total} limit={limit} onPage={(p) => { setPage(p); load(p); }} />
    </div>
  );
}
