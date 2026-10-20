import { useEffect, useState } from 'react';
import api from '../api/client';
import PageHeader from '../components/PageHeader';
import { useCommand } from '../lib/commands';
import Table from '../components/Table';
import Pagination from '../components/Pagination';

interface Payment { id: string; paymentNumber: string; type: string; date: string; amount: number; paymentMethod: string }

export default function Payments() {
  const [rows, setRows] = useState<Payment[]>([]);
  const [page, setPage] = useState(1);
  const [totalPages, setTotalPages] = useState(1);
  const [total, setTotal] = useState(0);
  const limit = 10;

  const [accounts, setAccounts] = useState<any[]>([]);
  const [invoices, setInvoices] = useState<any[]>([]);
  const [bills, setBills] = useState<any[]>([]);
  const [showForm, setShowForm] = useState(false);
  // Ctrl+N / Esc and the toolbar, via the desktop shell.
  useCommand('new', () => setShowForm(true));
  useCommand('close', () => setShowForm(false));
  const [form, setForm] = useState({ type: 'AR', invoiceId: '', billId: '', date: '', amount: '', paymentMethod: 'BANK_TRANSFER', paymentAccountId: '', notes: '' });
  const [error, setError] = useState('');

  const load = (p = page) =>
    api.get('/payments', { params: { page: p, limit } }).then((r) => {
      setRows(r.data.data);
      setTotal(r.data.total);
      setTotalPages(r.data.totalPages);
    });

  useEffect(() => { load(); }, [page]);
  useEffect(() => {
    api.get('/accounts').then((r) => setAccounts(r.data.filter((a: any) => a.type === 'ASSET')));
    api.get('/invoices', { params: { limit: 100 } }).then((r) => setInvoices((r.data.data ?? r.data).filter((i: any) => i.status === 'POSTED')));
    api.get('/bills',    { params: { limit: 100 } }).then((r) => setBills((r.data.data ?? r.data).filter((b: any) => b.status === 'POSTED')));
  }, []);

  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    setError('');
    try {
      const payload = {
        ...form,
        amount: Number(form.amount),
        invoiceId: form.type === 'AR' ? form.invoiceId : undefined,
        billId:    form.type === 'AP' ? form.billId    : undefined,
      };
      await api.post('/payments', payload);
      setShowForm(false);
      setForm({ type: 'AR', invoiceId: '', billId: '', date: '', amount: '', paymentMethod: 'BANK_TRANSFER', paymentAccountId: '', notes: '' });
      load(1); setPage(1);
    } catch (err: any) {
      setError(err?.response?.data?.message ?? 'Failed to record payment');
    }
  };

  const fmt = (v: number) => new Intl.NumberFormat('en-LK', { style: 'currency', currency: 'LKR' }).format(Number(v));

  return (
    <div>
      <PageHeader title="Payments" action={<button onClick={() => setShowForm(!showForm)} className="btn btn-primary">+ Record Payment</button>} />

      {error && <div className="mb-4 px-4 py-2.5 bg-red-50 border border-red-200 text-red-700 text-sm rounded-lg">{error}</div>}

      {showForm && (
        <div className="card mb-5">
          <h3 className="text-sm font-semibold text-gray-700 mb-3">Record Payment</h3>
          <form onSubmit={submit} className="space-y-3">
            <div className="form-grid" style={{ gridTemplateColumns: '1fr 1fr' }}>
              <select value={form.type} onChange={(e) => setForm({ ...form, type: e.target.value, invoiceId: '', billId: '' })}>
                <option value="AR">AR — Customer Payment</option>
                <option value="AP">AP — Vendor Payment</option>
              </select>
              {form.type === 'AR'
                ? <select required value={form.invoiceId} onChange={(e) => setForm({ ...form, invoiceId: e.target.value })}>
                    <option value="">Select Posted Invoice</option>
                    {invoices.map((i) => (
                      <option key={i.id} value={i.id}>
                        {i.invoiceNumber} — {i.customer?.name || i.billToName || 'one-off'} · balance {fmt(Number(i.totalAmount) - Number(i.amountPaid ?? 0))}
                      </option>
                    ))}
                  </select>
                : <select required value={form.billId} onChange={(e) => setForm({ ...form, billId: e.target.value })}>
                    <option value="">Select Posted Bill</option>
                    {bills.map((b) => <option key={b.id} value={b.id}>{b.billNumber} — {b.vendor?.name}</option>)}
                  </select>
              }
            </div>
            <div className="form-grid" style={{ gridTemplateColumns: '1fr 1fr 1fr 1fr' }}>
              <input required type="date" value={form.date} onChange={(e) => setForm({ ...form, date: e.target.value })} />
              <input required type="number" min="0.01" step="0.01" placeholder="Amount" value={form.amount} onChange={(e) => setForm({ ...form, amount: e.target.value })} />
              <select value={form.paymentMethod} onChange={(e) => setForm({ ...form, paymentMethod: e.target.value })}>
                <option value="BANK_TRANSFER">Bank Transfer</option>
                <option value="CASH">Cash</option>
                <option value="CHECK">Check</option>
              </select>
              <select required value={form.paymentAccountId} onChange={(e) => setForm({ ...form, paymentAccountId: e.target.value })}>
                <option value="">Bank / Cash Account</option>
                {accounts.map((a) => <option key={a.id} value={a.id}>{a.code} – {a.name}</option>)}
              </select>
            </div>
            <div className="flex gap-2">
              <button type="submit" className="btn btn-primary">Record Payment</button>
              <button type="button" onClick={() => setShowForm(false)} className="btn btn-secondary">Cancel</button>
            </div>
          </form>
        </div>
      )}

      <Table data={rows} columns={[
        { header: '#',      accessor: 'paymentNumber' },
        { header: 'Type',   accessor: 'type' },
        { header: 'Date',   accessor: 'date' },
        { header: 'Amount', accessor: (r) => fmt(r.amount) },
        { header: 'Method', accessor: 'paymentMethod' },
      ]} />
      <Pagination page={page} totalPages={totalPages} total={total} limit={limit} onPage={(p) => { setPage(p); load(p); }} />
    </div>
  );
}
