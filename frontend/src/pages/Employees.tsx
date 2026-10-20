import { useEffect, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import api, { errMsg } from '../api/client';
import PageHeader from '../components/PageHeader';
import Table from '../components/Table';
import StatusBadge from '../components/StatusBadge';
import ProtectedImage from '../components/ProtectedImage';
import EmployeeFields from '../components/EmployeeFields';
import {
  STATUS_LABEL,
  emptyEmployeeForm,
  initials,
  toEmployeeBody,
  type Employee,
  type EmployeeForm,
  type EmployeeStatus,
} from '../lib/employees';

export default function Employees() {
  const navigate = useNavigate();
  const [rows, setRows] = useState<Employee[]>([]);
  const [q, setQ] = useState('');
  const [status, setStatus] = useState<'' | EmployeeStatus>('');
  const [showForm, setShowForm] = useState(false);
  const [form, setForm] = useState<EmployeeForm>(emptyEmployeeForm);
  const [error, setError] = useState('');
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    const t = setTimeout(() => {
      api
        .get('/employees', { params: { q: q || undefined, status: status || undefined } })
        .then((r) => setRows(r.data))
        .catch(() => undefined);
    }, 200);
    return () => clearTimeout(t);
  }, [q, status]);

  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    setError('');
    setSaving(true);
    try {
      const { data } = await api.post('/employees', toEmployeeBody(form));
      setShowForm(false);
      setForm(emptyEmployeeForm);
      navigate(`/employees/${data.id}`);
    } catch (err) {
      setError(errMsg(err, 'Failed to save employee'));
    } finally {
      setSaving(false);
    }
  };

  return (
    <div>
      <PageHeader
        title="Employees"
        action={
          <button onClick={() => { setShowForm((v) => !v); setError(''); }} className="btn btn-primary">
            + New Employee
          </button>
        }
      />

      {showForm && (
        <div className="card mb-5">
          <form onSubmit={submit} className="form-grid" style={{ gridTemplateColumns: '1fr 1fr' }}>
            <EmployeeFields form={form} onChange={setForm} />
            {error && <p className="text-sm text-red-600" style={{ gridColumn: '1 / -1' }}>{error}</p>}
            <div className="flex gap-2" style={{ gridColumn: '1 / -1' }}>
              <button type="submit" className="btn btn-primary" disabled={saving}>
                {saving ? 'Saving…' : 'Save & open profile'}
              </button>
              <button type="button" onClick={() => setShowForm(false)} className="btn btn-secondary">Cancel</button>
            </div>
          </form>
        </div>
      )}

      <div className="flex flex-wrap items-center gap-2 mb-4">
        <input
          placeholder="Search by ID, name, department, title or email"
          value={q}
          onChange={(e) => setQ(e.target.value)}
          className="flex-1 min-w-[240px]"
        />
        <select value={status} onChange={(e) => setStatus(e.target.value as '' | EmployeeStatus)}>
          <option value="">All statuses</option>
          {(Object.keys(STATUS_LABEL) as EmployeeStatus[]).map((s) => (
            <option key={s} value={s}>{STATUS_LABEL[s]}</option>
          ))}
        </select>
      </div>

      <Table searchable={false}
        data={rows}
        emptyMessage="No employees yet. Add the first one with “+ New Employee”."
        columns={[
          {
            header: 'Employee',
            accessor: (r) => (
              <button
                type="button"
                onClick={() => navigate(`/employees/${r.id}`)}
                className="flex items-center gap-3 text-left group"
              >
                <ProtectedImage
                  src={r.photoKey ? `/employees/${r.id}/photo` : null}
                  alt={r.name}
                  fallback={initials(r.name)}
                  className="w-9 h-9 rounded-full text-xs shrink-0"
                />
                <span className="flex flex-col leading-tight">
                  <span className="font-medium text-gray-900 group-hover:text-blue-700">{r.name}</span>
                  <span className="text-xs text-gray-400 font-mono">{r.employeeId}</span>
                </span>
              </button>
            ),
          },
          { header: 'Job title', accessor: (r) => r.jobTitle || '—' },
          { header: 'Department', accessor: (r) => r.department || '—' },
          { header: 'Email', accessor: (r) => r.email || '—' },
          { header: 'Phone', accessor: (r) => r.phone || '—' },
          { header: 'Joined', accessor: (r) => r.joinDate || '—' },
          { header: 'Status', accessor: (r) => <StatusBadge status={r.status} /> },
          {
            header: '',
            accessor: (r) => (
              <button onClick={() => navigate(`/employees/${r.id}`)} className="btn-link text-xs">
                View profile
              </button>
            ),
          },
        ]}
      />
    </div>
  );
}
