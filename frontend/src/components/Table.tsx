import { useState, type ReactNode } from 'react';
import { Inbox, Search } from 'lucide-react';

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
      <div className="bg-white border border-gray-200/80 rounded-2xl py-16 flex flex-col items-center text-center shadow-sm">
        <span className="w-12 h-12 rounded-2xl bg-orange-50 text-orange-500 flex items-center justify-center mb-3">
          <Inbox size={22} />
        </span>
        <p className="text-gray-700 text-sm font-medium">{emptyMessage}</p>
        <p className="text-gray-400 text-xs mt-1">New records will appear here.</p>
      </div>
    );
  }

  const q = query.trim().toLowerCase();
  const rows = q ? data.filter((row) => JSON.stringify(row).toLowerCase().includes(q)) : data;

  return (
    <div className="rounded-2xl border border-gray-200/80 bg-white shadow-sm overflow-hidden">
      {searchable && data.length > 3 && (
        <div className="flex items-center justify-between gap-3 px-4 py-3 border-b border-gray-100">
          <label className="relative w-72 max-w-full">
            <Search size={15} className="absolute left-3 top-1/2 -translate-y-1/2 text-gray-400 pointer-events-none" />
            <input
              type="search"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              placeholder="Filter these rows…"
              className="!pl-9 !h-9 !bg-gray-50 focus:!bg-white"
              aria-label="Filter these rows"
            />
          </label>
          <span className="text-xs text-gray-400">
            {q ? `${rows.length} of ${data.length}` : `${data.length}`} {data.length === 1 ? 'row' : 'rows'}
          </span>
        </div>
      )}
      <div className="overflow-x-auto">
        <table className="min-w-full text-sm border-collapse">
          <thead>
            <tr className="bg-gray-50/80 border-b border-gray-200/80">
              {columns.map((col) => (
                <th
                  key={col.header}
                  className={`px-5 py-3 text-[11px] font-semibold text-gray-500 uppercase tracking-wider whitespace-nowrap ${alignOf(col.header, col.className)}`}
                >
                  {col.header}
                </th>
              ))}
            </tr>
          </thead>
          <tbody className="divide-y divide-gray-100">
            {rows.map((row) => (
              <tr key={row.id} className="hover:bg-orange-50/40 transition-colors">
                {columns.map((col) => (
                  <td
                    key={col.header}
                    className={`px-5 py-3.5 text-gray-700 align-middle ${alignOf(col.header, col.className)}`}
                  >
                    {typeof col.accessor === 'function'
                      ? col.accessor(row)
                      : String(row[col.accessor] ?? '')}
                  </td>
                ))}
              </tr>
            ))}
            {rows.length === 0 && (
              <tr>
                <td colSpan={columns.length} className="px-5 py-10 text-center text-sm text-gray-400">
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
