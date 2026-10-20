import { useEffect, useState, type ComponentType } from 'react';
import { Link } from 'react-router-dom';
import {
  AlertTriangle,
  ArrowDownRight,
  ArrowUpRight,
  Banknote,
  CheckCircle2,
  FileText,
  Landmark,
  Plus,
  Receipt,
  TrendingUp,
  Wallet,
  CreditCard,
} from 'lucide-react';
import api from '../api/client';
import { useAuth } from '../contexts/AuthContext';

type Icon = ComponentType<{ size?: number; className?: string }>;
interface Metric { label: string; value: number; note: string; icon: Icon; tone: string; accent: string }
interface Month { label: string; revenue: number; expenses: number }
interface Aging { total: number; overdue: number; overdueCount: number }

const MONTHS = ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec'];
const OVERDUE_BUCKETS = ['days1_30', 'days31_60', 'days61_90', 'days90plus'];

const fmt = (v: number) =>
  new Intl.NumberFormat('en-LK', { style: 'currency', currency: 'LKR' }).format(v ?? 0);
const compact = (v: number) =>
  new Intl.NumberFormat('en-LK', { notation: 'compact', maximumFractionDigits: 1 }).format(v ?? 0);
const isoDate = (d: Date) =>
  `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`;

type Bucket = { total?: number; items?: unknown[] };

function agingOf(data: Record<string, unknown>): Aging {
  const bucket = (k: string) => (data?.[k] ?? {}) as Bucket;
  const overdue = OVERDUE_BUCKETS.reduce((s, k) => s + Number(bucket(k).total ?? 0), 0);
  const overdueCount = OVERDUE_BUCKETS.reduce((s, k) => s + (bucket(k).items?.length ?? 0), 0);
  return { total: Number(data?.grandTotal ?? 0), overdue, overdueCount };
}

