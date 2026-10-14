const colors: Record<string, string> = {
  DRAFT:   'bg-gray-100 text-gray-600',
  POSTED:  'bg-blue-100 text-blue-700',
  PAID:    'bg-green-100 text-green-700',
  VOID:    'bg-red-100 text-red-600',
  REVERSED:'bg-yellow-100 text-yellow-700',
  ACTIVE:  'bg-green-100 text-green-700',
  ON_LEAVE:'bg-amber-100 text-amber-700',
  LEFT:    'bg-gray-200 text-gray-600',
};

export default function StatusBadge({ status }: { status: string }) {
  return (
    <span className={`px-2 py-0.5 rounded text-xs font-medium ${colors[status] ?? 'bg-gray-100 text-gray-600'}`}>
      {status}
    </span>
  );
}
