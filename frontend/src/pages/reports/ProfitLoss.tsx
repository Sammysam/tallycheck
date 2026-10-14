import { useEffect, useState } from 'react';
import api from '../../api/client';
import PageHeader from '../../components/PageHeader';

export default function ProfitLoss() {
  const [data, setData] = useState<any>(null);
  const today = new Date().toISOString().split('T')[0];
  const yearStart = `${new Date().getFullYear()}-01-01`;
  const [dateFrom, setDateFrom] = useState(yearStart);
  const [dateTo, setDateTo] = useState(today);

  const load = () => api.get('/reports/profit-loss', { params: { dateFrom, dateTo } }).then((r) => setData(r.data));
  useEffect(() => { load(); }, []);

  const fmt = (v: number) => new Intl.NumberFormat('en-LK', { style: 'currency', currency: 'LKR' }).format(v ?? 0);

  const Section = ({ title, rows, total }: { title: string; rows: any[]; total: number }) => (
    <div className="mb-6">
      <h3 className="font-semibold text-gray-700 mb-2 border-b pb-1">{title}</h3>
      {rows.map((r: any, i: number) => (
        <div key={i} className="flex justify-between py-1 text-sm text-gray-600 px-2">
          <span>{r.account.code} {r.account.name}</span>
          <span>{fmt(r.balance)}</span>
        </div>
      ))}
      <div className="flex justify-between py-1 text-sm font-semibold border-t mt-1 px-2">
        <span>Total {title}</span><span>{fmt(total)}</span>
      </div>
    </div>
  );

  return (
    <div>
      <PageHeader title="Profit & Loss" action={
        <div className="flex gap-2 items-center">
          <input type="date" value={dateFrom} onChange={(e) => setDateFrom(e.target.value)} className="border rounded px-2 py-1 text-sm" />
          <span className="text-gray-400 text-sm">to</span>
          <input type="date" value={dateTo} onChange={(e) => setDateTo(e.target.value)} className="border rounded px-2 py-1 text-sm" />
          <button onClick={load} className="px-3 py-1 text-sm bg-blue-600 text-white rounded hover:bg-blue-700">Run</button>
        </div>
      } />
      {data && (
        <div className="bg-white rounded-lg border border-gray-200 p-6 max-w-2xl">
          <Section title="Revenue"            rows={data.revenue}            total={data.totalRevenue} />
          <Section title="Cost of Goods Sold" rows={data.cogs}               total={data.totalCogs} />
          <div className="flex justify-between py-2 font-semibold text-gray-800 border-y mb-4 px-2">
            <span>Gross Profit</span><span>{fmt(data.grossProfit)}</span>
          </div>
          <Section title="Operating Expenses" rows={data.operatingExpenses}  total={data.totalOperatingExpenses} />
          <div className={`flex justify-between py-2 font-bold text-lg border-t px-2 ${data.netIncome >= 0 ? 'text-green-700' : 'text-red-600'}`}>
            <span>Net Income</span><span>{fmt(data.netIncome)}</span>
          </div>
        </div>
      )}
    </div>
  );
}
