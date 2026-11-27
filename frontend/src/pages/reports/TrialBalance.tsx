import { useEffect, useState } from 'react';
import api from '../../api/client';
import PageHeader from '../../components/PageHeader';
import { AccountLink } from '../../components/ReportTable';
import { fmtDate, todayIso } from '../../lib/region';

interface Row { account: { id: string; code: string; name: string; type: string }; debit: number; credit: number; balance: number }

const TYPE_LABEL: Record<string, string> = { ASSET: 'Asset', LIABILITY: 'Liability', EQUITY: 'Equity', REVENUE: 'Revenue', EXPENSE: 'Expense' };

/**
 * m1: a standard trial balance — each account's net balance in its debit or credit column, as
 * of a date (the ledger's own totals per account come from /ledger/trial-balance).
 */
export default function TrialBalance() {
  const [rows, setRows] = useState<Row[]>([]);
  const [loading, setLoading] = useState(true);
  const [asOf, setAsOf] = useState(todayIso());
  const [ranAsOf, setRanAsOf] = useState(asOf);
  const [showZero, setShowZero] = useState(false);

  const fetchRows = (date: string) =>
    api.get('/ledger/trial-balance', { params: { asOf: date } })
      .then((r) => { setRows(r.data); setRanAsOf(date); })
      .finally(() => setLoading(false));
  const load = (date: string) => {
    setLoading(true);
    fetchRows(date);
  };
  useEffect(() => { fetchRows(todayIso()); }, []);

  const fmt = (v: number) => new Intl.NumberFormat('en-US', { minimumFractionDigits: 2, maximumFractionDigits: 2 }).format(v);
  const cents = (v: number) => Math.round(Number(v) * 100);
  const net = rows.map((r) => ({ ...r, net: cents(r.debit) - cents(r.credit) }));
  const shown = showZero ? net : net.filter((r) => r.net !== 0);
  const totalDebit = net.reduce((s, r) => s + (r.net > 0 ? r.net : 0), 0) / 100;
  const totalCredit = net.reduce((s, r) => s + (r.net < 0 ? -r.net : 0), 0) / 100;

  return (
    <div>
      <PageHeader
        title="Trial Balance"
        subtitle={`As of ${fmtDate(ranAsOf)} · net balance of each account · click an account to see its entries`}
        action={
          <div className="flex gap-2 items-center">
            <label className="flex items-center gap-1.5 text-xs text-gray-600 mr-2">
              <input type="checkbox" checked={showZero} onChange={(e) => setShowZero(e.target.checked)} />
              Show accounts with no balance
            </label>
            <span className="text-sm text-gray-500">As of</span>
            <input type="date" value={asOf} onChange={(e) => setAsOf(e.target.value)} className="!w-40" aria-label="As of" />
            <button onClick={() => load(asOf)} className="btn btn-primary">Run</button>
          </div>
        }
      />
      {loading ? <p className="text-gray-500">Loading...</p> : (
        <table className="report-table">
          <thead>
            <tr>
              <th>Account</th>
              <th>Type</th>
              <th className="num">Debit</th>
              <th className="num">Credit</th>
            </tr>
          </thead>
          <tbody>
            {shown.map((r) => (
              <tr key={r.account.id}>
                <td><AccountLink account={r.account} dateTo={ranAsOf} /></td>
                <td className="text-xs text-gray-600">{TYPE_LABEL[r.account.type] ?? r.account.type}</td>
                <td className="num">{r.net > 0 ? fmt(r.net / 100) : ''}</td>
                <td className="num">{r.net < 0 ? fmt(-r.net / 100) : ''}</td>
              </tr>
            ))}
            <tr className="total">
              <td colSpan={2}>Total</td>
              <td className="num">{fmt(totalDebit)}</td>
              <td className="num">{fmt(totalCredit)}</td>
            </tr>
          </tbody>
        </table>
      )}
    </div>
  );
}
