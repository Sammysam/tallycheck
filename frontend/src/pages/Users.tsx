import { useEffect, useState } from 'react';
import api from '../api/client';
import PageHeader from '../components/PageHeader';
import Table from '../components/Table';
import StatusBadge from '../components/StatusBadge';

interface Role { id: string; name: string; permissions: string[]; isSystem: boolean }
interface UserRow { id: string; username: string; isActive: boolean; role: Role | null }

function PasswordInput({
  value,
  onChange,
  placeholder,
  minLength = 8,
  required,
  autoFocus,
}: {
  value: string;
  onChange: (v: string) => void;
  placeholder?: string;
  minLength?: number;
  required?: boolean;
  autoFocus?: boolean;
}) {
  const [show, setShow] = useState(false);
  return (
    <div style={{ position: 'relative', display: 'flex', alignItems: 'center' }}>
      <input
        required={required}
        autoFocus={autoFocus}
        type={show ? 'text' : 'password'}
        placeholder={placeholder}
        minLength={minLength}
        value={value}
        onChange={(e) => onChange(e.target.value)}
        style={{ width: '100%', paddingRight: '2.25rem' }}
      />
      <button
        type="button"
        onClick={() => setShow((s) => !s)}
        title={show ? 'Hide password' : 'Show password'}
        aria-label={show ? 'Hide password' : 'Show password'}
        style={{
          position: 'absolute', right: '0.5rem', background: 'none', border: 'none',
          cursor: 'pointer', padding: 0, display: 'flex', color: '#6b7280',
        }}
      >
        {show ? (
          // eye-off icon
          <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
            <path d="M17.94 17.94A10.07 10.07 0 0 1 12 20c-7 0-11-8-11-8a18.45 18.45 0 0 1 5.06-5.94M9.9 4.24A9.12 9.12 0 0 1 12 4c7 0 11 8 11 8a18.5 18.5 0 0 1-2.16 3.19m-6.72-1.07a3 3 0 1 1-4.24-4.24" />
            <line x1="1" y1="1" x2="23" y2="23" />
          </svg>
        ) : (
          // eye icon
          <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
            <path d="M1 12s4-8 11-8 11 8 11 8-4 8-11 8-11-8-11-8z" />
            <circle cx="12" cy="12" r="3" />
          </svg>
        )}
      </button>
    </div>
  );
}

