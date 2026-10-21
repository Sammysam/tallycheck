import { useEffect, useState, type ReactNode } from 'react';
import { Link, useNavigate } from 'react-router-dom';
import {
  AlertTriangle, Building2, CalendarCheck, CheckCircle2, FileClock, HardDrive, KeyRound, Lock, Percent, ShieldCheck, UserPlus, Users as UsersIcon,
} from 'lucide-react';
import api from '../api/client';
import PageHeader from '../components/PageHeader';
import { useAuth } from '../contexts/AuthContext';
import { fmtDate } from '../lib/region';

/**
 * D36: the Administration home — the state of users, security, backups, closing and company
 * setup on one page, the latest activity, and the common jobs one click away.
 */

interface UserRow { id: string; isActive: boolean; isTwoFactorEnabled: boolean; role: { name: string } | null }
interface LogRow { id: string; username: string | null; action: string; createdAt: string }
interface Backup { passwordSet: boolean; settings: { enabled: boolean }; status: { lastSuccessAt?: string | null; lastError?: string | null } }

type Tone = 'ok' | 'warn' | 'info';
const TONE: Record<Tone, string> = { ok: 'text-green-700', warn: 'text-amber-700', info: 'text-gray-600' };

function Tile({ icon, title, value, note, tone = 'info', to }: { icon: ReactNode; title: string; value: ReactNode; note: ReactNode; tone?: Tone; to: string }) {
  return (
    <Link to={to} className="card !mb-0 hover:border-orange-300 transition-colors block" data-testid="admin-tile">
      <div className="flex items-center gap-2 text-xs text-gray-500 mb-1">{icon}{title}</div>
      <div className="text-lg font-semibold text-gray-900">{value}</div>
      <div className={`text-xs mt-0.5 flex items-center gap-1 ${TONE[tone]}`}>
        {tone === 'ok' ? <CheckCircle2 size={12} /> : tone === 'warn' ? <AlertTriangle size={12} /> : null}{note}
      </div>
    </Link>
  );
}

const ago = (iso?: string | null) => {
  if (!iso) return '';
  const h = (Date.now() - new Date(iso).getTime()) / 3600000;
  if (h < 1) return 'less than an hour ago';
  if (h < 24) return `${Math.floor(h)} hour${Math.floor(h) === 1 ? '' : 's'} ago`;
  const d = Math.floor(h / 24);
  return `${d} day${d === 1 ? '' : 's'} ago`;
};

