import { useEffect, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import api from '../api/client';
import PageHeader from '../components/PageHeader';
import { useCommand } from '../lib/commands';
import Table from '../components/Table';
import { useAuth } from '../contexts/AuthContext';

interface Customer { id: string; name: string; email: string; phone: string; address: string }

export default function Customers() {
  const [rows, setRows] = useState<Customer[]>([]);
  const navigate = useNavigate();
  const { hasPermission } = useAuth();
  const [showForm, setShowForm] = useState(false);
  // Ctrl+N / Esc and the toolbar, via the desktop shell.
  useCommand('new', () => setShowForm(true));
  useCommand('close', () => setShowForm(false));
  const [form, setForm] = useState({ name: '', email: '', phone: '', address: '' });

  const load = () => api.get('/customers').then((r) => setRows(r.data));
  useEffect(() => { load(); }, []);

  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    await api.post('/customers', form);
    setShowForm(false);
    setForm({ name: '', email: '', phone: '', address: '' });
    load();
  };

  const remove = async (row: Customer) => {
    if (!window.confirm(`Delete customer "${row.name}"? This cannot be undone.`)) return;
    try {
      await api.delete(`/customers/${row.id}`);
      load();
    } catch (err: any) {
      alert(err?.response?.data?.message ?? 'Failed to delete customer');
    }
  };

  return (
    <div>
      <PageHeader
        title="Customers"
        action={<button onClick={() => setShowForm(!showForm)} className="btn btn-primary">+ New Customer</button>}
      />

      {showForm && (
        <div className="card mb-5">
          <form onSubmit={submit} className="form-grid" style={{ gridTemplateColumns: '1fr 1fr' }}>
            <input required placeholder="Full Name" value={form.name} onChange={(e) => setForm({ ...form, name: e.target.value })} />
            <input type="email" placeholder="Email" value={form.email} onChange={(e) => setForm({ ...form, email: e.target.value })} />
            <input placeholder="Phone" value={form.phone} onChange={(e) => setForm({ ...form, phone: e.target.value })} />
            <input placeholder="Address" value={form.address} onChange={(e) => setForm({ ...form, address: e.target.value })} />
            <div className="flex gap-2" style={{ gridColumn: '1 / -1' }}>
              <button type="submit" className="btn btn-primary">Save</button>
              <button type="button" onClick={() => setShowForm(false)} className="btn btn-secondary">Cancel</button>
            </div>
          </form>
        </div>
      )}

      <Table data={rows} columns={[
        { header: 'Name',    accessor: 'name' },
        { header: 'Email',   accessor: (r) => r.email   || '—' },
        { header: 'Phone',   accessor: (r) => r.phone   || '—' },
        { header: 'Address', accessor: (r) => r.address || '—' },
        { header: '', accessor: (r) => (
          <span className="space-x-3">
            {hasPermission('invoices') && (
              <button onClick={() => navigate(`/customers/${r.id}/statement`)} className="btn-link text-xs">Statement</button>
            )}
            <button onClick={() => remove(r)} className="btn-link text-xs" style={{ color: '#dc2626' }}>Delete</button>
          </span>
        )},
      ]} />
    </div>
  );
}
