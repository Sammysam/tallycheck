/** D61: recurring invoice schedules: labels and date steps shared by the screens. */

export interface Schedule {
  name: string;
  frequency: 'WEEKLY' | 'MONTHLY' | 'QUARTERLY' | 'YEARLY';
  every: number;
  startDate: string;
  endDate: string;
}

export const UNIT: Record<Schedule['frequency'], [string, string]> = {
  WEEKLY: ['week', 'weeks'],
  MONTHLY: ['month', 'months'],
  QUARTERLY: ['quarter', 'quarters'],
  YEARLY: ['year', 'years'],
};

/** "Every month", "Every 2 weeks". */
export const repeatsLabel = (frequency: Schedule['frequency'], every: number) =>
  every === 1 ? `Every ${UNIT[frequency][0]}` : `Every ${every} ${UNIT[frequency][1]}`;

const iso = (d: Date) => `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`;

/** One step after `date`, months clamped to the month's last day — as the server counts. */
export function nextAfter(date: string, frequency: Schedule['frequency'], every = 1): string {
  const [y, m, d] = date.split('-').map(Number);
  if (!y || !m || !d) return date;
  if (frequency === 'WEEKLY') return iso(new Date(y, m - 1, d + 7 * every));
  const months = every * (frequency === 'MONTHLY' ? 1 : frequency === 'QUARTERLY' ? 3 : 12);
  const target = new Date(y, m - 1 + months, 1);
  const last = new Date(target.getFullYear(), target.getMonth() + 1, 0).getDate();
  return iso(new Date(target.getFullYear(), target.getMonth(), Math.min(d, last)));
}
