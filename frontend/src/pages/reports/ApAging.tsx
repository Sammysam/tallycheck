import { useEffect, useState } from 'react';
import api from '../../api/client';
import PageHeader from '../../components/PageHeader';

export default function ApAging() {
  const [data, setData] = useState<any>(null);
  useEffect(() => { api.get('/reports/ap-aging').then((r) => setData(r.data)); }, []);

  const fmt = (v: number) => new Intl.NumberFormat('en-LK', { style: 'currency', currency: 'LKR' }).format(v ?? 0);

  const buckets = [
    { key: 'current',   label: 'Current' },
    { key: 'days1_30',  label: '1–30 days' },
    { key: 'days31_60', label: '31–60 days' },
    { key: 'days61_90', label: '61–90 days' },
    { key: 'days90plus',label: '90+ days' },
  ];

  return (
    <div>
      <PageHeader title="AP Aging" />
      {data && (
        <>
          <div className="grid grid-cols-5 gap-3 mb-6">
            {buckets.map((b) => (
              <div key={b.key} className="bg-white border border-gray-200 rounded-lg p-4 text-center">
                <p className="text-xs text-gray-500 mb-1">{b.label}</p>
                <p className="text-lg font-bold text-gray-800">{fmt(data[b.key]?.total ?? 0)}</p>
              </div>
            ))}
          </div>
          <div className="bg-white rounded-lg border border-gray-200 p-4">
            <div className="flex justify-between font-bold border-b pb-2 mb-2">
              <span>Grand Total Outstanding</span>
              <span className="text-red-600">{fmt(data.grandTotal)}</span>
            </div>
            {buckets.map((b) =>
              data[b.key]?.items?.map((item: any) => (
                <div key={item.id} className="flex justify-between py-1 text-sm border-b border-gray-50">
                  <span className="text-gray-500 w-24 shrink-0">{b.label}</span>
                  <span className="flex-1 text-gray-700">{item.number} — {item.name}</span>
                  <span className="text-gray-700">{fmt(item.amount)}</span>
                </div>
              ))
            )}
          </div>
        </>
      )}
    </div>
  );
}
