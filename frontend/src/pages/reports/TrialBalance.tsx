import { useEffect, useState } from 'react';
import api from '../../api/client';
import PageHeader from '../../components/PageHeader';

interface Row { account: { code: string; name: string; type: string }; debit: number; credit: number; balance: number }

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
      <PageHeader title="Trial Balance" />
      {loading ? <p className="text-gray-400">Loading...</p> : (
        <div className="bg-white rounded-lg border border-gray-200 overflow-hidden">
          <table className="min-w-full text-sm">
            <thead className="bg-gray-50 border-b border-gray-200">
              <tr>
                <th className="px-4 py-3 text-left font-semibold text-gray-600">Code</th>
                <th className="px-4 py-3 text-left font-semibold text-gray-600">Account</th>
                <th className="px-4 py-3 text-left font-semibold text-gray-600">Type</th>
                <th className="px-4 py-3 text-right font-semibold text-gray-600">Debit</th>
                <th className="px-4 py-3 text-right font-semibold text-gray-600">Credit</th>
              </tr>
            </thead>
            <tbody className="divide-y divide-gray-100">
              {rows.map((r, i) => (
                <tr key={i} className="hover:bg-gray-50">
                  <td className="px-4 py-2 font-mono text-xs text-gray-500">{r.account.code}</td>
                  <td className="px-4 py-2 text-gray-700">{r.account.name}</td>
                  <td className="px-4 py-2 text-gray-500 text-xs">{r.account.type}</td>
                  <td className="px-4 py-2 text-right">{Number(r.debit) > 0 ? fmt(r.debit) : ''}</td>
                  <td className="px-4 py-2 text-right">{Number(r.credit) > 0 ? fmt(r.credit) : ''}</td>
                </tr>
              ))}
            </tbody>
            <tfoot className="border-t-2 border-gray-300 bg-gray-50 font-semibold">
              <tr>
                <td colSpan={3} className="px-4 py-3">Total</td>
                <td className="px-4 py-3 text-right">{fmt(totalDebit)}</td>
                <td className="px-4 py-3 text-right">{fmt(totalCredit)}</td>
              </tr>
            </tfoot>
          </table>
        </div>
      )}
    </div>
  );
}
