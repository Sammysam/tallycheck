import { useEffect, useState } from 'react';
import { Link, useNavigate, useParams, useSearchParams } from 'react-router-dom';
import api, { errMsg } from '../api/client';
import PageHeader from '../components/PageHeader';
import { money, moneyIn } from '../lib/money';
import { fmtDate } from '../lib/region';

/** D23: every ledger line of one account with a running balance — the drill-down from reports. */

interface Line {
  entryId: string;
  entryNumber: string;
  date: string;
  description: string;
  reference: string | null;
  lineDescription: string | null;
  debit: number;
  credit: number;
  balance: number;
  /** D34: on a foreign-currency account. */
  foreignAmount?: number | null;
  foreignBalance?: number | null;
}
interface Activity {
  account: { code: string; name: string; type: string };
  openingBalance: number;
  lines: Line[];
  totalDebit: number;
  totalCredit: number;
  closingBalance: number;
  currency?: string | null;
  foreignOpening?: number | null;
  foreignClosing?: number | null;
}

const amount = (v: number) => (Number(v) === 0 ? '' : money(v));

export default function AccountActivity() {
  const { id } = useParams();
  const navigate = useNavigate();
  const [params, setParams] = useSearchParams();
  const [dateFrom, setDateFrom] = useState(params.get('dateFrom') ?? '');
  const [dateTo, setDateTo] = useState(params.get('dateTo') ?? '');
  const [data, setData] = useState<Activity | null>(null);
  const [error, setError] = useState('');

  const queryFrom = params.get('dateFrom') ?? '';
  const queryTo = params.get('dateTo') ?? '';
  useEffect(() => {
    api.get(`/ledger/activity/${id}`, { params: { dateFrom: queryFrom || undefined, dateTo: queryTo || undefined } })
      .then((r) => { setData(r.data); setError(''); })
      .catch((e) => setError(errMsg(e, 'Could not load the account')));
  }, [id, queryFrom, queryTo]);

  const run = () => {
    const q = new URLSearchParams();
    if (dateFrom) q.set('dateFrom', dateFrom);
    if (dateTo) q.set('dateTo', dateTo);
    setParams(q);
  };

  const period = queryFrom || queryTo ? `${queryFrom || 'the start'} to ${queryTo || 'today'}` : 'All dates';

  return (
    <div>
      <PageHeader
        title={data ? `${data.account.code} ${data.account.name}` : 'Account'}
        subtitle={data ? `${data.account.type} · ${period}` : undefined}
        action={
          <div className="flex gap-2 items-center no-print">
            <input type="date" value={dateFrom} onChange={(e) => setDateFrom(e.target.value)} className="!w-40" aria-label="From" />
            <span className="text-gray-500 text-sm">to</span>
            <input type="date" value={dateTo} onChange={(e) => setDateTo(e.target.value)} className="!w-40" aria-label="To" />
            <button onClick={run} className="btn btn-primary">Run</button>
            <button onClick={() => navigate(-1)} className="btn btn-secondary">Back</button>
          </div>
        }
      />
      {error && <div className="mb-3 px-3 py-2 bg-red-50 border border-red-200 text-red-700 text-sm rounded">{error}</div>}
      {data?.currency && (
        <p className="text-sm text-gray-600 mb-2">
          Kept in <strong>{data.currency}</strong>: balance {moneyIn(Number(data.foreignClosing ?? 0), data.currency)} ·
          shown in the books as {money(data.closingBalance)}. Use Revalue on the Chart of Accounts at month end.
        </p>
      )}
      {data && (
        <table className="report-table">
          <thead>
            <tr>
              <th>Date</th>
              <th>Entry</th>
              <th>Description</th>
              <th>Reference</th>
              <th className="num">Debit</th>
              <th className="num">Credit</th>
              <th className="num">Balance</th>
              {data.currency && <th className="num">{data.currency}</th>}
              {data.currency && <th className="num">{data.currency} balance</th>}
            </tr>
          </thead>
          <tbody>
            <tr className="total">
              <td colSpan={6}>Balance brought forward</td>
              <td className="num">{money(data.openingBalance)}</td>
              {data.currency && <td />}
              {data.currency && <td className="num">{moneyIn(Number(data.foreignOpening ?? 0), data.currency)}</td>}
            </tr>
            {data.lines.map((l, i) => (
              <tr key={`${l.entryId}-${i}`}>
                <td className="whitespace-nowrap"><span data-value={l.date ?? ''}>{fmtDate(l.date)}</span></td>
                <td className="whitespace-nowrap"><Link className="report-link" to={`/ledger/entries/${l.entryId}`}>{l.entryNumber}</Link></td>
                <td>{l.description}{l.lineDescription ? <span className="text-gray-500"> · {l.lineDescription}</span> : null}</td>
                <td>{l.reference ?? ''}</td>
                <td className="num">{amount(l.debit)}</td>
                <td className="num">{amount(l.credit)}</td>
                <td className={`num ${l.balance < 0 ? 'neg' : ''}`}>{money(l.balance)}</td>
                {data.currency && <td className="num">{Number(l.foreignAmount) ? moneyIn(Number(l.foreignAmount), data.currency) : ''}</td>}
                {data.currency && <td className="num">{moneyIn(Number(l.foreignBalance ?? 0), data.currency)}</td>}
              </tr>
            ))}
            {data.lines.length === 0 && (
              <tr><td colSpan={7} className="empty">No entries in this period.</td></tr>
            )}
            <tr className="total">
              <td colSpan={4}>Closing balance</td>
              <td className="num">{money(data.totalDebit)}</td>
              <td className="num">{money(data.totalCredit)}</td>
              <td className="num" data-testid="closing-balance">{money(data.closingBalance)}</td>
              {data.currency && <td />}
              {data.currency && <td className="num" data-testid="foreign-closing">{moneyIn(Number(data.foreignClosing ?? 0), data.currency)}</td>}
            </tr>
          </tbody>
        </table>
      )}
    </div>
  );
}
