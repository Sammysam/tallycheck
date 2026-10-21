import { useCallback, useEffect, useState } from 'react';
import { Download, FileText, KeyRound, LogIn, Search, Settings2, UserCog } from 'lucide-react';
import api, { errMsg } from '../api/client';
import PageHeader from '../components/PageHeader';
import Table from '../components/Table';
import Pagination from '../components/Pagination';
import { toast } from '../lib/toast';

/** D36: who did what, and when — filtered by user, dates, kind of activity and text. */

interface LogRow {
  id: string;
  username: string | null;
  action: string;
  detail: string | null;
  method: string | null;
  path: string | null;
  statusCode: number | null;
  createdAt: string;
}
interface LogUser { userId: string; username: string }

const KINDS = [
  { value: '', label: 'All activity' },
  { value: 'signin', label: 'Sign-ins' },
  { value: 'users', label: 'Users & roles' },
  { value: 'settings', label: 'Settings & taxes' },
  { value: 'documents', label: 'Documents & records' },
];

/** The kind of an activity, for its icon (same words the server filters on). */
function kindOf(action: string) {
  const a = action.toLowerCase();
  if (/login|^logged|2fa|two-factor/.test(a)) return a.includes('failed') ? 'failed' : 'signin';
  if (/user|role|password|administrator/.test(a)) return 'users';
  if (/setting|tax|lock|backup|restore/.test(a)) return 'settings';
  return 'documents';
}
const ICON = {
  signin: { icon: LogIn, color: '#2563eb' },
  failed: { icon: KeyRound, color: '#dc2626' },
  users: { icon: UserCog, color: '#7c3aed' },
  settings: { icon: Settings2, color: '#ea580c' },
  documents: { icon: FileText, color: '#475569' },
};

/** The reference wrote "an user"; shown in plain English (the stored text is unchanged). */
const tidy = (action: string) => action.replace(/\ban user\b/g, 'a user');

const stamp = (v: string) => {
  const d = new Date(v);
  return isNaN(d.getTime()) ? v : d.toLocaleString('en-GB', { day: '2-digit', month: 'short', year: 'numeric', hour: '2-digit', minute: '2-digit', second: '2-digit' });
};

