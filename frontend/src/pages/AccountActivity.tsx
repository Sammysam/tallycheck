import { useEffect, useState } from 'react';
import { useNavigate, useParams, useSearchParams } from 'react-router-dom';
import api, { errMsg } from '../api/client';
import PageHeader from '../components/PageHeader';
import { money } from '../lib/money';

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
}
interface Activity {
  account: { code: string; name: string; type: string };
  openingBalance: number;
  lines: Line[];
  totalDebit: number;
  totalCredit: number;
  closingBalance: number;
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
            </tr>
          </thead>
          <tbody>
            <tr className="total">
              <td colSpan={6}>Balance brought forward</td>
              <td className="num">{money(data.openingBalance)}</td>
            </tr>
            {data.lines.map((l, i) => (
              <tr key={`${l.entryId}-${i}`}>
                <td className="whitespace-nowrap">{l.date}</td>
                <td className="whitespace-nowrap">{l.entryNumber}</td>
                <td>{l.description}{l.lineDescription ? <span className="text-gray-500"> · {l.lineDescription}</span> : null}</td>
                <td>{l.reference ?? ''}</td>
                <td className="num">{amount(l.debit)}</td>
                <td className="num">{amount(l.credit)}</td>
                <td className={`num ${l.balance < 0 ? 'neg' : ''}`}>{money(l.balance)}</td>
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
            </tr>
          </tbody>
        </table>
      )}
    </div>
  );
}
