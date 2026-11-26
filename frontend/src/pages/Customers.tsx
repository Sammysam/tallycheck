import { useEffect, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import api from '../api/client';
import PageHeader from '../components/PageHeader';
import { useCommand } from '../lib/commands';
import Table from '../components/Table';
import { toast } from '../lib/toast';
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
  // The customer being edited (null = adding a new one).
  const [editingId, setEditingId] = useState<string | null>(null);

  const load = () => api.get('/customers').then((r) => setRows(r.data));
  useEffect(() => { load(); }, []);

  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (editingId) {
      await api.patch(`/customers/${editingId}`, form);
      toast('Customer saved');
    } else {
      await api.post('/customers', form);
      toast('Customer added');
    }
    setShowForm(false);
    setEditingId(null);
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
          {/* D76: a heading and visible labels, so a filled form still says what each box is. */}
          <h3 className="text-sm font-semibold text-gray-800 mb-2">{editingId ? `Edit ${form.name || 'customer'}` : 'New customer'}</h3>
          <form onSubmit={submit} className="form-grid" style={{ gridTemplateColumns: '1fr 1fr' }}>
            <label className="text-xs font-medium text-gray-600">Name
              <input className="mt-1 w-full" required placeholder="Full Name" value={form.name} onChange={(e) => setForm({ ...form, name: e.target.value })} />
            </label>
            <label className="text-xs font-medium text-gray-600">Email (optional)
              <input className="mt-1 w-full" type="email" placeholder="Email" value={form.email} onChange={(e) => setForm({ ...form, email: e.target.value })} />
            </label>
            <label className="text-xs font-medium text-gray-600">Phone (optional)
              <input className="mt-1 w-full" placeholder="Phone" value={form.phone} onChange={(e) => setForm({ ...form, phone: e.target.value })} />
            </label>
            <label className="text-xs font-medium text-gray-600">Address (optional)
              <input className="mt-1 w-full" placeholder="Address" value={form.address} onChange={(e) => setForm({ ...form, address: e.target.value })} />
            </label>
            <div className="flex gap-2" style={{ gridColumn: '1 / -1' }}>
              <button type="submit" className="btn btn-primary">Save</button>
              <button type="button" onClick={() => { setShowForm(false); setEditingId(null); setForm({ name: '', email: '', phone: '', address: '' }); }} className="btn btn-secondary">Cancel</button>
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
            <button onClick={() => { setForm({ name: r.name ?? '', email: r.email ?? '', phone: r.phone ?? '', address: r.address ?? '' }); setEditingId(r.id); setShowForm(true); window.scrollTo(0, 0); }} className="btn-link text-xs">Edit</button>
            <button onClick={() => remove(r)} className="btn-link text-xs" style={{ color: '#dc2626' }}>Delete</button>
          </span>
        )},
      ]} />
    </div>
  );
}
