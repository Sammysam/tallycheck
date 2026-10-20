import { useEffect, useState } from 'react';
import api from '../../api/client';
import PageHeader from '../../components/PageHeader';
import { ReportTable } from '../../components/ReportTable';
import { sectionRows, type AccountLine } from '../../lib/reportRows';
import { previousPeriod, samePeriodLastYear } from '../../lib/periods';

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

type CompareWith = '' | 'previous' | 'lastYear';
interface Range { dateFrom: string; dateTo: string }

export default function ProfitLoss() {
  const [data, setData] = useState<ProfitLossData | null>(null);
  const [before, setBefore] = useState<ProfitLossData | null>(null);
  const today = new Date().toISOString().split('T')[0];
  const yearStart = `${new Date().getFullYear()}-01-01`;
  const [dateFrom, setDateFrom] = useState(yearStart);
  const [dateTo, setDateTo] = useState(today);
  const [compareWith, setCompareWith] = useState<CompareWith>('');
  // The dates the figures on screen were run for; account links drill into the same range.
  const [ran, setRan] = useState<Range>({ dateFrom: yearStart, dateTo: today });
  const [ranBefore, setRanBefore] = useState<Range | null>(null);

  const load = () => {
    const range = { dateFrom, dateTo };
    const other = compareWith === 'previous' ? previousPeriod(dateFrom, dateTo)
      : compareWith === 'lastYear' ? samePeriodLastYear(dateFrom, dateTo) : null;
    return Promise.all([
      api.get('/reports/profit-loss', { params: range }),
      other ? api.get('/reports/profit-loss', { params: other }) : Promise.resolve(null),
    ]).then(([now, then]) => {
      setData(now.data);
      setBefore(then?.data ?? null);
      setRan(range);
      setRanBefore(other);
    });
  };
  useEffect(() => { load(); }, []);

  const head = (r: Range) => `${r.dateFrom} – ${r.dateTo}`;
  const compareHeader = before && ranBefore ? head(ranBefore) : undefined;
  const cmp = (lines: AccountLine[] | undefined, total: number | undefined) =>
    before ? { lines: lines ?? [], total: total ?? 0 } : undefined;

  return (
    <div>
      <PageHeader
        title="Profit & Loss"
        subtitle={data ? `${head(ran)} · click an account to see its entries` : undefined}
        action={
          <div className="flex gap-2 items-center">
            <input type="date" value={dateFrom} onChange={(e) => setDateFrom(e.target.value)} className="!w-40" aria-label="From" />
            <span className="text-gray-500 text-sm">to</span>
            <input type="date" value={dateTo} onChange={(e) => setDateTo(e.target.value)} className="!w-40" aria-label="To" />
            <select value={compareWith} onChange={(e) => setCompareWith(e.target.value as CompareWith)} className="!w-64" aria-label="Compare with">
              <option value="">No comparison</option>
              <option value="previous">Compare: previous period</option>
              <option value="lastYear">Compare: same period last year</option>
            </select>
            <button onClick={load} className="btn btn-primary">Run</button>
          </div>
        }
      />
      {data && (
        <div className={`${compareHeader ? 'max-w-5xl' : 'max-w-3xl'} space-y-3`}>
          <ReportTable title="Revenue" amountHeader={compareHeader ? head(ran) : 'Amount'} compareHeader={compareHeader}
            rows={sectionRows('Revenue', data.revenue, data.totalRevenue, ran, cmp(before?.revenue, before?.totalRevenue))} />
          <ReportTable title="Cost of Goods Sold" amountHeader={compareHeader ? head(ran) : 'Amount'} compareHeader={compareHeader}
            rows={sectionRows('Cost of Goods Sold', data.cogs, data.totalCogs, ran, cmp(before?.cogs, before?.totalCogs))} />
          <ReportTable title="Operating Expenses" amountHeader={compareHeader ? head(ran) : 'Amount'} compareHeader={compareHeader}
            rows={sectionRows('Operating Expenses', data.operatingExpenses, data.totalOperatingExpenses, ran,
              cmp(before?.operatingExpenses, before?.totalOperatingExpenses))} />
          <ReportTable
            title="Summary"
            amountHeader={compareHeader ? head(ran) : 'Amount'}
            compareHeader={compareHeader}
            rows={[
              { key: 'gross', label: 'Gross Profit', amount: Number(data.grossProfit), compare: before ? Number(before.grossProfit) : undefined, total: true },
              { key: 'net', label: 'Net Income', amount: Number(data.netIncome), compare: before ? Number(before.netIncome) : undefined, total: true },
            ]}
          />
        </div>
      )}
    </div>
  );
}
