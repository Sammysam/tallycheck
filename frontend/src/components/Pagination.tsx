interface PaginationProps {
  page: number;
  totalPages: number;
  total: number;
  limit: number;
  onPage: (p: number) => void;
}

export default function Pagination({ page, totalPages, total, limit, onPage }: PaginationProps) {
  if (totalPages <= 1) return null;

  const from = (page - 1) * limit + 1;
  const to   = Math.min(page * limit, total);

  return (
    <div className="flex items-center justify-between mt-4 text-sm">
      <span className="text-gray-500">
        Showing {from}–{to} of {total}
      </span>
      <div className="flex gap-1">
        <button
          onClick={() => onPage(page - 1)}
          disabled={page === 1}
          className="btn btn-secondary disabled:opacity-40 disabled:cursor-not-allowed"
          style={{ height: '1.875rem', padding: '0 0.625rem', fontSize: '0.75rem' }}
        >
          ← Prev
        </button>
        {Array.from({ length: Math.min(totalPages, 7) }, (_, i) => {
          const p = totalPages <= 7
            ? i + 1
            : page <= 4
              ? i + 1
              : page >= totalPages - 3
                ? totalPages - 6 + i
                : page - 3 + i;
          return (
            <button
              key={p}
              onClick={() => onPage(p)}
              className="btn"
              style={{
                height: '1.875rem',
                padding: '0 0.625rem',
                fontSize: '0.75rem',
                backgroundColor: p === page ? '#2563eb' : '#fff',
                color: p === page ? '#fff' : '#374151',
                border: '1px solid',
                borderColor: p === page ? '#2563eb' : '#d1d5db',
              }}
            >
              {p}
            </button>
          );
        })}
        <button
          onClick={() => onPage(page + 1)}
          disabled={page === totalPages}
          className="btn btn-secondary disabled:opacity-40 disabled:cursor-not-allowed"
          style={{ height: '1.875rem', padding: '0 0.625rem', fontSize: '0.75rem' }}
        >
          Next →
        </button>
      </div>
    </div>
  );
}
