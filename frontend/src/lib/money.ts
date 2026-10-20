import { region } from './region';

/** Decimal places of the company's currency: 2, or 0 for yen, won and the like (D29). */
export const currencyDigits = () => {
  try {
    return new Intl.NumberFormat('en', { style: 'currency', currency: region().currency }).resolvedOptions().maximumFractionDigits ?? 2;
  } catch {
    return 2;
  }
};

/** An amount in the company's currency and number format (D25), e.g. "LKR 2,500.63" or "£2,500.63". */
export const money = (v: number) => {
  const r = region();
  try {
    return new Intl.NumberFormat(r.locale, {
      style: 'currency',
      currency: r.currency,
      minimumFractionDigits: currencyDigits(),
      maximumFractionDigits: currencyDigits(),
    }).format(Number(v) || 0);
  } catch {
    return `${r.currency} ${amount(v)}`;
  }
};

/** An amount without the currency, in the company's number format, e.g. "2,500.63" ("2,501" for yen). */
export const amount = (v: number | string) => {
  try {
    const digits = currencyDigits();
    return new Intl.NumberFormat(region().locale, { minimumFractionDigits: digits, maximumFractionDigits: digits }).format(Number(v) || 0);
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

/**
 * D30: an amount in a document's own currency ("USD 110.00"); the home currency when `currency`
 * is empty or the home code.
 */
export const moneyIn = (v: number, currency?: string | null) => {
  if (!currency || currency === region().currency) return money(v);
  try {
    return new Intl.NumberFormat(region().locale, { style: 'currency', currency }).format(Number(v) || 0);
  } catch {
    return `${currency} ${(Number(v) || 0).toFixed(2)}`;
  }
};
