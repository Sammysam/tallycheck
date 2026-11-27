import { useEffect, useState } from 'react';
import api from '../../api/client';
import PageHeader from '../../components/PageHeader';
import { ReportTable } from '../../components/ReportTable';
import { sectionRows, type AccountLine } from '../../lib/reportRows';
import { lastYearEnd, shiftYears } from '../../lib/periods';
import { fmtDate, todayIso } from '../../lib/region';

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
  // m2: accounts with nothing on them are left out unless asked for.
  const [showZero, setShowZero] = useState(false);

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
  const compareHeader = before && ranBefore ? `As of ${fmtDate(ranBefore)}` : undefined;
  const amountHeader = compareHeader ? `As of ${fmtDate(ranAsOf)}` : 'Amount';
  const cmp = (lines: AccountLine[] | undefined, total: number | undefined) =>
    before ? { lines: lines ?? [], total: total ?? 0 } : undefined;
  const was = (v: number | undefined) => (before ? Number(v ?? 0) : undefined);
  const keep = (lines: AccountLine[] | undefined) => (showZero ? lines ?? [] : (lines ?? []).filter((l) => Number(l.balance) !== 0));
  // m2: the profit not yet closed into retained earnings belongs inside total equity.
  const equityRows = () => {
    if (!data) return [];
    const rows = sectionRows('Equity', keep(data.equity), Number(data.totalEquity) + Number(data.retainedEarnings), dates,
      before ? { lines: keep(before.equity), total: Number(before.totalEquity) + Number(before.retainedEarnings) } : undefined);
    const total = rows.pop()!;
    return [
      ...rows,
      { key: 'retained', label: 'Retained earnings (profit to date)', amount: Number(data.retainedEarnings), compare: was(before?.retainedEarnings) },
      total,
      { key: 'grand-le', label: 'Total liabilities and equity', amount: Number(data.totalLiabilitiesAndEquity), compare: was(before?.totalLiabilitiesAndEquity), total: true },
    ];
  };

  return (
    <div>
      <PageHeader
        title="Balance Sheet"
        subtitle={data ? `As of ${fmtDate(ranAsOf)} · click an account to see its entries` : undefined}
        action={
          <div className="flex gap-2 items-center">
            <label className="flex items-center gap-1.5 text-xs text-gray-600 mr-2">
              <input type="checkbox" checked={showZero} onChange={(e) => setShowZero(e.target.checked)} />
              Show accounts with no balance
            </label>
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
                ...sectionRows('Assets', keep(data.assets), data.totalAssets, dates, cmp(keep(before?.assets), before?.totalAssets)),
              ]}
            />
          </div>
          <div className="space-y-3">
            <ReportTable
              title="Liabilities"
              amountHeader={amountHeader}
              compareHeader={compareHeader}
              rows={sectionRows('Liabilities', keep(data.liabilities), data.totalLiabilities, dates, cmp(keep(before?.liabilities), before?.totalLiabilities))}
            />
            <ReportTable
              title="Equity"
              amountHeader={amountHeader}
              compareHeader={compareHeader}
              rows={equityRows()}
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
