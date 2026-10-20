import { useEffect, useState } from 'react';
import api, { errMsg } from '../../api/client';
import PageHeader from '../../components/PageHeader';
import { money } from '../../lib/money';
import { todayIso } from '../../lib/region';

/**
 * D26: what to report to the tax office for a period — per tax code, sales and the tax charged
 * on them, purchases and the tax paid on them, and the difference to pay (or reclaim).
 */

interface Row {
  taxCodeId: string | null;
  name: string;
  rate: number | null;
  salesNet: number;
  outputTax: number;
  purchasesNet: number;
  inputTax: number;
}
interface TaxReturnData {
  rows: Row[];
  totals: Omit<Row, 'taxCodeId' | 'name' | 'rate'>;
  netTax: number;
}

const monthStart = () => `${todayIso().slice(0, 7)}-01`;

export default function TaxReturn() {
  const [dateFrom, setDateFrom] = useState(monthStart());
  const [dateTo, setDateTo] = useState(todayIso());
  const [ran, setRan] = useState({ dateFrom: monthStart(), dateTo: todayIso() });
  const [data, setData] = useState<TaxReturnData | null>(null);
  const [error, setError] = useState('');

  useEffect(() => {
    api.get('/reports/tax', { params: ran })
      .then((r) => { setData(r.data); setError(''); })
      .catch((e) => setError(errMsg(e, 'Could not run the tax return')));
  }, [ran]);

  return (
    <div>
      <PageHeader
        title="Tax Return"
        subtitle={`${ran.dateFrom} – ${ran.dateTo} · posted documents by document date`}
        action={
          <div className="flex gap-2 items-center">
            <input type="date" value={dateFrom} onChange={(e) => setDateFrom(e.target.value)} className="!w-40" aria-label="From" />
            <span className="text-gray-500 text-sm">to</span>
            <input type="date" value={dateTo} onChange={(e) => setDateTo(e.target.value)} className="!w-40" aria-label="To" />
            <button onClick={() => setRan({ dateFrom, dateTo })} className="btn btn-primary">Run</button>
          </div>
        }
      />
      {error && <div className="mb-3 px-3 py-2 bg-red-50 border border-red-200 text-red-700 text-sm rounded">{error}</div>}
      {data && (
        <div className="space-y-3 max-w-5xl">
          <table className="report-table">
            <thead>
              <tr>
                <th>Tax code</th>
                <th className="num">Rate</th>
                <th className="num">Sales (net)</th>
                <th className="num">Tax on sales</th>
                <th className="num">Purchases (net)</th>
                <th className="num">Tax on purchases</th>
              </tr>
            </thead>
            <tbody>
              {data.rows.map((r) => (
                <tr key={r.taxCodeId ?? 'none'}>
                  <td>{r.name}</td>
                  <td className="num">{r.rate === null ? '' : `${r.rate}%`}</td>
                  <td className="num">{money(r.salesNet)}</td>
                  <td className="num">{money(r.outputTax)}</td>
                  <td className="num">{money(r.purchasesNet)}</td>
                  <td className="num">{money(r.inputTax)}</td>
                </tr>
              ))}
              {data.rows.length === 0 && (
                <tr><td colSpan={6} className="empty">No posted sales or purchases in this period.</td></tr>
              )}
              <tr className="total">
                <td colSpan={2}>Total</td>
                <td className="num">{money(data.totals.salesNet)}</td>
                <td className="num">{money(data.totals.outputTax)}</td>
                <td className="num">{money(data.totals.purchasesNet)}</td>
                <td className="num">{money(data.totals.inputTax)}</td>
              </tr>
            </tbody>
          </table>

          <table className="report-table max-w-xl">
            <thead><tr><th>Summary</th><th className="num">Amount</th></tr></thead>
            <tbody>
              <tr><td>Tax charged on sales</td><td className="num">{money(data.totals.outputTax)}</td></tr>
              <tr><td>Less tax paid on purchases</td><td className="num">{money(data.totals.inputTax)}</td></tr>
              <tr className="total">
                <td>{data.netTax >= 0 ? 'Tax to pay' : 'Tax to reclaim'}</td>
                <td className="num" data-testid="net-tax">{money(Math.abs(data.netTax))}</td>
              </tr>
            </tbody>
          </table>
          <p className="text-xs text-gray-500 px-1">
            Credit and debit notes reduce the period they are dated in. Draft and voided documents never count.
            Check the figures against your country's return form before filing.
          </p>
        </div>
      )}
    </div>
  );
}
