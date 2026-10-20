import { useEffect, useState } from 'react';
import api from '../../api/client';
import PageHeader from '../../components/PageHeader';
import { AccountLink } from '../../components/ReportTable';

interface Row { account: { id: string; code: string; name: string; type: string }; debit: number; credit: number; balance: number }

export default function TrialBalance() {
  const [rows, setRows] = useState<Row[]>([]);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    api.get('/ledger/trial-balance').then((r) => setRows(r.data)).finally(() => setLoading(false));
  }, []);

  const fmt = (v: number) => new Intl.NumberFormat('en-US', { minimumFractionDigits: 2 }).format(v);
  const totalDebit  = rows.reduce((s, r) => s + Number(r.debit), 0);
  const totalCredit = rows.reduce((s, r) => s + Number(r.credit), 0);

  return (
    <div>
      <PageHeader title="Trial Balance" subtitle="All dates · click an account to see its entries" />
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
            {rows.map((r) => (
              <tr key={r.account.id}>
                <td><AccountLink account={r.account} /></td>
                <td className="text-xs text-gray-600">{r.account.type}</td>
                <td className="num">{Number(r.debit) > 0 ? fmt(r.debit) : ''}</td>
                <td className="num">{Number(r.credit) > 0 ? fmt(r.credit) : ''}</td>
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
