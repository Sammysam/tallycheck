import { useEffect, useState } from 'react';
import api from '../api/client';
import PageHeader from '../components/PageHeader';
import { useCommand } from '../lib/commands';
import Table from '../components/Table';

interface Account { id: string; code: string; name: string; type: string; subtype: string }

export default function Accounts() {
  const [accounts, setAccounts] = useState<Account[]>([]);
  const [showForm, setShowForm] = useState(false);
  // Ctrl+N / Esc and the toolbar, via the desktop shell.
  useCommand('new', () => setShowForm(true));
  useCommand('close', () => setShowForm(false));
  const [form, setForm] = useState({ code: '', name: '', type: 'ASSET', subtype: '' });

  const load = () => api.get('/accounts').then((r) => setAccounts(r.data));
  useEffect(() => { load(); }, []);

  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    await api.post('/accounts', form);
    setShowForm(false);
    setForm({ code: '', name: '', type: 'ASSET', subtype: '' });
    load();
  };

  const remove = async (row: Account) => {
    if (!window.confirm(`Delete account "${row.code} – ${row.name}"? This cannot be undone.`)) return;
    try {
      await api.delete(`/accounts/${row.id}`);
      load();
    } catch (err: any) {
      alert(err?.response?.data?.message ?? 'Failed to delete account');
    }
  };

  return (
    <div>
      <PageHeader
        title="Chart of Accounts"
        action={<button onClick={() => setShowForm(!showForm)} className="btn btn-primary">+ New Account</button>}
      />

      {showForm && (
        <div className="card mb-5">
          <form onSubmit={submit} className="form-grid" style={{ gridTemplateColumns: '1fr 1fr' }}>
            <input required placeholder="Code (e.g. 1000)" value={form.code} onChange={(e) => setForm({ ...form, code: e.target.value })} />
            <input required placeholder="Account Name" value={form.name} onChange={(e) => setForm({ ...form, name: e.target.value })} />
            <select value={form.type} onChange={(e) => setForm({ ...form, type: e.target.value })}>
              {['ASSET','LIABILITY','EQUITY','REVENUE','EXPENSE'].map((t) => <option key={t}>{t}</option>)}
            </select>
            <input placeholder="Subtype (optional, e.g. current_asset)" value={form.subtype} onChange={(e) => setForm({ ...form, subtype: e.target.value })} />
            <div className="flex gap-2" style={{ gridColumn: '1 / -1' }}>
              <button type="submit" className="btn btn-primary">Save Account</button>
              <button type="button" onClick={() => setShowForm(false)} className="btn btn-secondary">Cancel</button>
            </div>
          </form>
        </div>
      )}

      <Table
        data={accounts}
        columns={[
          { header: 'Code',    accessor: 'code' },
          { header: 'Name',    accessor: 'name' },
          { header: 'Type',    accessor: 'type' },
          { header: 'Subtype', accessor: (r) => r.subtype ?? '—' },
          { header: '', accessor: (r) => (
            <button onClick={() => remove(r)} className="btn-link text-xs" style={{ color: '#dc2626' }}>Delete</button>
          )},
        ]}
      />
    </div>
  );
}
