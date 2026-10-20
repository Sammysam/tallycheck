import { region } from './region';

/** An amount in the company's currency and number format (D25), e.g. "LKR 2,500.63" or "£2,500.63". */
export const money = (v: number) => {
  const r = region();
  try {
    return new Intl.NumberFormat(r.locale, {
      style: 'currency',
      currency: r.currency,
      minimumFractionDigits: 2,
      maximumFractionDigits: 2,
    }).format(Number(v) || 0);
  } catch {
    return `${r.currency} ${amount(v)}`;
  }
};

/** An amount without the currency, in the company's number format, e.g. "2,500.63". */
export const amount = (v: number | string) => {
  try {
    return new Intl.NumberFormat(region().locale, { minimumFractionDigits: 2, maximumFractionDigits: 2 }).format(Number(v) || 0);
  } catch {
    return (Number(v) || 0).toFixed(2);
  }
};

/** Short amounts for charts: "2.5K". */
export const compactAmount = (v: number) => {
  try {
    return new Intl.NumberFormat(region().locale, { notation: 'compact', maximumFractionDigits: 1 }).format(v ?? 0);
  } catch {
    return String(v);
  }
};
