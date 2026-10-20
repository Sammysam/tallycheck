import { useEffect, useState } from 'react';
import api from '../../api/client';
import PageHeader from '../../components/PageHeader';
import { ReportTable } from '../../components/ReportTable';
import { sectionRows, type AccountLine } from '../../lib/reportRows';

interface BalanceSheetData {
  assets: AccountLine[];
  liabilities: AccountLine[];
  equity: AccountLine[];
  totalAssets: number;
  totalLiabilities: number;
  totalEquity: number;
  retainedEarnings: number;
  totalLiabilitiesAndEquity: number;
  balanced: boolean;
}

export default function BalanceSheet() {
  const [data, setData] = useState<BalanceSheetData | null>(null);
  const [asOf, setAsOf] = useState(new Date().toISOString().split('T')[0]);
  // A balance sheet is cumulative: an account link shows everything up to the date it was run for.
  const [ranAsOf, setRanAsOf] = useState(asOf);

  const load = () =>
    api.get('/reports/balance-sheet', { params: { asOf } }).then((r) => {
      setData(r.data);
      setRanAsOf(asOf);
    });
  useEffect(() => { load(); }, []);

  const dates = { dateTo: ranAsOf };

  return (
    <div>
      <PageHeader
        title="Balance Sheet"
        subtitle={data ? `As of ${ranAsOf} · click an account to see its entries` : undefined}
        action={
          <div className="flex gap-2 items-center">
            <span className="text-sm text-gray-500">As of</span>
            <input type="date" value={asOf} onChange={(e) => setAsOf(e.target.value)} className="!w-40" />
            <button onClick={load} className="btn btn-primary">Run</button>
          </div>
        }
      />
      {data && (
        <div className="grid grid-cols-1 xl:grid-cols-2 gap-3 max-w-5xl items-start">
          <div className="space-y-3">
            <ReportTable
              title="Assets"
              rows={[
                ...sectionRows('Assets', data.assets, data.totalAssets, dates),
                { key: 'grand-assets', label: 'TOTAL ASSETS', amount: Number(data.totalAssets), total: true },
              ]}
            />
          </div>
          <div className="space-y-3">
            <ReportTable title="Liabilities" rows={sectionRows('Liabilities', data.liabilities, data.totalLiabilities, dates)} />
            <ReportTable
              title="Equity"
              rows={[
                ...sectionRows('Equity', data.equity, data.totalEquity, dates),
                { key: 'retained', label: 'Retained Earnings', amount: Number(data.retainedEarnings) },
                { key: 'grand-le', label: 'TOTAL L + E', amount: Number(data.totalLiabilitiesAndEquity), total: true },
              ]}
            />
            <p className={`text-xs px-1 ${data.balanced ? 'text-green-700' : 'text-red-600'}`}>
              {data.balanced ? '✓ Balanced' : '✗ Out of balance'}
            </p>
          </div>
        </div>
      )}
    </div>
  );
}
