import { useEffect, useState } from 'react';
import api from '../../api/client';
import PageHeader from '../../components/PageHeader';

export default function BalanceSheet() {
  const [data, setData] = useState<any>(null);
  const [asOf, setAsOf] = useState(new Date().toISOString().split('T')[0]);

  const load = () => api.get('/reports/balance-sheet', { params: { asOf } }).then((r) => setData(r.data));
  useEffect(() => { load(); }, []);

  const fmt = (v: number) => new Intl.NumberFormat('en-LK', { style: 'currency', currency: 'LKR' }).format(v ?? 0);

  const Section = ({ title, rows, total }: { title: string; rows: any[]; total: number }) => (
    <div className="mb-4">
      <h3 className="font-semibold text-gray-700 mb-1 border-b pb-1">{title}</h3>
      {rows.map((r: any, i: number) => (
        <div key={i} className="flex justify-between py-0.5 text-sm text-gray-600 px-2">
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
      <PageHeader title="Balance Sheet" action={
        <div className="flex gap-2 items-center">
          <span className="text-sm text-gray-500">As of</span>
          <input type="date" value={asOf} onChange={(e) => setAsOf(e.target.value)} className="border rounded px-2 py-1 text-sm" />
          <button onClick={load} className="px-3 py-1 text-sm bg-blue-600 text-white rounded hover:bg-blue-700">Run</button>
        </div>
      } />
      {data && (
        <div className="grid grid-cols-2 gap-6 max-w-4xl">
          <div className="bg-white rounded-lg border border-gray-200 p-5">
            <h2 className="font-bold text-gray-800 mb-3">Assets</h2>
            <Section title="Assets" rows={data.assets} total={data.totalAssets} />
            <div className="flex justify-between font-bold text-base border-t pt-2 px-2">
              <span>TOTAL ASSETS</span><span>{fmt(data.totalAssets)}</span>
            </div>
          </div>
          <div className="bg-white rounded-lg border border-gray-200 p-5">
            <h2 className="font-bold text-gray-800 mb-3">Liabilities & Equity</h2>
            <Section title="Liabilities" rows={data.liabilities} total={data.totalLiabilities} />
            <Section title="Equity"      rows={data.equity}      total={data.totalEquity} />
            <div className="flex justify-between py-0.5 text-sm text-gray-600 px-2">
              <span>Retained Earnings</span><span>{fmt(data.retainedEarnings)}</span>
            </div>
            <div className="flex justify-between font-bold text-base border-t mt-2 pt-2 px-2">
              <span>TOTAL L + E</span><span>{fmt(data.totalLiabilitiesAndEquity)}</span>
            </div>
            <p className={`text-xs mt-2 px-2 ${data.balanced ? 'text-green-600' : 'text-red-500'}`}>
              {data.balanced ? '✓ Balanced' : '✗ Out of balance'}
            </p>
          </div>
        </div>
      )}
    </div>
  );
}
