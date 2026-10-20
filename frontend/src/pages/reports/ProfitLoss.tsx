import { useEffect, useState } from 'react';
import api from '../../api/client';
import PageHeader from '../../components/PageHeader';
import { ReportTable } from '../../components/ReportTable';
import { sectionRows, type AccountLine } from '../../lib/reportRows';

interface ProfitLossData {
  revenue: AccountLine[];
  cogs: AccountLine[];
  operatingExpenses: AccountLine[];
  totalRevenue: number;
  totalCogs: number;
  totalOperatingExpenses: number;
  grossProfit: number;
  netIncome: number;
}

export default function ProfitLoss() {
  const [data, setData] = useState<ProfitLossData | null>(null);
  const today = new Date().toISOString().split('T')[0];
  const yearStart = `${new Date().getFullYear()}-01-01`;
  const [dateFrom, setDateFrom] = useState(yearStart);
  const [dateTo, setDateTo] = useState(today);
  // The dates the figures on screen were run for; account links drill into the same range.
  const [ran, setRan] = useState({ dateFrom: yearStart, dateTo: today });

  const load = () =>
    api.get('/reports/profit-loss', { params: { dateFrom, dateTo } }).then((r) => {
      setData(r.data);
      setRan({ dateFrom, dateTo });
    });
  useEffect(() => { load(); }, []);

  return (
    <div>
      <PageHeader
        title="Profit & Loss"
        subtitle={data ? `${ran.dateFrom} to ${ran.dateTo} · click an account to see its entries` : undefined}
        action={
          <div className="flex gap-2 items-center">
            <input type="date" value={dateFrom} onChange={(e) => setDateFrom(e.target.value)} className="!w-40" />
            <span className="text-gray-500 text-sm">to</span>
            <input type="date" value={dateTo} onChange={(e) => setDateTo(e.target.value)} className="!w-40" />
            <button onClick={load} className="btn btn-primary">Run</button>
          </div>
        }
      />
      {data && (
        <div className="max-w-3xl space-y-3">
          <ReportTable title="Revenue" rows={sectionRows('Revenue', data.revenue, data.totalRevenue, ran)} />
          <ReportTable title="Cost of Goods Sold" rows={sectionRows('Cost of Goods Sold', data.cogs, data.totalCogs, ran)} />
          <ReportTable
            title="Operating Expenses"
            rows={sectionRows('Operating Expenses', data.operatingExpenses, data.totalOperatingExpenses, ran)}
          />
          <ReportTable
            title="Summary"
            rows={[
              { key: 'gross', label: 'Gross Profit', amount: Number(data.grossProfit), total: true },
              { key: 'net', label: 'Net Income', amount: Number(data.netIncome), total: true },
            ]}
          />
        </div>
      )}
    </div>
  );
}
