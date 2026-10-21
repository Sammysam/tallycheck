import { useEffect, useState, type FormEvent } from 'react';
import { Copy, Lock, Pencil, Trash2, Users as UsersIcon } from 'lucide-react';
import api, { errMsg } from '../api/client';
import PageHeader from '../components/PageHeader';
import { useCommand } from '../lib/commands';
import ConfirmDialog from '../components/ConfirmDialog';
import { groupsFor, type PermissionGroup } from '../lib/permissions';
import { toast } from '../lib/toast';

/**
 * D36: a role is the list of screens its users may open. Shown in plain words, grouped as the
 * menu is, with how many users have each role.
 */

interface Permission { key: string; label: string }
interface Role { id: string; name: string; permissions: string[]; isSystem: boolean }
interface UserRow { id: string; isActive: boolean; role: { id: string } | null }

const emptyForm = { name: '', permissions: [] as string[] };

export default function Roles() {
  const [rows, setRows] = useState<Role[]>([]);
  const [groups, setGroups] = useState<PermissionGroup[]>([]);
  const [users, setUsers] = useState<UserRow[]>([]);
  const [showForm, setShowForm] = useState(false);
  const [editingId, setEditingId] = useState<string | null>(null);
  const [form, setForm] = useState(emptyForm);
  const [error, setError] = useState('');
  const [deleting, setDeleting] = useState<Role | null>(null);
  const [busy, setBusy] = useState(false);

  const load = () => {
    api.get('/roles').then((r) => setRows(r.data));
    api.get('/roles/permissions').then((r) => setGroups(groupsFor(r.data as Permission[])));
    api.get('/users').then((r) => setUsers(r.data)).catch(() => {});
  };
  useEffect(() => { load(); }, []);

  const usersWith = (role: Role) => users.filter((u) => u.role?.id === role.id).length;

  const toggle = (key: string) => setForm((f) => ({
    ...f, permissions: f.permissions.includes(key) ? f.permissions.filter((p) => p !== key) : [...f.permissions, key],
  }));
  const toggleGroup = (g: PermissionGroup, on: boolean) => setForm((f) => {
    const keys = g.items.map((i) => i.key);
    const rest = f.permissions.filter((p) => !keys.includes(p));
    return { ...f, permissions: on ? [...rest, ...keys] : rest };
  });

  const startCreate = (from?: Role) => {
    setEditingId(null);
    setForm(from ? { name: `${from.name} (copy)`, permissions: [...from.permissions] } : emptyForm);
    setError('');
    setShowForm(true);
    window.scrollTo({ top: 0 });
  };
  useCommand('new', () => startCreate());
  useCommand('close', () => setShowForm(false));

  const startEdit = (role: Role) => {
    setEditingId(role.id);
    setForm({ name: role.name, permissions: [...role.permissions] });
    setError('');
    setShowForm(true);
    window.scrollTo({ top: 0 });
  };

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    setError('');
    try {
      if (editingId) await api.patch(`/roles/${editingId}`, form);
      else await api.post('/roles', form);
      toast(editingId ? `Role ${form.name} saved.` : `Role ${form.name} created.`);
      setShowForm(false);
      setForm(emptyForm);
      setEditingId(null);
      load();
    } catch (err) {
      setError(errMsg(err, 'Failed to save role'));
    }
  };

  const remove = async () => {
    if (!deleting) return;
    setBusy(true);
    try {
      await api.delete(`/roles/${deleting.id}`);
      toast(`Role ${deleting.name} deleted.`);
      setDeleting(null);
      load();
    } catch (err) {
      toast(errMsg(err, 'Failed to delete role'), 'error');
      setDeleting(null);
    } finally {
      setBusy(false);
    }
  };

  const summary = (role: Role) => {
    if (role.isSystem && role.name === 'Admin') return [{ title: 'Everything', labels: ['Full access, including users, payroll, settings and backups'] }];
    return groups
      .map((g) => ({ title: g.title, labels: g.items.filter((i) => role.permissions.includes(i.key)).map((i) => i.label) }))
      .filter((g) => g.labels.length);
  };

  return (
    <div>
      <PageHeader
        title="Roles"
        subtitle="A role decides which screens its users can open. Give each person the smallest role that lets them do their job."
        action={<button onClick={() => startCreate()} className="btn btn-primary">+ New Role</button>}
      />

      {showForm && (
        <div className="card mb-5">
          <form onSubmit={submit} aria-label="Role">
            <div className="flex items-end gap-3 mb-4">
              <label className="text-xs font-medium text-gray-600" style={{ width: 320 }}>Role name
                <input className="mt-1" required placeholder="Role name" value={form.name} onChange={(e) => setForm({ ...form, name: e.target.value })} />
              </label>
              <span className="text-xs text-gray-500 pb-2">{form.permissions.length} permission{form.permissions.length === 1 ? '' : 's'} chosen</span>
            </div>
            <div className="grid gap-3" style={{ gridTemplateColumns: 'repeat(auto-fill, minmax(300px, 1fr))' }}>
              {groups.map((g) => {
                const all = g.items.every((i) => form.permissions.includes(i.key));
                return (
                  <fieldset key={g.title} className="border border-[#e5e7eb] rounded-md p-3">
                    <legend className="px-1 text-xs font-semibold text-gray-700 flex items-center gap-2">
                      {g.title}
                      <button type="button" className="btn-link text-[11px] font-normal" onClick={() => toggleGroup(g, !all)}>{all ? 'none' : 'all'}</button>
                    </legend>
                    {g.items.map((p) => (
                      <label key={p.key} className="flex items-start gap-2 py-1 cursor-pointer">
                        <input type="checkbox" className="mt-0.5" checked={form.permissions.includes(p.key)} onChange={() => toggle(p.key)} aria-label={p.label} />
                        <span>
                          <span className="text-sm text-gray-800">{p.label}</span>
                          {p.description && <span className="block text-xs text-gray-500">{p.description}</span>}
                        </span>
                      </label>
                    ))}
                  </fieldset>
                );
              })}
            </div>
            {error && <p className="text-sm text-red-600 mt-3" role="alert">{error}</p>}
            <div className="flex gap-2 mt-4">
              <button type="submit" className="btn btn-primary">{editingId ? 'Save Role' : 'Create Role'}</button>
              <button type="button" onClick={() => setShowForm(false)} className="btn btn-secondary">Cancel</button>
            </div>
          </form>
        </div>
      )}

      <div className="grid gap-3" style={{ gridTemplateColumns: 'repeat(auto-fill, minmax(360px, 1fr))' }}>
        {rows.map((r) => (
          <div key={r.id} className="card !mb-0 flex flex-col" data-testid="role-card">
            <div className="flex items-center gap-2 mb-2">
              <h3 className="text-sm font-semibold text-gray-900">{r.name}</h3>
              {r.isSystem && <span className="inline-flex items-center gap-1 text-[10px] uppercase tracking-wide bg-gray-100 text-gray-600 px-1.5 py-0.5 rounded"><Lock size={10} /> Built in</span>}
              <span className="ml-auto inline-flex items-center gap-1 text-xs text-gray-500"><UsersIcon size={13} /> {usersWith(r)} user{usersWith(r) === 1 ? '' : 's'}</span>
            </div>
            <div className="flex-1 space-y-1 text-xs">
              {summary(r).map((g) => (
                <p key={g.title}><span className="font-medium text-gray-700">{g.title}:</span> <span className="text-gray-600">{g.labels.join(', ')}</span></p>
              ))}
              {summary(r).length === 0 && <p className="text-gray-400">Allows nothing yet.</p>}
            </div>
            <div className="flex gap-3 mt-3 pt-2 border-t border-[#f0f0f0]">
              {!r.isSystem && <button className="btn-link text-xs inline-flex items-center gap-1" onClick={() => startEdit(r)}><Pencil size={12} /> Edit</button>}
              <button className="btn-link text-xs inline-flex items-center gap-1" onClick={() => startCreate(r)}><Copy size={12} /> Duplicate</button>
              {!r.isSystem && (
                <button className="btn-link text-xs inline-flex items-center gap-1 ml-auto" style={{ color: '#dc2626' }} onClick={() => setDeleting(r)}><Trash2 size={12} /> Delete</button>
              )}
            </div>
          </div>
        ))}
      </div>

      {deleting && (
        <ConfirmDialog title="Delete role" confirmLabel="Delete role" danger busy={busy} onConfirm={remove} onCancel={() => setDeleting(null)}>
          <p>Delete the role <strong>{deleting.name}</strong>?</p>
          {usersWith(deleting) > 0
            ? <p className="text-amber-700">{usersWith(deleting)} user{usersWith(deleting) === 1 ? ' has' : 's have'} this role. Give them another role first.</p>
            : <p className="text-gray-600">No user has this role.</p>}
        </ConfirmDialog>
      )}
    </div>
  );
}
