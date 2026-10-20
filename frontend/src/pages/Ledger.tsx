import { useEffect, useState } from 'react';
import api from '../api/client';
import PageHeader from '../components/PageHeader';
import StatusBadge from '../components/StatusBadge';
import { money } from '../lib/money';

interface Entry { id: string; entryNumber: string; date: string; description: string; status: string; lines: any[] }

export default function Ledger() {
  const [rows, setRows] = useState<Entry[]>([]);
  const [expanded, setExpanded] = useState<string | null>(null);

  useEffect(() => { api.get('/ledger/entries').then((r) => setRows(r.data)); }, []);

  const fmt = (v: number) => money(Number(v));

  return (
    <div>
      <PageHeader title="Journal Entries" />
      <div className="bg-white rounded-lg border border-gray-200 overflow-hidden">
        <table className="min-w-full text-sm">
          <thead className="bg-gray-50 border-b border-gray-200">
            <tr>
              <th className="px-4 py-3 text-left font-semibold text-gray-600">#</th>
              <th className="px-4 py-3 text-left font-semibold text-gray-600">Date</th>
              <th className="px-4 py-3 text-left font-semibold text-gray-600">Description</th>
              <th className="px-4 py-3 text-left font-semibold text-gray-600">Status</th>
            </tr>
          </thead>
          <tbody className="divide-y divide-gray-100">
            {rows.map((entry) => (
              <>
                <tr key={entry.id} className="hover:bg-gray-50 cursor-pointer" onClick={() => setExpanded(expanded === entry.id ? null : entry.id)}>
                  <td className="px-4 py-3 font-mono text-xs text-blue-600">{entry.entryNumber}</td>
                  <td className="px-4 py-3 text-gray-700">{entry.date}</td>
                  <td className="px-4 py-3 text-gray-700">{entry.description}</td>
                  <td className="px-4 py-3"><StatusBadge status={entry.status} /></td>
                </tr>
                {expanded === entry.id && (
                  <tr key={`${entry.id}-lines`}>
                    <td colSpan={4} className="px-8 py-3 bg-gray-50">
                      <table className="min-w-full text-xs">
                        <thead>
                          <tr className="text-gray-500">
                            <th className="text-left pb-1">Account</th>
                            <th className="text-right pb-1">Debit</th>
                            <th className="text-right pb-1">Credit</th>
                          </tr>
                        </thead>
                        <tbody>
                          {entry.lines?.map((line: any) => (
                            <tr key={line.id}>
                              <td className="py-0.5">{line.account?.code} {line.account?.name}</td>
                              <td className="text-right py-0.5">{Number(line.debit) > 0 ? fmt(Number(line.debit)) : ''}</td>
                              <td className="text-right py-0.5">{Number(line.credit) > 0 ? fmt(Number(line.credit)) : ''}</td>
                            </tr>
                          ))}
                        </tbody>
                      </table>
                    </td>
                  </tr>
                )}
              </>
            ))}
          </tbody>
        </table>
        {rows.length === 0 && <p className="text-gray-400 text-sm py-8 text-center">No journal entries yet.</p>}
      </div>
    </div>
  );
}
