import { useEffect, useState, type ComponentType, type ReactNode } from 'react';
import { Link } from 'react-router-dom';
import {
  AlertTriangle,
  ArrowDownRight,
  ArrowUpRight,
  Banknote,
  CheckCircle2,
  Landmark,
  TrendingUp,
  Wallet,
} from 'lucide-react';
import api from '../api/client';
import { useAuth } from '../contexts/AuthContext';
import PageHeader from '../components/PageHeader';
import { fiscalYearStart, region } from '../lib/region';
import { compactAmount, money } from '../lib/money';

type Icon = ComponentType<{ size?: number; className?: string; strokeWidth?: number }>;
interface Metric { label: string; value: number; note: string; icon: Icon; tone: string }
interface Month { label: string; revenue: number; expenses: number }
interface Aging { total: number; overdue: number; overdueCount: number }

const MONTHS = ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec'];
const OVERDUE_BUCKETS = ['days1_30', 'days31_60', 'days61_90', 'days90plus'];

const fmt = (v: number) =>
  money(Number(v ?? 0));
const compact = (v: number) =>
  compactAmount(v);
const isoDate = (d: Date) =>
  `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`;

type Bucket = { total?: number; items?: unknown[] };

function agingOf(data: Record<string, unknown>): Aging {
  const bucket = (k: string) => (data?.[k] ?? {}) as Bucket;
  const overdue = OVERDUE_BUCKETS.reduce((s, k) => s + Number(bucket(k).total ?? 0), 0);
  const overdueCount = OVERDUE_BUCKETS.reduce((s, k) => s + (bucket(k).items?.length ?? 0), 0);
  return { total: Number(data?.grandTotal ?? 0), overdue, overdueCount };
}

/** A titled group box, the desktop way of framing a block of information. */
function Panel({ title, note, children, className = '' }: { title: string; note?: string; children: ReactNode; className?: string }) {
  return (
    <section className={`bg-white border border-[#e0e0e0] rounded-md ${className}`}>
      <header className="flex items-baseline justify-between gap-3 px-3 py-2 border-b border-[#ececec] bg-[#fafafa] rounded-t-md">
        <h2 className="text-sm font-semibold text-[#1b1b1b]">{title}</h2>
        {note && <span className="text-xs text-gray-500">{note}</span>}
      </header>
      <div className="p-3">{children}</div>
    </section>
  );
}

