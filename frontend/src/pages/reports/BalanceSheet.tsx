import { useEffect, useState } from 'react';
import api from '../../api/client';
import PageHeader from '../../components/PageHeader';
import { ReportTable } from '../../components/ReportTable';
import { sectionRows, type AccountLine } from '../../lib/reportRows';
import { lastYearEnd, shiftYears } from '../../lib/periods';
import { todayIso } from '../../lib/region';

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

type CompareWith = '' | 'lastYear' | 'yearEnd';

export default function BalanceSheet() {
  const [data, setData] = useState<BalanceSheetData | null>(null);
  const [before, setBefore] = useState<BalanceSheetData | null>(null);
  const [asOf, setAsOf] = useState(todayIso());
  const [compareWith, setCompareWith] = useState<CompareWith>('');
  // A balance sheet is cumulative: an account link shows everything up to the date it was run for.
  const [ranAsOf, setRanAsOf] = useState(asOf);
  const [ranBefore, setRanBefore] = useState<string | null>(null);

  const load = () => {
    const other = compareWith === 'lastYear' ? shiftYears(asOf, -1) : compareWith === 'yearEnd' ? lastYearEnd(asOf) : null;
    return Promise.all([
      api.get('/reports/balance-sheet', { params: { asOf } }),
      other ? api.get('/reports/balance-sheet', { params: { asOf: other } }) : Promise.resolve(null),
    ]).then(([now, then]) => {
      setData(now.data);
      setBefore(then?.data ?? null);
      setRanAsOf(asOf);
      setRanBefore(other);
    });
  };
  useEffect(() => { load(); }, []);

  const dates = { dateTo: ranAsOf };
  const compareHeader = before && ranBefore ? `As of ${ranBefore}` : undefined;
  const amountHeader = compareHeader ? `As of ${ranAsOf}` : 'Amount';
  const cmp = (lines: AccountLine[] | undefined, total: number | undefined) =>
    before ? { lines: lines ?? [], total: total ?? 0 } : undefined;
  const was = (v: number | undefined) => (before ? Number(v ?? 0) : undefined);

  return (
    <div>
      <PageHeader
        title="Balance Sheet"
        subtitle={data ? `As of ${ranAsOf} · click an account to see its entries` : undefined}
        action={
          <div className="flex gap-2 items-center">
            <span className="text-sm text-gray-500">As of</span>
            <input type="date" value={asOf} onChange={(e) => setAsOf(e.target.value)} className="!w-40" aria-label="As of" />
            <select value={compareWith} onChange={(e) => setCompareWith(e.target.value as CompareWith)} className="!w-64" aria-label="Compare with">
              <option value="">No comparison</option>
              <option value="lastYear">Compare: same date last year</option>
              <option value="yearEnd">Compare: last year end</option>
            </select>
            <button onClick={load} className="btn btn-primary">Run</button>
          </div>
        }
      />
      {data && (
        <div className={`grid grid-cols-1 ${compareHeader ? '' : 'xl:grid-cols-2'} gap-3 max-w-5xl items-start`}>
          <div className="space-y-3">
            <ReportTable
              title="Assets"
              amountHeader={amountHeader}
              compareHeader={compareHeader}
              rows={[
                ...sectionRows('Assets', data.assets, data.totalAssets, dates, cmp(before?.assets, before?.totalAssets)),
                { key: 'grand-assets', label: 'TOTAL ASSETS', amount: Number(data.totalAssets), compare: was(before?.totalAssets), total: true },
              ]}
            />
          </div>
          <div className="space-y-3">
            <ReportTable
              title="Liabilities"
              amountHeader={amountHeader}
              compareHeader={compareHeader}
              rows={sectionRows('Liabilities', data.liabilities, data.totalLiabilities, dates, cmp(before?.liabilities, before?.totalLiabilities))}
            />
            <ReportTable
              title="Equity"
              amountHeader={amountHeader}
              compareHeader={compareHeader}
              rows={[
                ...sectionRows('Equity', data.equity, data.totalEquity, dates, cmp(before?.equity, before?.totalEquity)),
                { key: 'retained', label: 'Retained Earnings', amount: Number(data.retainedEarnings), compare: was(before?.retainedEarnings) },
                { key: 'grand-le', label: 'TOTAL L + E', amount: Number(data.totalLiabilitiesAndEquity), compare: was(before?.totalLiabilitiesAndEquity), total: true },
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
