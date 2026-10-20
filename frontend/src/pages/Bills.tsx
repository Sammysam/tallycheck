import { useEffect, useState } from 'react';
import api from '../api/client';
import PageHeader from '../components/PageHeader';
import { useCommand } from '../lib/commands';
import Table from '../components/Table';
import StatusBadge from '../components/StatusBadge';
import Pagination from '../components/Pagination';

interface Bill { id: string; billNumber: string; vendor: { name: string }; date: string; totalAmount: number; status: string }

const uid = () => `${Date.now()}-${Math.random().toString(36).slice(2)}`;
const emptyLine = () => ({ _key: uid(), description: '', quantity: 1, unitPrice: 0, accountId: '' });

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
  const [form, setForm] = useState({ vendorId: '', date: '', dueDate: '', lines: [emptyLine()] });
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
    api.get('/accounts').then((r) => setAccounts(r.data.filter((a: any) => a.type === 'EXPENSE')));
  }, []);

  const setLine = (i: number, field: string, val: any) => {
    const lines = form.lines.map((l, idx) => idx === i ? { ...l, [field]: val } : l);
    setForm({ ...form, lines });
  };

  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    setError('');
    try {
      const { lines, ...rest } = form;
      await api.post('/bills', { ...rest, lines: lines.map(({ _key, ...l }) => l) });
      setShowForm(false);
      setForm({ vendorId: '', date: '', dueDate: '', lines: [emptyLine()] });
      load(1); setPage(1);
    } catch (err: any) {
      setError(err?.response?.data?.message ?? 'Failed to save bill');
    }
  };

  const action = async (id: string, ep: string) => {
    try { await api.post(`/bills/${id}/${ep}`); load(); }
    catch (err: any) { alert(err?.response?.data?.message ?? 'Action failed'); }
  };

  const fmt = (v: number) => new Intl.NumberFormat('en-LK', { style: 'currency', currency: 'LKR' }).format(Number(v));

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
            <div>
              <div className="grid gap-2 mb-1" style={{ gridTemplateColumns: '3fr 80px 100px 2fr' }}>
                <span className="text-xs text-gray-500 font-medium">Description</span>
                <span className="text-xs text-gray-500 font-medium">Qty</span>
                <span className="text-xs text-gray-500 font-medium">Unit Price</span>
                <span className="text-xs text-gray-500 font-medium">Expense Account</span>
              </div>
              {form.lines.map((line, i) => (
                <div key={line._key} className="grid gap-2 mb-2" style={{ gridTemplateColumns: '3fr 80px 100px 2fr' }}>
                  <input required placeholder="Item description" value={line.description} onChange={(e) => setLine(i, 'description', e.target.value)} />
                  <input required type="number" min="0.001" step="any" value={line.quantity} onChange={(e) => setLine(i, 'quantity', Number(e.target.value))} />
                  <input required type="number" min="0" step="0.01" value={line.unitPrice || ''} onChange={(e) => setLine(i, 'unitPrice', Number(e.target.value))} />
                  <select required value={line.accountId} onChange={(e) => setLine(i, 'accountId', e.target.value)}>
                    <option value="">Account</option>
                    {accounts.map((a) => <option key={a.id} value={a.id}>{a.code} – {a.name}</option>)}
                  </select>
                </div>
              ))}
              <button type="button" className="btn-link" onClick={() => setForm({ ...form, lines: [...form.lines, emptyLine()] })}>+ Add line</button>
            </div>
            <div className="flex gap-2 pt-1">
              <button type="submit" className="btn btn-primary">Save Bill</button>
              <button type="button" onClick={() => setShowForm(false)} className="btn btn-secondary">Cancel</button>
            </div>
          </form>
        </div>
      )}

      <Table data={rows} columns={[
        { header: '#',       accessor: 'billNumber' },
        { header: 'Vendor',  accessor: (r) => r.vendor?.name ?? '—' },
        { header: 'Date',    accessor: 'date' },
        { header: 'Amount',  accessor: (r) => fmt(r.totalAmount) },
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
