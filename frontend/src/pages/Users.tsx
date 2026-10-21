import { useEffect, useMemo, useState, type FormEvent } from 'react';
import { Search, ShieldCheck, ShieldOff } from 'lucide-react';
import api, { errMsg } from '../api/client';
import PageHeader from '../components/PageHeader';
import { useCommand } from '../lib/commands';
import Table from '../components/Table';
import StatusBadge from '../components/StatusBadge';
import ActionMenu from '../components/ActionMenu';
import Dialog from '../components/desktop/Dialog';
import ConfirmDialog from '../components/ConfirmDialog';
import PasswordInput, { StrengthBar } from '../components/PasswordInput';
import { useAuth } from '../contexts/AuthContext';
import { toast } from '../lib/toast';
import { permissionLabel } from '../lib/permissions';

/**
 * D36: who can sign in, their role and their security status. Changes go through dialogs that
 * say what will happen; nobody can lock themselves out (the server checks this too).
 */

interface Role { id: string; name: string; permissions: string[]; isSystem: boolean }
interface UserRow {
  id: string; username: string; isActive: boolean; isTwoFactorEnabled: boolean;
  role: Role | null; roleId: string | null; createdAt: string;
}

const when = (iso?: string) => {
  if (!iso) return '';
  const d = new Date(iso);
  return isNaN(d.getTime()) ? iso : d.toLocaleString('en-GB', { day: '2-digit', month: 'short', year: 'numeric', hour: '2-digit', minute: '2-digit' });
};

function RoleSummary({ role }: { role?: Role }) {
  if (!role) return <p className="text-xs text-gray-500">Without a role the user can sign in but open nothing.</p>;
  const isAdmin = role.name === 'Admin';
  return (
    <p className="text-xs text-gray-600">
      {isAdmin ? 'Can do everything, including users, payroll, settings and backups.'
        : role.permissions.length ? `Can use: ${role.permissions.map(permissionLabel).join(', ')}.` : 'This role allows nothing yet.'}
    </p>
  );
}

type Confirm = { kind: 'active' | '2fa'; user: UserRow } | null;

