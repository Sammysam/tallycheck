import { useEffect, useState } from 'react';
import api from '../api/client';
import PageHeader from '../components/PageHeader';
import { useCommand } from '../lib/commands';
import Table from '../components/Table';
import StatusBadge from '../components/StatusBadge';
import Pagination from '../components/Pagination';
import { money } from '../lib/money';

interface Expense { id: string; expenseNumber: string; date: string; description: string; amount: number; status: string }

export default function Expenses() {
  const [rows, setRows] = useState<Expense[]>([]);
  const [page, setPage] = useState(1);
  const [totalPages, setTotalPages] = useState(1);
  const [total, setTotal] = useState(0);
  const limit = 10;

  const [accounts, setAccounts] = useState<any[]>([]);
  const [showForm, setShowForm] = useState(false);
  // Ctrl+N / Esc and the toolbar, via the desktop shell.
  useCommand('new', () => setShowForm(true));
  useCommand('close', () => setShowForm(false));
  const [form, setForm] = useState({ date: '', description: '', amount: '', expenseAccountId: '', paymentAccountId: '', receiptUrl: '' });
  const [error, setError] = useState('');

  const load = (p = page) =>
    api.get('/expenses', { params: { page: p, limit } }).then((r) => {
      setRows(r.data.data);
      setTotal(r.data.total);
      setTotalPages(r.data.totalPages);
    });

  useEffect(() => { load(); }, [page]);
  useEffect(() => { api.get('/accounts').then((r) => setAccounts(r.data)); }, []);

  const expenseAccounts = accounts.filter((a) => a.type === 'EXPENSE');
  const assetAccounts   = accounts.filter((a) => a.type === 'ASSET');

  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    setError('');
    try {
      await api.post('/expenses', { ...form, amount: Number(form.amount) });
      setShowForm(false);
      setForm({ date: '', description: '', amount: '', expenseAccountId: '', paymentAccountId: '', receiptUrl: '' });
      load(1); setPage(1);
    } catch (err: any) {
      setError(err?.response?.data?.message ?? 'Failed to save expense');
    }
  };

  const action = async (id: string, ep: string) => {
    try { await api.post(`/expenses/${id}/${ep}`); load(); }
    catch (err: any) { alert(err?.response?.data?.message ?? 'Action failed'); }
  };

  const fmt = (v: number) => money(Number(v));

  return (
    <div>
      <PageHeader title="Expenses" action={<button onClick={() => setShowForm(!showForm)} className="btn btn-primary">+ New Expense</button>} />

      {error && <div className="mb-4 px-4 py-2.5 bg-red-50 border border-red-200 text-red-700 text-sm rounded-lg">{error}</div>}

      {showForm && (
        <div className="card mb-5">
          <h3 className="text-sm font-semibold text-gray-700 mb-3">New Expense</h3>
          <form onSubmit={submit} className="space-y-3">
            <div className="form-grid" style={{ gridTemplateColumns: '1fr 2fr 1fr' }}>
              <input required type="date" value={form.date} onChange={(e) => setForm({ ...form, date: e.target.value })} />
              <input required placeholder="Description" value={form.description} onChange={(e) => setForm({ ...form, description: e.target.value })} />
              <input required type="number" min="0.01" step="0.01" placeholder="Amount" value={form.amount} onChange={(e) => setForm({ ...form, amount: e.target.value })} />
            </div>
            <div className="form-grid" style={{ gridTemplateColumns: '1fr 1fr' }}>
              <select required value={form.expenseAccountId} onChange={(e) => setForm({ ...form, expenseAccountId: e.target.value })}>
                <option value="">Expense Account</option>
                {expenseAccounts.map((a) => <option key={a.id} value={a.id}>{a.code} – {a.name}</option>)}
              </select>
              <select required value={form.paymentAccountId} onChange={(e) => setForm({ ...form, paymentAccountId: e.target.value })}>
                <option value="">Paid From (Bank / Cash)</option>
                {assetAccounts.map((a) => <option key={a.id} value={a.id}>{a.code} – {a.name}</option>)}
              </select>
            </div>
            <input placeholder="Receipt URL (optional)" value={form.receiptUrl} onChange={(e) => setForm({ ...form, receiptUrl: e.target.value })} />
            <div className="flex gap-2">
              <button type="submit" className="btn btn-primary">Save</button>
              <button type="button" onClick={() => setShowForm(false)} className="btn btn-secondary">Cancel</button>
            </div>
          </form>
        </div>
      )}

      <Table data={rows} columns={[
        { header: '#',           accessor: 'expenseNumber' },
        { header: 'Date',        accessor: 'date' },
        { header: 'Description', accessor: 'description' },
        { header: 'Amount',      accessor: (r) => fmt(r.amount) },
        { header: 'Status',      accessor: (r) => <StatusBadge status={r.status} /> },
        { header: 'Actions',     accessor: (r) => (
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