export default function Dashboard() {
  const { user, hasPermission } = useAuth();
  // The year-to-date figures are company-wide reporting data, so they follow
  // the reports permission — a user without it never sees or fetches them.
  const showOverview = hasPermission('reports');
  const [metrics, setMetrics] = useState<Metric[]>([]);
  const [months, setMonths] = useState<Month[]>([]);
  const [ar, setAr] = useState<Aging | null>(null);
  const [ap, setAp] = useState<Aging | null>(null);
  const [loading, setLoading] = useState(true);

  const loadMetrics = () => {
    const now = new Date();
    const today = isoDate(now);
    const year = now.getFullYear();
    const yearStart = `${year}-01-01`;
    const monthRanges = Array.from({ length: now.getMonth() + 1 }, (_, m) => ({
      label: MONTHS[m],
      from: isoDate(new Date(year, m, 1)),
      to: isoDate(new Date(year, m + 1, 0)),
    }));
    setLoading(true);
    Promise.all([
      api.get('/reports/profit-loss', { params: { dateFrom: yearStart, dateTo: today } }),
      api.get('/reports/balance-sheet', { params: { asOf: today } }),
      api.get('/reports/ar-aging'),
      api.get('/reports/ap-aging'),
      Promise.all(monthRanges.map((r) => api.get('/reports/profit-loss', { params: { dateFrom: r.from, dateTo: r.to } }))),
    ])
      .then(([pl, bs, arRes, apRes, monthly]) => {
        const revenue = Number(pl.data.totalRevenue ?? 0);
        const net = Number(pl.data.netIncome ?? 0);
        const arA = agingOf(arRes.data);
        const apA = agingOf(apRes.data);
        setAr(arA);
        setAp(apA);
        setMetrics([
          { label: 'Total Revenue', value: revenue, note: 'Income this year', icon: TrendingUp, tone: 'bg-blue-50 text-blue-600', accent: 'from-blue-500 to-blue-400' },
          { label: 'Total Expenses', value: revenue - net, note: 'Costs this year', icon: Wallet, tone: 'bg-rose-50 text-rose-600', accent: 'from-rose-500 to-rose-400' },
          { label: 'Net Income', value: net, note: net >= 0 ? 'Profit this year' : 'Loss this year', icon: Banknote, tone: 'bg-emerald-50 text-emerald-600', accent: 'from-emerald-500 to-emerald-400' },
          { label: 'Total Assets', value: Number(bs.data.totalAssets ?? 0), note: 'As of today', icon: Landmark, tone: 'bg-violet-50 text-violet-600', accent: 'from-violet-500 to-violet-400' },
          { label: 'Receivables (AR)', value: arA.total, note: 'Customers owe you', icon: ArrowDownRight, tone: 'bg-amber-50 text-amber-600', accent: 'from-amber-500 to-amber-400' },
          { label: 'Payables (AP)', value: apA.total, note: 'You owe vendors', icon: ArrowUpRight, tone: 'bg-orange-50 text-orange-600', accent: 'from-orange-500 to-orange-400' },
        ]);
        setMonths(monthly.map((r, i) => {
          const rev = Number(r.data.totalRevenue ?? 0);
          return { label: monthRanges[i].label, revenue: rev, expenses: rev - Number(r.data.netIncome ?? 0) };
        }));
      })
      .catch(() => {})
      .finally(() => setLoading(false));
  };

  useEffect(() => { if (showOverview) loadMetrics(); }, [showOverview]);

  const hour = new Date().getHours();
  const greeting = hour < 12 ? 'Good morning' : hour < 17 ? 'Good afternoon' : 'Good evening';
  const peak = Math.max(1, ...months.flatMap((m) => [m.revenue, m.expenses]));

  const quick = [
    { label: 'New Invoice', href: '/invoices', permission: 'invoices', icon: FileText,   tone: 'bg-blue-50 text-blue-600' },
    { label: 'New Bill',    href: '/bills',    permission: 'bills',    icon: Receipt,    tone: 'bg-orange-50 text-orange-600' },
    { label: 'New Expense', href: '/expenses', permission: 'expenses', icon: Wallet,     tone: 'bg-violet-50 text-violet-600' },
    { label: 'New Payment', href: '/payments', permission: 'payments', icon: CreditCard, tone: 'bg-emerald-50 text-emerald-600' },
  ].filter((q) => hasPermission(q.permission));

  return (
    <div className="space-y-6">
      {/* Welcome banner */}
      <div className="relative overflow-hidden rounded-2xl bg-slate-900 px-7 py-6 text-white">
        <div className="absolute -right-10 -top-16 w-64 h-64 rounded-full bg-orange-500/25 blur-3xl" />
        <div className="absolute right-40 -bottom-20 w-56 h-56 rounded-full bg-blue-500/20 blur-3xl" />
        <div className="relative flex items-center justify-between gap-6">
          <div>
            <p className="text-sm text-orange-300 font-medium">{greeting},</p>
            <h1 className="text-2xl font-bold tracking-tight mt-0.5">{user?.username}</h1>
            <p className="text-sm text-slate-300 mt-1">
              {showOverview ? `Here is how the business is doing in ${new Date().getFullYear()}.` : 'Welcome back.'}
            </p>
          </div>
          {quick.length > 0 && (
            <Link
              to={quick[0].href}
              className="hidden md:inline-flex items-center gap-2 rounded-xl bg-orange-500 hover:bg-orange-600 px-4 py-2.5 text-sm font-semibold shadow-lg shadow-orange-900/30 transition-colors"
            >
              <Plus size={16} /> {quick[0].label}
            </Link>
          )}
        </div>
      </div>

      {/* Metric cards */}
      {showOverview && (loading ? (
        <div className="grid grid-cols-1 sm:grid-cols-2 xl:grid-cols-3 gap-4">
          {[...Array(6)].map((_, i) => (
            <div key={i} className="bg-white rounded-2xl border border-gray-200/80 p-5 animate-pulse">
              <div className="w-10 h-10 bg-gray-100 rounded-xl mb-4" />
              <div className="h-3 bg-gray-100 rounded w-1/3 mb-3" />
              <div className="h-7 bg-gray-100 rounded w-1/2" />
            </div>
          ))}
        </div>
      ) : (
        <div className="grid grid-cols-1 sm:grid-cols-2 xl:grid-cols-3 gap-4">
          {metrics.map((m) => (
            <div key={m.label} className="relative overflow-hidden bg-white rounded-2xl border border-gray-200/80 p-5 shadow-sm hover:shadow-md transition-shadow">
              <span className={`absolute inset-x-0 top-0 h-1 bg-gradient-to-r ${m.accent}`} />
              <div className="flex items-start justify-between">
                <div>
                  <p className="text-sm font-medium text-gray-500">{m.label}</p>
                  <p className={`mt-2 text-[26px] leading-tight font-bold tracking-tight tabular-nums ${m.value < 0 ? 'text-red-600' : 'text-gray-900'}`}>
                    {fmt(m.value)}
                  </p>
                  <p className="mt-1 text-xs text-gray-400">{m.note}</p>
                </div>
                <span className={`w-11 h-11 rounded-xl flex items-center justify-center ${m.tone}`}>
                  <m.icon size={20} />
                </span>
              </div>
            </div>
          ))}
        </div>
      ))}

      {showOverview && !loading && (
        <div className="grid grid-cols-1 xl:grid-cols-3 gap-4">
          {/* Monthly chart */}
          <div className="xl:col-span-2 bg-white rounded-2xl border border-gray-200/80 p-6 shadow-sm">
            <div className="flex items-center justify-between mb-6">
              <div>
                <h2 className="text-base font-semibold text-gray-900">Revenue and expenses</h2>
                <p className="text-xs text-gray-400 mt-0.5">By month, {new Date().getFullYear()}</p>
              </div>
              <div className="flex items-center gap-4 text-xs text-gray-500">
                <span className="flex items-center gap-1.5"><span className="w-2.5 h-2.5 rounded-sm bg-orange-500" />Revenue</span>
                <span className="flex items-center gap-1.5"><span className="w-2.5 h-2.5 rounded-sm bg-slate-300" />Expenses</span>
              </div>
            </div>
            <div className="flex items-end gap-2 h-56 border-b border-gray-100">
              {months.map((m) => (
                <div key={m.label} className="group flex-1 h-full flex flex-col justify-end items-center">
                  <div className="relative w-full flex-1 flex items-end justify-center gap-1">
                    <div className="pointer-events-none absolute -top-1 left-1/2 -translate-x-1/2 -translate-y-full whitespace-nowrap rounded-lg bg-slate-900 px-2.5 py-1.5 text-[11px] text-white opacity-0 group-hover:opacity-100 transition-opacity z-10">
                      <div>Revenue {fmt(m.revenue)}</div>
                      <div className="text-slate-300">Expenses {fmt(m.expenses)}</div>
                    </div>
                    <div className="w-1/3 max-w-5 rounded-t-md bg-gradient-to-t from-orange-600 to-orange-400 transition-all" style={{ height: `${Math.max(0, (m.revenue / peak) * 100)}%`, minHeight: m.revenue > 0 ? 3 : 0 }} />
                    <div className="w-1/3 max-w-5 rounded-t-md bg-slate-300 transition-all" style={{ height: `${Math.max(0, (m.expenses / peak) * 100)}%`, minHeight: m.expenses > 0 ? 3 : 0 }} />
                  </div>
                </div>
              ))}
            </div>
            <div className="flex gap-2 mt-2">
              {months.map((m) => (
                <span key={m.label} className="flex-1 text-center text-[11px] text-gray-400">{m.label}</span>
              ))}
            </div>
            <p className="mt-3 text-[11px] text-gray-400">Highest month: {compact(peak)} LKR</p>
          </div>

          {/* Needs attention */}
          <div className="bg-white rounded-2xl border border-gray-200/80 p-6 shadow-sm">
            <h2 className="text-base font-semibold text-gray-900">Needs attention</h2>
            <p className="text-xs text-gray-400 mt-0.5 mb-5">Overdue amounts from the ageing reports</p>
            <div className="space-y-3">
              {[
                { label: 'Overdue customer invoices', a: ar, to: '/reports/ar-aging', tone: 'amber' },
                { label: 'Overdue vendor bills', a: ap, to: '/reports/ap-aging', tone: 'red' },
              ].map(({ label, a, to, tone }) => (
                <Link key={label} to={to} className="block rounded-xl border border-gray-100 p-4 hover:border-gray-200 hover:bg-gray-50/60 transition-colors">
                  {a && a.overdue > 0 ? (
                    <div className="flex items-start gap-3">
                      <span className={`mt-0.5 w-8 h-8 rounded-lg flex items-center justify-center ${tone === 'amber' ? 'bg-amber-50 text-amber-600' : 'bg-red-50 text-red-600'}`}>
                        <AlertTriangle size={16} />
                      </span>
                      <div className="min-w-0">
                        <p className="text-sm font-medium text-gray-800">{label}</p>
                        <p className="text-lg font-bold text-gray-900 tabular-nums">{fmt(a.overdue)}</p>
                        <p className="text-xs text-gray-400">{a.overdueCount} item{a.overdueCount === 1 ? '' : 's'} past due · of {fmt(a.total)} outstanding</p>
                      </div>
                    </div>
                  ) : (
                    <div className="flex items-center gap-3">
                      <span className="w-8 h-8 rounded-lg flex items-center justify-center bg-emerald-50 text-emerald-600">
                        <CheckCircle2 size={16} />
                      </span>
                      <div>
                        <p className="text-sm font-medium text-gray-800">{label}</p>
                        <p className="text-xs text-gray-400">Nothing overdue</p>
                      </div>
                    </div>
                  )}
                </Link>
              ))}
            </div>
          </div>
        </div>
      )}

      {/* Quick actions */}
      {quick.length > 0 && (
        <div>
          <h2 className="text-sm font-semibold text-gray-700 mb-3">Quick actions</h2>
          <div className="grid grid-cols-2 lg:grid-cols-4 gap-3">
            {quick.map((q) => (
              <Link
                key={q.label}
                to={q.href}
                className="group flex items-center gap-3 bg-white rounded-2xl border border-gray-200/80 p-4 shadow-sm hover:shadow-md hover:-translate-y-0.5 transition-all"
              >
                <span className={`w-10 h-10 rounded-xl flex items-center justify-center ${q.tone}`}>
                  <q.icon size={18} />
                </span>
                <span className="text-sm font-semibold text-gray-800">{q.label}</span>
                <Plus size={16} className="ml-auto text-gray-300 group-hover:text-orange-500 transition-colors" />
              </Link>
            ))}
          </div>
        </div>
      )}
    </div>
  );
}
