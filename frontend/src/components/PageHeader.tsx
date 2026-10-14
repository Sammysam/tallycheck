import type { ReactNode } from 'react';

interface PageHeaderProps {
  title: string;
  action?: ReactNode;
}

export default function PageHeader({ title, action }: PageHeaderProps) {
  return (
    <div className="flex items-center justify-between mb-5">
      <h1 className="text-xl font-bold text-gray-900 tracking-tight">{title}</h1>
      {action && <div className="flex items-center gap-2">{action}</div>}
    </div>
  );
}