export default function Dashboard() {
  const { hasPermission } = useAuth();
  // The year-to-date figures are company-wide reporting data, so they follow
  // the reports permission — a user without it never sees or fetches them.
  const showOverview = hasPermission('reports');
  const [metrics, setMetrics] = useState<Metric[]>([]);
  const [months, setMonths] = useState<Month[]>([]);
  const [ar, setAr] = useState<Aging | null>(null);
  const [ap, setAp] = useState<Aging | null>(null);
  const [loading, setLoading] = useState(true);
  // D25: figures run from the start of the company's financial year.
  const yearStart = fiscalYearStart();

  const loadMetrics = () => {
    const now = new Date();
    const today = isoDate(now);
    const [startYear, startMonth] = yearStart.split('-').map(Number);
    const count = (now.getFullYear() - startYear) * 12 + now.getMonth() - (startMonth - 1) + 1;
    const monthRanges = Array.from({ length: count }, (_, i) => {
      const first = new Date(startYear, startMonth - 1 + i, 1);
      return {
        label: MONTHS[first.getMonth()],
        from: isoDate(first),
        to: isoDate(new Date(first.getFullYear(), first.getMonth() + 1, 0)),
      };
    });
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
          { label: 'Total Revenue', value: revenue, note: 'Income this year', icon: TrendingUp, tone: 'text-blue-600' },
          { label: 'Total Expenses', value: revenue - net, note: 'Costs this year', icon: Wallet, tone: 'text-rose-600' },
          { label: 'Net Income', value: net, note: net >= 0 ? 'Profit this year' : 'Loss this year', icon: Banknote, tone: 'text-emerald-600' },
          { label: 'Total Assets', value: Number(bs.data.totalAssets ?? 0), note: 'As of today', icon: Landmark, tone: 'text-violet-600' },
          { label: 'Receivables (AR)', value: arA.total, note: 'Customers owe you', icon: ArrowDownRight, tone: 'text-amber-600' },
          { label: 'Payables (AP)', value: apA.total, note: 'You owe vendors', icon: ArrowUpRight, tone: 'text-orange-600' },
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

  const peak = Math.max(1, ...months.flatMap((m) => [m.revenue, m.expenses]));

  if (!showOverview) {
    return (
      <div>
        <PageHeader title="Dashboard" />
        <p className="text-sm text-gray-600">Use the menu bar, the toolbar or the navigation pane to open a screen.</p>
      </div>
    );
  }

  return (
    <div>
      <PageHeader title="Dashboard" subtitle={`Financial year from ${yearStart}. Press F5 to refresh.`} />

      {/* Key figures */}
      <div className="grid grid-cols-2 lg:grid-cols-3 2xl:grid-cols-6 gap-2 mb-3">
        {(loading ? Array.from({ length: 6 }, () => null) : metrics).map((m, i) =>
          m === null ? (
            <div key={i} className="bg-white border border-[#e0e0e0] rounded-md px-3 py-2.5 h-[74px] animate-pulse" />
          ) : (
            <div key={m.label} className="bg-white border border-[#e0e0e0] rounded-md px-3 py-2.5">
              <div className="flex items-center gap-1.5 text-xs text-gray-600">
                <m.icon size={14} strokeWidth={2} className={m.tone} />
                {m.label}
              </div>
              <p className={`mt-1 text-lg font-semibold tabular-nums ${m.value < 0 ? 'text-red-600' : 'text-[#1b1b1b]'}`}>{fmt(m.value)}</p>
              <p className="text-[11px] text-gray-500">{m.note}</p>
            </div>
          ),
        )}
      </div>

      {!loading && (
        <div className="grid grid-cols-1 xl:grid-cols-3 gap-3">
          <Panel title="Revenue and expenses" note={`By month from ${yearStart} · highest ${compact(peak)} ${region().currency}`} className="xl:col-span-2">
            <div className="flex items-center gap-4 text-xs text-gray-600 mb-2">
              <span className="flex items-center gap-1.5"><span className="w-2.5 h-2.5 bg-orange-500" />Revenue</span>
              <span className="flex items-center gap-1.5"><span className="w-2.5 h-2.5 bg-slate-400" />Expenses</span>
            </div>
            <div className="flex items-end gap-2 h-48 border-b border-l border-[#d9d9d9] px-1">
              {months.map((m) => (
                <div
                  key={m.label}
                  className="flex-1 h-full flex items-end justify-center gap-0.5"
                  title={`${m.label}: revenue ${fmt(m.revenue)}, expenses ${fmt(m.expenses)}`}
                >
                  <div className="w-1/3 max-w-4 bg-orange-500" style={{ height: `${Math.max(0, (m.revenue / peak) * 100)}%`, minHeight: m.revenue > 0 ? 2 : 0 }} />
                  <div className="w-1/3 max-w-4 bg-slate-400" style={{ height: `${Math.max(0, (m.expenses / peak) * 100)}%`, minHeight: m.expenses > 0 ? 2 : 0 }} />
                </div>
              ))}
            </div>
            <div className="flex gap-2 mt-1 px-1">
              {months.map((m) => (
                <span key={m.label} className="flex-1 text-center text-[11px] text-gray-500">{m.label}</span>
              ))}
            </div>
          </Panel>

          <Panel title="Needs attention" note="From the ageing reports">
            <div className="divide-y divide-[#efefef]">
              {[
                { label: 'Overdue customer invoices', a: ar, to: '/reports/ar-aging' },
                { label: 'Overdue vendor bills', a: ap, to: '/reports/ap-aging' },
              ].map(({ label, a, to }) => (
                <Link key={label} to={to} className="flex items-start gap-2.5 py-2.5 first:pt-0 last:pb-0 hover:bg-[#f7f7f7] -mx-1 px-1 rounded">
                  {a && a.overdue > 0 ? (
                    <>
                      <AlertTriangle size={16} className="mt-0.5 text-amber-600 shrink-0" />
                      <div className="min-w-0">
                        <p className="text-sm text-[#1b1b1b]">{label}</p>
                        <p className="text-base font-semibold tabular-nums">{fmt(a.overdue)}</p>
                        <p className="text-xs text-gray-500">{a.overdueCount} item{a.overdueCount === 1 ? '' : 's'} past due · of {fmt(a.total)} outstanding</p>
                      </div>
                    </>
                  ) : (
                    <>
                      <CheckCircle2 size={16} className="mt-0.5 text-emerald-600 shrink-0" />
                      <div>
                        <p className="text-sm text-[#1b1b1b]">{label}</p>
                        <p className="text-xs text-gray-500">Nothing overdue</p>
                      </div>
                    </>
                  )}
                </Link>
              ))}
            </div>
          </Panel>
        </div>
      )}
    </div>
  );
}
