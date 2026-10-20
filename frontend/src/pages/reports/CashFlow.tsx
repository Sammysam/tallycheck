import { useEffect, useState } from 'react';
import api, { errMsg } from '../../api/client';
import PageHeader from '../../components/PageHeader';
import { AccountLink, ReportTable, type ReportRow } from '../../components/ReportTable';
import type { AccountRef } from '../../lib/reportRows';

/**
 * D24: where the cash came from and went, straight from the ledger. Each cash movement is
 * listed against the account on the other side of the entry, grouped into operating,
 * investing and financing.
 */

interface Account extends AccountRef { type: string; isActive: boolean }
interface Section { lines: { account: AccountRef; amount: number }[]; total: number }
interface CashFlowData {
  cashAccounts: AccountRef[];
  openingCash: number;
  operating: Section;
  investing: Section;
  financing: Section;
  netChange: number;
  closingCash: number;
}

export default function CashFlow() {
  const today = new Date().toISOString().split('T')[0];
  const yearStart = `${new Date().getFullYear()}-01-01`;
  const [dateFrom, setDateFrom] = useState(yearStart);
  const [dateTo, setDateTo] = useState(today);
  const [ran, setRan] = useState({ dateFrom: yearStart, dateTo: today });
  const [assets, setAssets] = useState<Account[]>([]);
  const [cash, setCash] = useState<Set<string>>(new Set());
  const [data, setData] = useState<CashFlowData | null>(null);
  const [error, setError] = useState('');

  const run = (ids: Set<string>) =>
    api.get('/reports/cash-flow', { params: { dateFrom, dateTo, cashAccounts: [...ids].join(',') || undefined } })
      .then((r) => {
        setData(r.data);
        setRan({ dateFrom, dateTo });
        // The server chose the default cash accounts when none were picked: show them ticked.
        setCash(new Set((r.data.cashAccounts as AccountRef[]).map((a) => a.id)));
      })
      .then(() => setError(''))
      .catch((e) => setError(errMsg(e, 'Could not run the cash flow statement')));

  useEffect(() => {
    api.get('/accounts').then((r) => setAssets(r.data.filter((a: Account) => a.type === 'ASSET' && a.isActive)));
    run(new Set());
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const toggle = (id: string) => setCash((s) => {
    const n = new Set(s);
    if (n.has(id)) n.delete(id); else n.add(id);
    return n;
  });

  const rows = (title: string, s: Section): ReportRow[] => [
    ...s.lines.map((l) => ({ key: l.account.id, label: <AccountLink account={l.account} {...ran} />, amount: Number(l.amount) })),
    { key: `total-${title}`, label: `Net cash from ${title.toLowerCase()} activities`, amount: Number(s.total), total: true },
  ];

  return (
    <div>
      <PageHeader
        title="Cash Flow"
        subtitle={data ? `${ran.dateFrom} – ${ran.dateTo} · money in is positive, money out negative` : undefined}
        action={
          <div className="flex gap-2 items-center">
            <input type="date" value={dateFrom} onChange={(e) => setDateFrom(e.target.value)} className="!w-40" aria-label="From" />
            <span className="text-gray-500 text-sm">to</span>
            <input type="date" value={dateTo} onChange={(e) => setDateTo(e.target.value)} className="!w-40" aria-label="To" />
            <button onClick={() => run(cash)} className="btn btn-primary">Run</button>
          </div>
        }
      />
      {error && <div className="mb-3 px-3 py-2 bg-red-50 border border-red-200 text-red-700 text-sm rounded">{error}</div>}

      <div className="flex flex-wrap items-center gap-x-4 gap-y-1 mb-3 text-sm">
        <span className="text-xs font-semibold text-gray-600">Cash and bank accounts:</span>
        {assets.map((a) => (
          <label key={a.id} className="flex items-center gap-1.5">
            <input type="checkbox" checked={cash.has(a.id)} onChange={() => toggle(a.id)} />
            {a.code} {a.name}
          </label>
        ))}
      </div>

      {data && (
        <div className="max-w-3xl space-y-3">
          <ReportTable title="Cash at the start" rows={[
            { key: 'opening', label: `Cash and bank on ${ran.dateFrom}`, amount: Number(data.openingCash), total: true },
          ]} />
          <ReportTable title="Operating activities" rows={rows('Operating', data.operating)} />
          <ReportTable title="Investing activities" rows={rows('Investing', data.investing)} />
          <ReportTable title="Financing activities" rows={rows('Financing', data.financing)} />
          <ReportTable title="Summary" rows={[
            { key: 'net', label: 'Net change in cash', amount: Number(data.netChange), total: true },
            { key: 'closing', label: `Cash and bank on ${ran.dateTo}`, amount: Number(data.closingCash), total: true },
          ]} />
          <p className="text-xs text-gray-500 px-1">
            Each line is the account on the other side of the cash entries. Assets and liabilities count as operating
            unless their subtype marks them long-term (fixed, non_current, long_term, investment, loan); equity is financing.
          </p>
        </div>
      )}
    </div>
  );
}