export default function AuditLog() {
  const [rows, setRows] = useState<LogRow[]>([]);
  const [users, setUsers] = useState<LogUser[]>([]);
  const [filter, setFilter] = useState({ userId: '', dateFrom: '', dateTo: '', kind: '', search: '' });
  const [applied, setApplied] = useState(filter);
  const [page, setPage] = useState(1);
  const [totalPages, setTotalPages] = useState(1);
  const [total, setTotal] = useState(0);
  const [exporting, setExporting] = useState(false);
  const limit = 20;

  const params = useCallback((f: typeof filter, p: number, l: number) => ({
    page: p, limit: l,
    userId: f.userId || undefined, dateFrom: f.dateFrom || undefined, dateTo: f.dateTo || undefined,
    kind: f.kind || undefined, search: f.search.trim() || undefined,
  }), []);

  useEffect(() => { api.get('/audit-logs/users').then((r) => setUsers(r.data)); }, []);
  useEffect(() => {
    api.get('/audit-logs', { params: params(applied, page, limit) }).then((r) => {
      setRows(r.data.data);
      setTotal(r.data.total);
      setTotalPages(r.data.totalPages);
    });
  }, [applied, page, params]);

  // Lists and dates apply at once; the search box applies on Enter or the Search button.
  const change = (patch: Partial<typeof filter>, now = true) => {
    const next = { ...filter, ...patch };
    setFilter(next);
    if (now) { setApplied(next); setPage(1); }
  };
  const clear = () => {
    const empty = { userId: '', dateFrom: '', dateTo: '', kind: '', search: '' };
    setFilter(empty);
    setApplied(empty);
    setPage(1);
  };
  const filtered = Object.values(applied).some(Boolean);

  /** Every matching row (not only this page) as a CSV that Excel opens. */
  const exportCsv = async () => {
    setExporting(true);
    try {
      const all: LogRow[] = [];
      for (let p = 1; p <= 100; p++) {
        const r = await api.get('/audit-logs', { params: params(applied, p, 100) });
        all.push(...r.data.data);
        if (p >= r.data.totalPages) break;
      }
      const cell = (v: unknown) => `"${String(v ?? '').replace(/"/g, '""')}"`;
      const lines = [['Date & time', 'User', 'Activity', 'Details'].map(cell).join(','),
        ...all.map((r) => [stamp(r.createdAt), r.username ?? '', tidy(r.action), r.detail ?? ''].map(cell).join(','))];
      const blob = new Blob(['﻿' + lines.join('\r\n')], { type: 'text/csv;charset=utf-8' });
      const a = document.createElement('a');
      a.href = URL.createObjectURL(blob);
      a.download = `Activity Log ${new Date().toISOString().slice(0, 10)}.csv`;
      a.click();
      URL.revokeObjectURL(a.href);
      toast(`Exported ${all.length} row${all.length === 1 ? '' : 's'}.`);
    } catch (e) {
      toast(errMsg(e, 'Could not export the log'), 'error');
    } finally {
      setExporting(false);
    }
  };

  return (
    <div>
      <PageHeader
        title="Activity Log"
        subtitle="Every sign-in and every change, with who made it and when. The log cannot be edited."
        action={<button className="btn btn-secondary inline-flex items-center gap-1.5" onClick={exportCsv} disabled={exporting || total === 0}>
          <Download size={14} /> {exporting ? 'Exporting…' : 'Export to Excel'}
        </button>}
      />

      <div className="card !py-3 mb-3">
        <div className="grid gap-2 items-end" style={{ gridTemplateColumns: '1.2fr 1.2fr 1fr 1fr 2fr auto' }}>
          <label className="text-xs font-medium text-gray-600">User
            <select className="mt-1" value={filter.userId} onChange={(e) => change({ userId: e.target.value })} aria-label="User">
              <option value="">All users</option>
              {users.map((u) => <option key={u.userId} value={u.userId}>{u.username}</option>)}
            </select>
          </label>
          <label className="text-xs font-medium text-gray-600">Kind
            <select className="mt-1" value={filter.kind} onChange={(e) => change({ kind: e.target.value })} aria-label="Kind of activity">
              {KINDS.map((k) => <option key={k.value} value={k.value}>{k.label}</option>)}
            </select>
          </label>
          <label className="text-xs font-medium text-gray-600">From
            <input className="mt-1" type="date" value={filter.dateFrom} onChange={(e) => change({ dateFrom: e.target.value })} aria-label="From" />
          </label>
          <label className="text-xs font-medium text-gray-600">To
            <input className="mt-1" type="date" value={filter.dateTo} onChange={(e) => change({ dateTo: e.target.value })} aria-label="To" />
          </label>
          <form className="text-xs font-medium text-gray-600" onSubmit={(e) => { e.preventDefault(); change({}, true); }}>
            Search
            <div className="relative mt-1">
              <Search size={14} className="absolute left-2.5 top-1/2 -translate-y-1/2 text-gray-400" />
              <input className="!pl-8" placeholder="Activity, details or user — press Enter" value={filter.search}
                onChange={(e) => change({ search: e.target.value }, false)} aria-label="Search the log" />
            </div>
          </form>
          <button className="btn btn-secondary" onClick={clear} disabled={!filtered && !filter.search}>Clear</button>
        </div>
      </div>

      <p className="text-xs text-gray-500 mb-2">{total} {total === 1 ? 'entry' : 'entries'}{filtered ? (total === 1 ? ' matches the filters' : ' match the filters') : ''}.</p>

      <Table data={rows} columns={[
        { header: 'Date & Time', accessor: (r) => <span className="whitespace-nowrap">{stamp(r.createdAt)}</span> },
        { header: 'User',        accessor: (r) => r.username || '—' },
        { header: 'Activity',    accessor: (r) => {
          const k = ICON[kindOf(r.action) as keyof typeof ICON];
          const Icon = k.icon;
          return <span className="inline-flex items-center gap-2"><Icon size={14} style={{ color: k.color }} />{tidy(r.action)}</span>;
        } },
        { header: 'Details',     accessor: (r) => r.detail || <span className="text-gray-400">—</span> },
      ]} />

      <Pagination page={page} totalPages={totalPages} total={total} limit={limit} onPage={setPage} />
    </div>
  );
}
