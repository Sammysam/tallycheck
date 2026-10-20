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
  /** The same line for the comparison period (D24), when one is shown. */
  compare?: number;
  /** Bold, with a rule above: a subtotal or total. */
  total?: boolean;
}

const cents = (v: number) => Math.round(v * 100);

/**
 * One block of a financial statement as a grid: label on the left, amount on the right. With
 * `compareHeader`, a comparison column and a Change column (amount − comparison) follow.
 */
export function ReportTable({ title, rows, amountHeader = 'Amount', compareHeader }: {
  title: string;
  rows: ReportRow[];
  amountHeader?: string;
  compareHeader?: string;
}) {
  const span = compareHeader ? 4 : 2;
  return (
    <table className="report-table">
      <thead>
        <tr>
          <th>{title}</th>
          <th className="num">{amountHeader}</th>
          {compareHeader && <th className="num">{compareHeader}</th>}
          {compareHeader && <th className="num">Change</th>}
        </tr>
      </thead>
      <tbody>
        {rows.map((r) => {
          const change = (cents(r.amount) - cents(r.compare ?? 0)) / 100;
          return (
            <tr key={r.key} className={r.total ? 'total' : ''}>
              <td>{r.label}</td>
              <td className={`num ${r.amount < 0 ? 'neg' : ''}`}>{money(r.amount)}</td>
              {compareHeader && <td className={`num ${(r.compare ?? 0) < 0 ? 'neg' : ''}`}>{money(r.compare ?? 0)}</td>}
              {compareHeader && <td className={`num ${change < 0 ? 'neg' : ''}`}>{money(change)}</td>}
            </tr>
          );
        })}
        {rows.length === 0 && (
          <tr><td colSpan={span} className="empty">No accounts with a balance.</td></tr>
        )}
      </tbody>
    </table>
  );
}
