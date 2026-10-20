import type { ReactNode } from 'react';
import { Link } from 'react-router-dom';
import { useAuth } from '../contexts/AuthContext';
import { money } from '../lib/money';

interface AccountRef { id: string; code: string; name: string }

/**
 * D23: an account name in a report opens that account's lines for the same dates. Without the
 * `ledger` permission it stays plain text.
 */
export function AccountLink({ account, dateFrom, dateTo }: { account: AccountRef; dateFrom?: string; dateTo?: string }) {
  const { hasPermission } = useAuth();
  const label = `${account.code} ${account.name}`;
  if (!hasPermission('ledger')) return <>{label}</>;
  const q = new URLSearchParams();
  if (dateFrom) q.set('dateFrom', dateFrom);
  if (dateTo) q.set('dateTo', dateTo);
  const query = q.toString();
  return (
    <Link to={`/ledger/account/${account.id}${query ? `?${query}` : ''}`} className="report-link" title="Show the entries behind this figure">
      {label}
    </Link>
  );
}

export interface ReportRow {
  key: string;
  label: ReactNode;
  amount: number;
  /** Bold, with a rule above: a subtotal or total. */
  total?: boolean;
}

/** One block of a financial statement as a grid: label on the left, amount on the right. */
export function ReportTable({ title, rows, amountHeader = 'Amount' }: { title: string; rows: ReportRow[]; amountHeader?: string }) {
  return (
    <table className="report-table">
      <thead>
        <tr>
          <th>{title}</th>
          <th className="num">{amountHeader}</th>
        </tr>
      </thead>
      <tbody>
        {rows.map((r) => (
          <tr key={r.key} className={r.total ? 'total' : ''}>
            <td>{r.label}</td>
            <td className={`num ${r.amount < 0 ? 'neg' : ''}`}>{money(r.amount)}</td>
          </tr>
        ))}
        {rows.length === 0 && (
          <tr><td colSpan={2} className="empty">No accounts with a balance.</td></tr>
        )}
      </tbody>
    </table>
  );
}
