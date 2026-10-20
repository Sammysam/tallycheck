import { AccountLink, type ReportRow } from '../components/ReportTable';

export interface AccountRef { id: string; code: string; name: string }
export interface AccountLine { account: AccountRef; balance: number }

/** The same section for the comparison period (D24). */
export interface Comparison { lines: AccountLine[]; total: number }

/**
 * Account lines (each a link to its entries, D23) followed by their "Total …" row. With a
 * comparison, every row carries the comparison figure too; an account that only has a figure
 * in the comparison period is listed as well.
 */
export function sectionRows(
  title: string,
  lines: AccountLine[],
  total: number,
  dates: { dateFrom?: string; dateTo?: string },
  comparison?: Comparison,
): ReportRow[] {
  const before = new Map((comparison?.lines ?? []).map((l) => [l.account.id, Number(l.balance)]));
  const extra = (comparison?.lines ?? []).filter((c) => !lines.some((l) => l.account.id === c.account.id));
  const all = [...lines, ...extra.map((c) => ({ account: c.account, balance: 0 }))];
  return [
    ...all.map((l) => ({
      key: l.account.id,
      label: <AccountLink account={l.account} {...dates} />,
      amount: Number(l.balance),
      compare: comparison ? before.get(l.account.id) ?? 0 : undefined,
    })),
    {
      key: `total-${title}`,
      label: `Total ${title}`,
      amount: Number(total),
      compare: comparison ? Number(comparison.total) : undefined,
      total: true,
    },
  ];
}
