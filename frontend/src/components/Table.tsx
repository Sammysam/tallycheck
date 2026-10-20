import { useState, type ReactNode } from 'react';
import { Inbox, Search } from 'lucide-react';
import { fmtDate, isIsoDate } from '../lib/region';

interface Column<T> {
  header: string;
  accessor: keyof T | ((row: T) => ReactNode);
  className?: string;
}

interface TableProps<T> {
  columns: Column<T>[];
  data: T[];
  emptyMessage?: string;
  /** Shows a box that filters the rows on screen. Off where the page has its own search. */
  searchable?: boolean;
}

/** Money columns line up on the right. */
const AMOUNT_HEADER = /amount|paid|total|balance|debit|credit|salary|gross|net pay|outstanding/i;

function alignOf(header: string, className?: string) {
  if (className?.includes('text-')) return className;
  return `${AMOUNT_HEADER.test(header) ? 'text-right tabular-nums' : 'text-left'} ${className ?? ''}`;
}

export default function Table<T extends { id: string }>({
  columns,
  data,
  emptyMessage = 'No records found.',
  searchable = true,
}: TableProps<T>) {
  const [query, setQuery] = useState('');

  if (data.length === 0) {
    return (
      <div className="bg-white border border-[#e0e0e0] rounded-md py-10 flex flex-col items-center text-center">
        <Inbox size={28} strokeWidth={1.5} className="text-gray-400 mb-2" />
        <p className="text-gray-700 text-sm font-medium">{emptyMessage}</p>
        <p className="text-gray-400 text-xs mt-1">New records will appear here.</p>
      </div>
    );
  }

  const q = query.trim().toLowerCase();
  const rows = q ? data.filter((row) => JSON.stringify(row).toLowerCase().includes(q)) : data;

  return (
    <div className="rounded-md border border-[#e0e0e0] bg-white overflow-hidden">
      {searchable && data.length > 3 && (
        <div className="flex items-center justify-between gap-3 px-2 py-1.5 border-b border-[#e5e5e5] bg-[#fafafa]">
          <label className="relative w-64 max-w-full">
            <Search size={14} className="absolute left-2.5 top-1/2 -translate-y-1/2 text-gray-400 pointer-events-none" />
            <input
              type="search"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              placeholder="Filter these rows…"
              className="!pl-8 !h-7"
              aria-label="Filter these rows"
            />
          </label>
          <span className="text-xs text-gray-500 pr-1">
            {q ? `${rows.length} of ${data.length}` : `${data.length}`} {data.length === 1 ? 'row' : 'rows'}
          </span>
        </div>
      )}
      <div className="overflow-x-auto">
        <table className="min-w-full text-sm border-collapse">
          <thead>
            <tr className="bg-[#f5f5f5] border-b border-[#d9d9d9]">
              {columns.map((col) => (
                <th
                  key={col.header}
                  className={`sticky top-0 px-3 py-1.5 text-xs font-semibold text-gray-700 whitespace-nowrap border-r border-[#e5e5e5] last:border-r-0 ${alignOf(col.header, col.className)}`}
                >
                  {col.header}
                </th>
              ))}
            </tr>
          </thead>
          <tbody>
            {rows.map((row) => (
              <tr key={row.id} className="border-b border-[#efefef] last:border-b-0 even:bg-[#fbfbfb] hover:bg-[#fff4ec]">
                {columns.map((col) => (
                  <td
                    key={col.header}
                    className={`px-3 py-1.5 text-[#1b1b1b] align-middle ${alignOf(col.header, col.className)}`}
                  >
                    {typeof col.accessor === 'function'
                      ? col.accessor(row)
                      : isIsoDate(row[col.accessor])
                        ? <span data-value={String(row[col.accessor])}>{fmtDate(String(row[col.accessor]))}</span>
                        : String(row[col.accessor] ?? '')}
                  </td>
                ))}
              </tr>
            ))}
            {rows.length === 0 && (
              <tr>
                <td colSpan={columns.length} className="px-3 py-8 text-center text-sm text-gray-500">
                  Nothing matches “{query}”.
                </td>
              </tr>
            )}
          </tbody>
        </table>
      </div>
    </div>
  );
}
