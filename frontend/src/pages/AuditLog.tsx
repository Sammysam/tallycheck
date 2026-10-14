import { useEffect, useState } from 'react';
import api from '../api/client';
import PageHeader from '../components/PageHeader';
import Table from '../components/Table';
import Pagination from '../components/Pagination';

interface LogRow {
  id: string;
  username: string;
  action: string;
  detail: string | null;
  method: string;
  path: string;
  createdAt: string;
}
interface LogUser { userId: string; username: string }

export default function AuditLog() {
  const [rows, setRows] = useState<LogRow[]>([]);
  const [users, setUsers] = useState<LogUser[]>([]);
  const [userId, setUserId] = useState('');
  const [page, setPage] = useState(1);
  const [totalPages, setTotalPages] = useState(1);
  const [total, setTotal] = useState(0);
  const limit = 20;

  const load = (p = page, uid = userId) =>
    api.get('/audit-logs', { params: { page: p, limit, userId: uid || undefined } }).then((r) => {
      setRows(r.data.data);
      setTotal(r.data.total);
      setTotalPages(r.data.totalPages);
    });

  useEffect(() => { api.get('/audit-logs/users').then((r) => setUsers(r.data)); }, []);
  useEffect(() => { load(1, userId); setPage(1); /* eslint-disable-next-line */ }, [userId]);

  const fmt = (v: string) => {
    const d = new Date(v);
    return isNaN(d.getTime()) ? v : d.toLocaleString('en-GB', {
      day: '2-digit', month: 'short', year: 'numeric', hour: '2-digit', minute: '2-digit', second: '2-digit',
    });
  };

  return (
    <div>
      <PageHeader
        title="Activity Log"
        action={
          <select value={userId} onChange={(e) => setUserId(e.target.value)}>
            <option value="">All users</option>
            {users.map((u) => <option key={u.userId} value={u.userId}>{u.username}</option>)}
          </select>
        }
      />

      <Table data={rows} columns={[
        { header: 'Date & Time', accessor: (r) => fmt(r.createdAt) },
        { header: 'User',        accessor: (r) => r.username || '—' },
        { header: 'Activity',    accessor: (r) => r.action },
        { header: 'Details',     accessor: (r) => r.detail || '—' },
      ]} />

      <Pagination page={page} totalPages={totalPages} total={total} limit={limit} onPage={(p) => { setPage(p); load(p, userId); }} />
    </div>
  );
}
