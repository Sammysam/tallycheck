import { useEffect, useState } from 'react';
import api from '../api/client';
import PageHeader from '../components/PageHeader';
import { useCommand } from '../lib/commands';
import Table from '../components/Table';

interface Permission { key: string; label: string }
interface Role { id: string; name: string; permissions: string[]; isSystem: boolean }

const emptyForm = { name: '', permissions: [] as string[] };

export default function Roles() {
  const [rows, setRows] = useState<Role[]>([]);
  const [catalog, setCatalog] = useState<Permission[]>([]);
  const [showForm, setShowForm] = useState(false);
  const [editingId, setEditingId] = useState<string | null>(null);
  const [form, setForm] = useState(emptyForm);
  const [error, setError] = useState('');

  const load = () => {
    api.get('/roles').then((r) => setRows(r.data));
    api.get('/roles/permissions').then((r) => setCatalog(r.data));
  };
  useEffect(() => { load(); }, []);

  const togglePermission = (key: string) => {
    setForm((f) => ({
      ...f,
      permissions: f.permissions.includes(key)
        ? f.permissions.filter((p) => p !== key)
        : [...f.permissions, key],
    }));
  };

  const startCreate = () => {
    setEditingId(null);
    setForm(emptyForm);
    setError('');
    setShowForm(true);
  };

  // Ctrl+N / Esc and the toolbar, via the desktop shell.
  useCommand('new', startCreate);
  useCommand('close', () => setShowForm(false));

  const startEdit = (role: Role) => {
    setEditingId(role.id);
    setForm({ name: role.name, permissions: role.permissions });
    setError('');
    setShowForm(true);
  };

  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    setError('');
    try {
      if (editingId) {
        await api.patch(`/roles/${editingId}`, form);
      } else {
        await api.post('/roles', form);
      }
      setShowForm(false);
      setForm(emptyForm);
      setEditingId(null);
      load();
    } catch (err: any) {
      setError(err?.response?.data?.message ?? 'Failed to save role');
    }
  };

  const remove = async (role: Role) => {
    if (!confirm(`Delete role "${role.name}"?`)) return;
    try {
      await api.delete(`/roles/${role.id}`);
      load();
    } catch (err: any) {
      alert(err?.response?.data?.message ?? 'Failed to delete role');
    }
  };

  return (
    <div>
      <PageHeader
        title="Roles"
        action={<button onClick={startCreate} className="btn btn-primary">+ New Role</button>}
      />

      {showForm && (
        <div className="card mb-5">
          <form onSubmit={submit}>
            <div className="form-grid mb-3" style={{ gridTemplateColumns: '1fr' }}>
              <input
                required
                placeholder="Role name"
                value={form.name}
                onChange={(e) => setForm({ ...form, name: e.target.value })}
                disabled={!!editingId && rows.find((r) => r.id === editingId)?.isSystem}
              />
            </div>
            <p className="text-xs font-semibold text-gray-500 uppercase tracking-wide mb-2">Pages this role can access</p>
            <div className="grid grid-cols-2 sm:grid-cols-3 gap-2 mb-3">
              {catalog.map((perm) => (
                <label key={perm.key} className="flex items-center gap-2 text-sm text-gray-700">
                  <input
                    type="checkbox"
                    checked={form.permissions.includes(perm.key)}
                    onChange={() => togglePermission(perm.key)}
                  />
                  {perm.label}
                </label>
              ))}
            </div>
            {error && <p className="text-sm text-red-600 mb-2">{error}</p>}
            <div className="flex gap-2">
              <button type="submit" className="btn btn-primary">Save</button>
              <button type="button" onClick={() => setShowForm(false)} className="btn btn-secondary">Cancel</button>
            </div>
          </form>
        </div>
      )}

      <Table data={rows} columns={[
        {
          header: 'Name',
          accessor: (r) => (
            <span className="font-medium">
              {r.name}
              {r.isSystem && <span className="ml-2 text-xs text-gray-400">(system)</span>}
            </span>
          ),
        },
        {
          header: 'Pages',
          accessor: (r) => (
            <div className="flex flex-wrap gap-1">
              {r.permissions.map((p) => (
                <span key={p} className="px-1.5 py-0.5 rounded bg-gray-100 text-gray-600 text-xs">{p}</span>
              ))}
            </div>
          ),
        },
        {
          header: '',
          accessor: (r) => r.isSystem ? null : (
            <div className="flex gap-3">
              <button onClick={() => startEdit(r)} className="btn-link">Edit</button>
              <button onClick={() => remove(r)} className="btn-link" style={{ color: '#dc2626' }}>Delete</button>
            </div>
          ),
        },
      ]} />
    </div>
  );
}
