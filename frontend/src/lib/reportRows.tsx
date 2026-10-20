import { AccountLink, type ReportRow } from '../components/ReportTable';

export interface AccountRef { id: string; code: string; name: string }
export interface AccountLine { account: AccountRef; balance: number }

/** Account lines (each a link to its entries, D23) followed by their "Total …" row. */
export function sectionRows(
  title: string,
  lines: AccountLine[],
  total: number,
  dates: { dateFrom?: string; dateTo?: string },
): ReportRow[] {
  return [
    ...lines.map((l) => ({
      key: l.account.id,
      label: <AccountLink account={l.account} {...dates} />,
      amount: Number(l.balance),
    })),
    { key: `total-${title}`, label: `Total ${title}`, amount: Number(total), total: true },
  ];
}
