import { useEffect, useState } from 'react';
import api from '../api/client';
import { useAuth } from '../contexts/AuthContext';

interface Metric { label: string; value: string; color: string; bg: string }

export default function Dashboard() {
  const { hasPermission } = useAuth();
  // The year-to-date figures are company-wide reporting data, so they follow
  // the reports permission — a user without it never sees or fetches them.
  const showOverview = hasPermission('reports');
  const [metrics, setMetrics] = useState<Metric[]>([]);
  const [loading, setLoading] = useState(true);

  const fmt = (v: number) =>
    new Intl.NumberFormat('en-LK', { style: 'currency', currency: 'LKR' }).format(v ?? 0);

  const loadMetrics = () => {
    const today = new Date().toISOString().split('T')[0];
    const yearStart = `${new Date().getFullYear()}-01-01`;
    setLoading(true);
    Promise.all([
      api.get('/reports/profit-loss', { params: { dateFrom: yearStart, dateTo: today } }),
      api.get('/reports/balance-sheet', { params: { asOf: today } }),
      api.get('/reports/ar-aging'),
      api.get('/reports/ap-aging'),
    ])
      .then(([pl, bs, ar, ap]) => {
        setMetrics([
          { label: 'Net Income (YTD)',     value: fmt(pl.data.netIncome),              color: 'text-emerald-700', bg: 'bg-emerald-50' },
          { label: 'Total Revenue (YTD)',  value: fmt(pl.data.totalRevenue),           color: 'text-blue-700',    bg: 'bg-blue-50' },
          { label: 'Total Assets',         value: fmt(bs.data.totalAssets),            color: 'text-violet-700',  bg: 'bg-violet-50' },
          { label: 'Total Expenses (YTD)', value: fmt(pl.data.totalOperatingExpenses), color: 'text-gray-700',    bg: 'bg-gray-50' },
          { label: 'AR Outstanding',       value: fmt(ar.data.grandTotal),             color: 'text-orange-700',  bg: 'bg-orange-50' },
          { label: 'AP Outstanding',       value: fmt(ap.data.grandTotal),             color: 'text-red-700',     bg: 'bg-red-50' },
        ]);
      })
      .catch(() => {})
      .finally(() => setLoading(false));
  };

  useEffect(() => { if (showOverview) loadMetrics(); }, [showOverview]);

  return (
    <div className="space-y-6">

      {/* Header row */}
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-xl font-bold text-gray-900 tracking-tight">Dashboard</h1>
          {showOverview && <p className="text-sm text-gray-400 mt-0.5">Year-to-date overview</p>}
        </div>
      </div>

      {/* Metric cards */}
      {showOverview && (loading ? (
        <div className="grid grid-cols-3 gap-4">
          {[...Array(6)].map((_, i) => (
            <div key={i} className="bg-white rounded-xl border border-gray-200 p-5 animate-pulse">
              <div className="h-3 bg-gray-100 rounded w-2/3 mb-3" />
              <div className="h-7 bg-gray-100 rounded w-1/2" />
            </div>
          ))}
        </div>
      ) : (
        <div className="grid grid-cols-3 gap-4">
          {metrics.map((m) => (
            <div key={m.label} className="bg-white rounded-xl border border-gray-200 p-5 hover:shadow-sm transition-shadow">
              <p className="text-xs font-medium text-gray-500 mb-2 uppercase tracking-wide">{m.label}</p>
              <p className={`text-2xl font-bold tracking-tight ${m.color}`}>{m.value}</p>
            </div>
          ))}
        </div>
      ))}

      {/* Quick links */}
      <div>
        <h2 className="text-sm font-semibold text-gray-500 uppercase tracking-wide mb-3">Quick Actions</h2>
        <div className="grid grid-cols-4 gap-3">
          {[
            { label: 'New Invoice',  href: '/invoices',  permission: 'invoices', color: 'text-blue-600',   bg: 'hover:bg-blue-50'   },
            { label: 'New Bill',     href: '/bills',     permission: 'bills',    color: 'text-orange-600', bg: 'hover:bg-orange-50' },
            { label: 'New Expense',  href: '/expenses',  permission: 'expenses', color: 'text-violet-600', bg: 'hover:bg-violet-50' },
            { label: 'New Payment',  href: '/payments',  permission: 'payments', color: 'text-emerald-600',bg: 'hover:bg-emerald-50'},
          ].filter((q) => hasPermission(q.permission)).map((q) => (
            <a
              key={q.label}
              href={q.href}
              className={`bg-white rounded-xl border border-gray-200 p-4 text-sm font-medium ${q.color} ${q.bg} transition-colors text-center`}
            >
              {q.label}
            </a>
          ))}
        </div>
      </div>
    </div>
  );
}
