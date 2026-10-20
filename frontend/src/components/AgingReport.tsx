import { useEffect, useState } from 'react';
import api from '../api/client';
import PageHeader from './PageHeader';
import { money } from '../lib/money';
import { fmtDate } from '../lib/region';

const BUCKETS = [
  { key: 'current', label: 'Current' },
  { key: 'days1_30', label: '1–30 days' },
  { key: 'days31_60', label: '31–60 days' },
  { key: 'days61_90', label: '61–90 days' },
  { key: 'days90plus', label: '90+ days' },
];

interface Item { id: string; number: string; name: string; date: string; dueDate: string | null; amount: number }
type Aging = Record<string, { items: Item[]; total: number } | number> & { grandTotal: number };

/** AR or AP ageing: bucket totals, then every open document as a grid row. */
export default function AgingReport({ kind }: { kind: 'ar' | 'ap' }) {
  const [data, setData] = useState<Aging | null>(null);
  useEffect(() => { api.get(`/reports/${kind}-aging`).then((r) => setData(r.data)); }, [kind]);

  const bucket = (key: string) => {
    const b = data?.[key];
    return typeof b === 'object' && b ? b : { items: [] as Item[], total: 0 };
  };
  const party = kind === 'ar' ? 'Customer' : 'Vendor';
  const doc = kind === 'ar' ? 'Invoice' : 'Bill';

  return (
    <div>
      <PageHeader title={kind === 'ar' ? 'AR Aging' : 'AP Aging'} subtitle="Aged from the due date (or the document date) to today." />
      {data && (
        <div className="space-y-3">
          <table className="report-table max-w-3xl">
            <thead>
              <tr>
                {BUCKETS.map((b) => <th key={b.key} className="num">{b.label}</th>)}
                <th className="num">Grand Total Outstanding</th>
              </tr>
            </thead>
            <tbody>
              <tr className="total">
                {BUCKETS.map((b) => <td key={b.key} className="num">{money(bucket(b.key).total)}</td>)}
                <td className="num">{money(data.grandTotal)}</td>
              </tr>
            </tbody>
          </table>

          <table className="report-table">
            <thead>
              <tr>
                <th>Age</th>
                <th>{doc}</th>
                <th>{party}</th>
                <th>Date</th>
                <th>Due</th>
                <th className="num">Outstanding</th>
              </tr>
            </thead>
            <tbody>
              {BUCKETS.flatMap((b) =>
                bucket(b.key).items.map((item) => (
                  <tr key={item.id}>
                    <td>{b.label}</td>
                    <td>{item.number}</td>
                    <td>{item.name}</td>
                    <td><span data-value={item.date ?? ''}>{fmtDate(item.date)}</span></td>
                    <td>{item.dueDate ? <span data-value={item.dueDate ?? ''}>{fmtDate(item.dueDate)}</span> : '—'}</td>
                    <td className="num">{money(item.amount)}</td>
                  </tr>
                )),
              )}
              {Number(data.grandTotal) === 0 && (
                <tr><td colSpan={6} className="empty">Nothing outstanding.</td></tr>
              )}
              <tr className="total">
                <td colSpan={5}>Grand Total Outstanding</td>
                <td className="num">{money(data.grandTotal)}</td>
              </tr>
            </tbody>
          </table>
        </div>
      )}
    </div>
  );
}
