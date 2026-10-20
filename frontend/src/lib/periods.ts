/** Date arithmetic for comparison columns (D24). Dates are local `YYYY-MM-DD` strings. */

const parse = (iso: string) => {
  const [y, m, d] = iso.split('-').map(Number);
  return new Date(y, m - 1, d);
};
const fmt = (d: Date) =>
  `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`;

export const addDays = (iso: string, days: number) => {
  const d = parse(iso);
  d.setDate(d.getDate() + days);
  return fmt(d);
};

/** The same day `years` earlier; 29 February becomes 28 February. */
export const shiftYears = (iso: string, years: number) => {
  const d = parse(iso);
  const day = d.getDate();
  d.setDate(1);
  d.setFullYear(d.getFullYear() + years);
  const last = new Date(d.getFullYear(), d.getMonth() + 1, 0).getDate();
  d.setDate(Math.min(day, last));
  return fmt(d);
};

/** The period of the same length that ends the day before `from`. */
export const previousPeriod = (from: string, to: string) => {
  const days = Math.round((parse(to).getTime() - parse(from).getTime()) / 86_400_000);
  const prevTo = addDays(from, -1);
  return { dateFrom: addDays(prevTo, -days), dateTo: prevTo };
};

export const samePeriodLastYear = (from: string, to: string) => ({ dateFrom: shiftYears(from, -1), dateTo: shiftYears(to, -1) });

/** 31 December of the year before `iso`. */
export const lastYearEnd = (iso: string) => `${parse(iso).getFullYear() - 1}-12-31`;
