import type { ReactNode } from 'react';

interface Column<T> {
  header: string;
  accessor: keyof T | ((row: T) => ReactNode);
  className?: string;
}

interface TableProps<T> {
  columns: Column<T>[];
  data: T[];
  emptyMessage?: string;
}

export default function Table<T extends { id: string }>({
  columns,
  data,
  emptyMessage = 'No records found.',
}: TableProps<T>) {
  if (data.length === 0) {
    return (
      <div className="bg-white border border-gray-200 rounded-lg py-12 text-center">
        <p className="text-gray-400 text-sm">{emptyMessage}</p>
      </div>
    );
  }

  return (
    <div className="overflow-x-auto rounded-lg border border-gray-200 bg-white shadow-sm">
      <table className="min-w-full text-sm border-collapse">
        <thead>
          <tr className="bg-gray-50 border-b border-gray-200">
            {columns.map((col) => (
              <th
                key={col.header}
                className={`px-4 py-3 text-left text-xs font-semibold text-gray-500 uppercase tracking-wide whitespace-nowrap ${col.className ?? ''}`}
              >
                {col.header}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {data.map((row, rowIdx) => (
            <tr
              key={row.id}
              className={`border-b border-gray-100 hover:bg-blue-50/40 transition-colors ${rowIdx === data.length - 1 ? 'border-b-0' : ''}`}
            >
              {columns.map((col) => (
                <td
                  key={col.header}
                  className={`px-4 py-3 text-gray-700 align-middle ${col.className ?? ''}`}
                >
                  {typeof col.accessor === 'function'
                    ? col.accessor(row)
                    : String(row[col.accessor] ?? '')}
                </td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