export default function Users() {
  const { user: me } = useAuth();
  const [rows, setRows] = useState<UserRow[]>([]);
  const [roles, setRoles] = useState<Role[]>([]);
  const [lastSignIn, setLastSignIn] = useState<Record<string, string>>({});
  const [search, setSearch] = useState('');
  const [creating, setCreating] = useState(false);
  const [form, setForm] = useState({ username: '', password: '', again: '', roleId: '' });
  const [editing, setEditing] = useState<UserRow | null>(null);
  const [editRole, setEditRole] = useState('');
  const [resetting, setResetting] = useState<UserRow | null>(null);
  const [resetPwd, setResetPwd] = useState({ password: '', again: '' });
  const [confirm, setConfirm] = useState<Confirm>(null);
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(false);

  const load = () => {
    api.get('/users').then((r) => setRows(r.data));
    api.get('/roles').then((r) => setRoles(r.data));
    api.get('/users/last-sign-in')
      .then((r) => setLastSignIn(Object.fromEntries((r.data as { userId: string; at: string }[]).map((x) => [x.userId, x.at]))))
      .catch(() => {});
  };
  useEffect(() => { load(); }, []);

  const openNew = () => { setForm({ username: '', password: '', again: '', roleId: '' }); setError(''); setCreating(true); };
  useCommand('new', openNew);
  useCommand('close', () => { setCreating(false); setEditing(null); setResetting(null); });

  const shown = useMemo(() => {
    const q = search.trim().toLowerCase();
    return q ? rows.filter((r) => r.username.toLowerCase().includes(q) || (r.role?.name ?? '').toLowerCase().includes(q)) : rows;
  }, [rows, search]);
  const active = rows.filter((r) => r.isActive).length;
  const withTwoFactor = rows.filter((r) => r.isActive && r.isTwoFactorEnabled).length;

  const run = async (work: () => Promise<unknown>, done: string, after?: () => void) => {
    setBusy(true);
    setError('');
    try {
      await work();
      toast(done);
      after?.();
      load();
    } catch (err) {
      const msg = errMsg(err, 'That did not work');
      setError(msg);
      toast(msg, 'error');
    } finally {
      setBusy(false);
    }
  };

  const create = (e: FormEvent) => {
    e.preventDefault();
    if (form.password !== form.again) { setError('The two passwords are different.'); return; }
    run(() => api.post('/users', { username: form.username.trim(), password: form.password, roleId: form.roleId || undefined }),
      `User ${form.username.trim()} created.`, () => setCreating(false));
  };
  const saveRole = (e: FormEvent) => {
    e.preventDefault();
    if (!editing) return;
    const role = roles.find((r) => r.id === editRole);
    run(() => api.patch(`/users/${editing.id}`, { roleId: editRole }), `${editing.username} is now ${role?.name ?? 'without a role'}.`, () => setEditing(null));
  };
  const savePassword = (e: FormEvent) => {
    e.preventDefault();
    if (!resetting) return;
    if (resetPwd.password !== resetPwd.again) { setError('The two passwords are different.'); return; }
    run(() => api.post(`/users/${resetting.id}/reset-password`, { password: resetPwd.password }),
      `New password set for ${resetting.username}. Give it to them in person.`, () => setResetting(null));
  };
  const confirmAction = () => {
    if (!confirm) return;
    const u = confirm.user;
    if (confirm.kind === '2fa') {
      run(() => api.post(`/users/${u.id}/reset-2fa`, {}), `2FA turned off for ${u.username}. They can turn it on again from the top bar.`, () => setConfirm(null));
    } else {
      run(() => api.patch(`/users/${u.id}`, { isActive: !u.isActive }),
        u.isActive ? `${u.username} can no longer sign in.` : `${u.username} can sign in again.`, () => setConfirm(null));
    }
  };

  const roleOptions = (
    <>
      <option value="">No role</option>
      {roles.map((r) => <option key={r.id} value={r.id}>{r.name}</option>)}
    </>
  );

  return (
    <div>
      <PageHeader
        title="Users"
        subtitle={`${active} active of ${rows.length} · ${withTwoFactor} with two-factor sign-in`}
        action={<button onClick={openNew} className="btn btn-primary">+ New User</button>}
      />

      <div className="flex items-center gap-3 mb-3">
        <div className="relative" style={{ width: 320 }}>
          <Search size={14} className="absolute left-2.5 top-1/2 -translate-y-1/2 text-gray-400" />
          <input className="!pl-8" placeholder="Search by name or role" value={search} onChange={(e) => setSearch(e.target.value)} aria-label="Search users" />
        </div>
        {withTwoFactor < active && (
          <p className="text-xs text-amber-700">Tip: ask every user — especially Admins and payroll staff — to turn on two-factor sign-in.</p>
        )}
      </div>

      <Table data={shown} columns={[
        { header: 'User', accessor: (r) => (
          <span className="font-medium text-gray-900">
            {r.username}{r.id === me?.id && <span className="ml-2 text-[10px] uppercase tracking-wide bg-blue-50 text-blue-700 px-1.5 py-0.5 rounded">You</span>}
          </span>
        ) },
        { header: 'Role', accessor: (r) => r.role ? (
          <span className={`text-xs px-2 py-0.5 rounded ${r.role.name === 'Admin' ? 'bg-orange-50 text-orange-700' : 'bg-gray-100 text-gray-700'}`}>{r.role.name}</span>
        ) : <span className="text-xs text-gray-400">No role</span> },
        { header: 'Status', accessor: (r) => <StatusBadge status={r.isActive ? 'ACTIVE' : 'INACTIVE'} /> },
        { header: '2FA', accessor: (r) => r.isTwoFactorEnabled
          ? <span className="inline-flex items-center gap-1 text-xs text-green-700"><ShieldCheck size={14} /> On</span>
          : <span className="inline-flex items-center gap-1 text-xs text-gray-500"><ShieldOff size={14} /> Off</span> },
        { header: 'Last sign-in', accessor: (r) => lastSignIn[r.id] ? when(lastSignIn[r.id]) : <span className="text-gray-400">Never</span> },
        { header: 'Created', accessor: (r) => when(r.createdAt) },
        { header: '', accessor: (r) => (
          <ActionMenu items={[
            { label: 'Change role…', onSelect: () => { setEditing(r); setEditRole(r.role?.id ?? r.roleId ?? ''); setError(''); } },
            { label: 'Reset password…', onSelect: () => { setResetting(r); setResetPwd({ password: '', again: '' }); setError(''); } },
            ...(r.isTwoFactorEnabled ? [{ label: 'Turn off 2FA…', onSelect: () => setConfirm({ kind: '2fa', user: r }) }] : []),
            ...(r.id === me?.id ? [] : [{ label: r.isActive ? 'Deactivate…' : 'Activate…', danger: r.isActive, onSelect: () => setConfirm({ kind: 'active', user: r }) }]),
          ]} />
        ) },
      ]} />
      {shown.length === 0 && rows.length > 0 && <p className="text-sm text-gray-500 mt-3">No user matches “{search}”.</p>}

      {creating && (
        <Dialog
          title="New user"
          width={480}
          onClose={() => !busy && setCreating(false)}
          buttons={
            <>
              <button type="submit" form="new-user" className="btn btn-primary" disabled={busy}>{busy ? 'Saving…' : 'Save'}</button>
              <button type="button" className="btn btn-secondary" disabled={busy} onClick={() => setCreating(false)}>Cancel</button>
            </>
          }
        >
          <form id="new-user" onSubmit={create} className="space-y-3">
            <label className="block text-xs font-medium text-gray-600">Username
              <input className="mt-1" required minLength={3} type="text" placeholder="Username" autoFocus value={form.username}
                onChange={(e) => setForm({ ...form, username: e.target.value })} />
              <span className="font-normal text-gray-500">At least 3 characters: letters, numbers, dots, dashes or underscores.</span>
            </label>
            <label className="block text-xs font-medium text-gray-600">Password
              <div className="mt-1"><PasswordInput required placeholder="Password (min 8 chars)" value={form.password} onChange={(v) => setForm({ ...form, password: v })} /></div>
              <StrengthBar password={form.password} />
            </label>
            <label className="block text-xs font-medium text-gray-600">Type it again
              <div className="mt-1"><PasswordInput required placeholder="Confirm password" value={form.again} onChange={(v) => setForm({ ...form, again: v })} /></div>
            </label>
            <label className="block text-xs font-medium text-gray-600">Role
              <select className="mt-1" value={form.roleId} onChange={(e) => setForm({ ...form, roleId: e.target.value })} aria-label="Role">{roleOptions}</select>
            </label>
            <RoleSummary role={roles.find((r) => r.id === form.roleId)} />
            {error && <p className="text-sm text-red-600" role="alert">{error}</p>}
          </form>
        </Dialog>
      )}

      {editing && (
        <Dialog
          title={`Change role — ${editing.username}`}
          width={460}
          onClose={() => !busy && setEditing(null)}
          buttons={
            <>
              <button type="submit" form="edit-role" className="btn btn-primary" disabled={busy || editRole === (editing.role?.id ?? '')}>Save change</button>
              <button type="button" className="btn btn-secondary" disabled={busy} onClick={() => setEditing(null)}>Cancel</button>
            </>
          }
        >
          <form id="edit-role" onSubmit={saveRole} className="space-y-3">
            <p className="text-sm text-gray-700">Now: <strong>{editing.role?.name ?? 'No role'}</strong></p>
            <label className="block text-xs font-medium text-gray-600">New role
              <select className="mt-1" value={editRole} onChange={(e) => setEditRole(e.target.value)} aria-label="New role">{roleOptions}</select>
            </label>
            <RoleSummary role={roles.find((r) => r.id === editRole)} />
            <p className="text-xs text-gray-500">The change applies the next time {editing.username} opens a screen.</p>
            {error && <p className="text-sm text-red-600" role="alert">{error}</p>}
          </form>
        </Dialog>
      )}

      {resetting && (
        <Dialog
          title={`Reset password — ${resetting.username}`}
          width={440}
          onClose={() => !busy && setResetting(null)}
          buttons={
            <>
              <button type="submit" form="reset-pw" className="btn btn-primary" disabled={busy}>Set password</button>
              <button type="button" className="btn btn-secondary" disabled={busy} onClick={() => setResetting(null)}>Cancel</button>
            </>
          }
        >
          <form id="reset-pw" onSubmit={savePassword} className="space-y-3">
            <PasswordInput autoFocus required placeholder="New password (min 8 chars)" value={resetPwd.password} onChange={(v) => setResetPwd({ ...resetPwd, password: v })} />
            <StrengthBar password={resetPwd.password} />
            <PasswordInput required placeholder="Type it again" value={resetPwd.again} onChange={(v) => setResetPwd({ ...resetPwd, again: v })} />
            <p className="text-xs text-gray-500">Give the new password to {resetting.username} in person, not by email.</p>
            {error && <p className="text-sm text-red-600" role="alert">{error}</p>}
          </form>
        </Dialog>
      )}

      {confirm && (
        <ConfirmDialog
          title={confirm.kind === '2fa' ? 'Turn off two-factor sign-in' : confirm.user.isActive ? 'Deactivate user' : 'Activate user'}
          confirmLabel={confirm.kind === '2fa' ? 'Turn off 2FA' : confirm.user.isActive ? 'Deactivate' : 'Activate'}
          danger={confirm.kind === 'active' && confirm.user.isActive}
          busy={busy}
          onConfirm={confirmAction}
          onCancel={() => setConfirm(null)}
        >
          {confirm.kind === '2fa' ? (
            <p>{confirm.user.username} will sign in with the password only, until they turn 2FA on again. Use this when they lose their phone.</p>
          ) : confirm.user.isActive ? (
            <p><strong>{confirm.user.username}</strong> will not be able to sign in. Their work and history stay; you can activate them again later.</p>
          ) : (
            <p><strong>{confirm.user.username}</strong> will be able to sign in again with their current password.</p>
          )}
          {error && <p className="text-red-600" role="alert">{error}</p>}
        </ConfirmDialog>
      )}
    </div>
  );
}
