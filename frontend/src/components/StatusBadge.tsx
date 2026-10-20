const colors: Record<string, [string, string]> = {
  DRAFT:   ['bg-gray-100 text-gray-600 ring-gray-200',         'bg-gray-400'],
  POSTED:  ['bg-blue-50 text-blue-700 ring-blue-200',          'bg-blue-500'],
  PAID:    ['bg-emerald-50 text-emerald-700 ring-emerald-200', 'bg-emerald-500'],
  COMPLETED:['bg-emerald-50 text-emerald-700 ring-emerald-200', 'bg-emerald-500'],
  VOID:    ['bg-red-50 text-red-600 ring-red-200',             'bg-red-500'],
  REVERSED:['bg-amber-50 text-amber-700 ring-amber-200',       'bg-amber-500'],
  ACTIVE:  ['bg-emerald-50 text-emerald-700 ring-emerald-200', 'bg-emerald-500'],
  ON_LEAVE:['bg-amber-50 text-amber-700 ring-amber-200',       'bg-amber-500'],
  LEFT:    ['bg-gray-100 text-gray-600 ring-gray-200',         'bg-gray-400'],
};

export default function StatusBadge({ status }: { status: string }) {
  const [pill, dot] = colors[status] ?? colors.DRAFT;
  return (
    <span className={`inline-flex items-center gap-1.5 px-1.5 py-px rounded-sm text-[11px] font-semibold ring-1 ring-inset ${pill}`}>
      <span className={`w-1.5 h-1.5 rounded-full ${dot}`} />
      {status}
    </span>
  );
}