export default function AdminCentre() {
  const navigate = useNavigate();
  const { user } = useAuth();
  const [users, setUsers] = useState<UserRow[] | null>(null);
  const [logs, setLogs] = useState<LogRow[]>([]);
  const [backup, setBackup] = useState<Backup | null | 'unavailable'>(null);
  /** Hours since the last good backup, worked out when it loads. */
  const [backupAge, setBackupAge] = useState(Infinity);
  const [lock, setLock] = useState<string | null | undefined>(undefined);
  const [settings, setSettings] = useState<Record<string, string>>({});
  const [taxCodes, setTaxCodes] = useState<number | null>(null);

  useEffect(() => {
    api.get('/users').then((r) => setUsers(r.data)).catch(() => setUsers([]));
    api.get('/audit-logs', { params: { page: 1, limit: 6 } }).then((r) => setLogs(r.data.data)).catch(() => {});
    api.get('/system/backup').then((r) => {
      setBackup(r.data);
      const last = (r.data as Backup).status.lastSuccessAt;
      setBackupAge(last ? (Date.now() - new Date(last).getTime()) / 3600000 : Infinity);
    }).catch(() => setBackup('unavailable'));
    api.get('/books/lock').then((r) => setLock(r.data.lockedThrough ?? null)).catch(() => setLock(null));
    api.get('/settings').then((r) => setSettings(r.data)).catch(() => {});
    api.get('/tax-codes').then((r) => setTaxCodes((Array.isArray(r.data) ? r.data : r.data.data ?? []).length)).catch(() => {});
  }, []);

  const active = users?.filter((u) => u.isActive) ?? [];
  const twoFa = active.filter((u) => u.isTwoFactorEnabled).length;
  const admins = active.filter((u) => u.role?.name === 'Admin').length;

  const companyFields = ['company.name', 'company.address', 'company.email', 'company.phone', 'region.country', 'company.logo', 'company.paymentInstructions'];
  const filled = companyFields.filter((k) => (settings[k] ?? '').trim()).length;

  const b = backup && backup !== 'unavailable' ? backup : null;
  const lastBackup = b?.status.lastSuccessAt;
  const backupStale = !lastBackup || backupAge > 26;

  const quick: [string, ReactNode, string][] = [
    ['New user', <UserPlus size={16} />, '/users'],
    ['Back up now', <HardDrive size={16} />, '/settings?tab=backup'],
    ['Lock the books', <Lock size={16} />, '/settings?tab=closing'],
    ['Taxes', <Percent size={16} />, '/taxes'],
    ['Company details', <Building2 size={16} />, '/settings?tab=company'],
    ['Activity log', <FileClock size={16} />, '/audit'],
  ];

  return (
    <div>
      <PageHeader title="Admin Centre" subtitle={`Signed in as ${user?.username ?? ''}. The state of the system at a glance; click a card to manage it.`} />

      <div className="grid gap-3 mb-5" style={{ gridTemplateColumns: 'repeat(auto-fill, minmax(230px, 1fr))' }}>
        <Tile to="/users" icon={<UsersIcon size={14} />} title="Users" value={users ? `${active.length} active` : '…'}
          note={users ? `${admins} administrator${admins === 1 ? '' : 's'} · ${users.length - active.length} inactive` : ''} />
        <Tile to="/users" icon={<ShieldCheck size={14} />} title="Two-factor sign-in" value={users ? `${twoFa} of ${active.length}` : '…'}
          tone={users && twoFa < active.length ? 'warn' : 'ok'} note={users && twoFa < active.length ? `${active.length - twoFa} user${active.length - twoFa === 1 ? '' : 's'} without 2FA` : 'Everyone uses it'} />
        <Tile to="/settings?tab=backup" icon={<HardDrive size={14} />} title="Backups"
          value={backup === 'unavailable' ? 'Main PC only' : !b ? '…' : lastBackup ? ago(lastBackup) : 'Never'}
          tone={backup === 'unavailable' ? 'info' : !b ? 'info' : !b.passwordSet || backupStale ? 'warn' : 'ok'}
          note={backup === 'unavailable' ? 'Open on the main PC as an Administrator' : !b ? '' : !b.passwordSet ? 'Set a backup password' : b.settings.enabled ? 'Daily backup is on' : 'Daily backup is off'} />
        <Tile to="/settings?tab=closing" icon={<CalendarCheck size={14} />} title="Books locked up to"
          value={lock === undefined ? '…' : lock ? fmtDate(lock) : 'Not locked'} tone={lock ? 'ok' : 'info'}
          note={lock ? 'Nothing can be posted on or before this date' : 'Lock each month once it is closed'} />
        <Tile to="/settings?tab=company" icon={<Building2 size={14} />} title="Company details" value={`${filled} of ${companyFields.length}`}
          tone={filled === companyFields.length ? 'ok' : 'warn'} note={filled === companyFields.length ? 'Complete' : 'Some details are missing from invoices'} />
        <Tile to="/taxes" icon={<Percent size={14} />} title="Tax codes" value={taxCodes === null ? '…' : `${taxCodes}`}
          note={taxCodes ? 'VAT / GST / withholding' : 'None — invoices show no tax'} />
      </div>

      <div className="grid gap-4" style={{ gridTemplateColumns: '1fr 1.6fr' }}>
        <section className="card !mb-0">
          <h3 className="text-sm font-semibold text-gray-800 mb-3">Quick actions</h3>
          <div className="grid grid-cols-2 gap-2">
            {quick.map(([label, icon, to]) => (
              <button key={label} type="button" onClick={() => navigate(to)}
                className="flex items-center gap-2 px-3 py-2.5 rounded border border-[#e5e7eb] text-sm text-gray-800 hover:border-orange-300 hover:bg-orange-50 text-left">
                <span className="text-orange-600">{icon}</span>{label}
              </button>
            ))}
          </div>
          <p className="text-xs text-gray-500 mt-3 flex items-center gap-1"><KeyRound size={12} /> Security tip: give each person their own account and the smallest role they need.</p>
        </section>

        <section className="card !mb-0">
          <div className="flex items-center mb-2">
            <h3 className="text-sm font-semibold text-gray-800">Latest activity</h3>
            <Link to="/audit" className="btn-link text-xs ml-auto">Open the activity log →</Link>
          </div>
          <table className="min-w-full text-sm">
            <tbody>
              {logs.map((l) => (
                <tr key={l.id} className="border-b border-[#f3f3f3] last:border-0">
                  <td className="py-1.5 pr-3 text-xs text-gray-500 whitespace-nowrap">{new Date(l.createdAt).toLocaleString('en-GB', { day: '2-digit', month: 'short', hour: '2-digit', minute: '2-digit' })}</td>
                  <td className="py-1.5 pr-3 text-gray-700">{l.username ?? '—'}</td>
                  <td className="py-1.5 text-gray-800">{l.action.replace(/\ban user\b/g, 'a user')}</td>
                </tr>
              ))}
              {logs.length === 0 && <tr><td className="py-3 text-gray-400 text-sm">Nothing yet.</td></tr>}
            </tbody>
          </table>
        </section>
      </div>
    </div>
  );
}