export default function Users() {
  const [rows, setRows] = useState<UserRow[]>([]);
  const [roles, setRoles] = useState<Role[]>([]);
  const [showForm, setShowForm] = useState(false);
  const [form, setForm] = useState({ username: '', password: '', roleId: '' });
  const [error, setError] = useState('');

  // reset-password inline box state
  const [resetTarget, setResetTarget] = useState<UserRow | null>(null);
  const [resetPwd, setResetPwd] = useState('');
  const [resetError, setResetError] = useState('');

  const load = () => {
    api.get('/users').then((r) => setRows(r.data));
    api.get('/roles').then((r) => setRoles(r.data));
  };
  useEffect(() => { load(); }, []);

  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    setError('');
    try {
      await api.post('/users', { ...form, roleId: form.roleId || undefined });
      setShowForm(false);
      setForm({ username: '', password: '', roleId: '' });
      load();
    } catch (err: any) {
      setError(err?.response?.data?.message ?? 'Failed to create user');
    }
  };

  const changeRole = async (id: string, roleId: string) => {
    try {
      await api.patch(`/users/${id}`, { roleId });
      load();
    } catch (err: any) {
      alert(err?.response?.data?.message ?? 'Failed to update role');
    }
  };

  const toggleActive = async (row: UserRow) => {
    try {
      await api.patch(`/users/${row.id}`, { isActive: !row.isActive });
      load();
    } catch (err: any) {
      alert(err?.response?.data?.message ?? 'Failed to update user');
    }
  };

  const openReset = (row: UserRow) => {
    setResetTarget(row);
    setResetPwd('');
    setResetError('');
  };

  const submitReset = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!resetTarget) return;
    if (resetPwd.length < 8) {
      setResetError('Password must be at least 8 characters');
      return;
    }
    try {
      await api.post(`/users/${resetTarget.id}/reset-password`, { password: resetPwd });
      alert(`Password reset for ${resetTarget.username}.`);
      setResetTarget(null);
      setResetPwd('');
    } catch (err: any) {
      setResetError(err?.response?.data?.message ?? 'Failed to reset password');
    }
  };

  const reset2FA = async (row: UserRow) => {
    if (!window.confirm(`Reset 2FA for ${row.username}? They will sign in with their password only until they turn 2FA on again.`)) return;
    try {
      await api.post(`/users/${row.id}/reset-2fa`, {});
      alert(`2FA reset for ${row.username}. They can turn it on again from the top bar.`);
      load();
    } catch (err: any) {
      alert(err?.response?.data?.message ?? 'Failed to reset 2FA');
    }
  };

  return (
    <div>
      <PageHeader
        title="Users"
        action={<button onClick={() => setShowForm(!showForm)} className="btn btn-primary">+ New User</button>}
      />

      {showForm && (
        <div className="card mb-5">
          <form onSubmit={submit} className="form-grid" style={{ gridTemplateColumns: '1fr 1fr 1fr' }}>
            <input required type="text" placeholder="Username" value={form.username} onChange={(e) => setForm({ ...form, username: e.target.value })} />
            <PasswordInput required placeholder="Password (min 8 chars)" value={form.password} onChange={(v) => setForm({ ...form, password: v })} />
            <select value={form.roleId} onChange={(e) => setForm({ ...form, roleId: e.target.value })}>
              <option value="">No role</option>
              {roles.map((r) => <option key={r.id} value={r.id}>{r.name}</option>)}
            </select>
            {error && <p className="text-sm text-red-600" style={{ gridColumn: '1 / -1' }}>{error}</p>}
            <div className="flex gap-2" style={{ gridColumn: '1 / -1' }}>
              <button type="submit" className="btn btn-primary">Save</button>
              <button type="button" onClick={() => setShowForm(false)} className="btn btn-secondary">Cancel</button>
            </div>
          </form>
        </div>
      )}

      {resetTarget && (
        <div className="card mb-5">
          <form onSubmit={submitReset} className="form-grid" style={{ gridTemplateColumns: '1fr auto' }}>
            <p className="text-sm" style={{ gridColumn: '1 / -1', marginBottom: '0.5rem' }}>
              Set a new password for <strong>{resetTarget.username}</strong>
            </p>
            <PasswordInput autoFocus placeholder="New password (min 8 chars)" value={resetPwd} onChange={setResetPwd} />
            <div className="flex gap-2">
              <button type="submit" className="btn btn-primary">Save Password</button>
              <button type="button" onClick={() => setResetTarget(null)} className="btn btn-secondary">Cancel</button>
            </div>
            {resetError && <p className="text-sm text-red-600" style={{ gridColumn: '1 / -1' }}>{resetError}</p>}
          </form>
        </div>
      )}

      <Table data={rows} columns={[
        { header: 'Username', accessor: 'username' },
        {
          header: 'Role',
          accessor: (r) => (
            <select value={r.role?.id ?? ''} onChange={(e) => changeRole(r.id, e.target.value)}>
              <option value="" disabled>No role</option>
              {roles.map((role) => <option key={role.id} value={role.id}>{role.name}</option>)}
            </select>
          ),
        },
        { header: 'Status', accessor: (r) => <StatusBadge status={r.isActive ? 'ACTIVE' : 'INACTIVE'} /> },
        {
          header: '',
          accessor: (r) => (
            <div className="flex gap-3">
              <button onClick={() => toggleActive(r)} className="btn-link text-xs">
                {r.isActive ? 'Deactivate' : 'Activate'}
              </button>
              <button onClick={() => openReset(r)} className="btn-link text-xs">Reset Password</button>
              <button onClick={() => reset2FA(r)} className="btn-link text-xs">Reset 2FA</button>
            </div>
          ),
        },
      ]} />
    </div>
  );
}
