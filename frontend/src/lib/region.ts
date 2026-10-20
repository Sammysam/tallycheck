/**
 * D25: the customer's own company details and region, from Settings. Nothing about any
 * particular company is built into the app; until the administrator fills these in, the app
 * shows neutral placeholders (no name, no logo, US dollars).
 *
 * Loaded once after sign-in (Layout) and read synchronously everywhere else — money
 * formatting, invoice and statement letterheads, amount-in-words, default report dates.
 */

export interface Region {
  companyName: string;
  address: string;
  phone: string;
  fax: string;
  email: string;
  website: string;
  taxNumber: string;
  /** A data: URL uploaded on the Settings page, or '' for no logo. */
  logo: string;
  paymentInstructions: string;
  footerNote: string;
  country: string;
  currency: string;
  currencySymbol: string;
  currencyWord: string;
  currencyMinorWord: string;
  locale: string;
  /** 1 = January … 12 = December. */
  fiscalYearStart: number;
}

const NEUTRAL: Region = {
  companyName: '',
  address: '',
  phone: '',
  fax: '',
  email: '',
  website: '',
  taxNumber: '',
  logo: '',
  paymentInstructions: '',
  footerNote: '',
  country: '',
  currency: 'USD',
  currencySymbol: '$',
  currencyWord: 'Dollars',
  currencyMinorWord: 'Cents',
  locale: 'en-US',
  fiscalYearStart: 1,
};

let current: Region = NEUTRAL;
const listeners = new Set<() => void>();

export const region = () => current;

/** Builds the region from the `GET /api/settings` map. */
export function regionFromSettings(s: Record<string, string>): Region {
  const v = (k: string, fallback: string) => (s[k] ?? '').trim() || fallback;
  const month = Number(s['region.fiscalYearStart']);
  return {
    companyName: v('company.name', ''),
    address: v('company.address', ''),
    phone: v('company.phone', ''),
    fax: v('company.fax', ''),
    email: v('company.email', ''),
    website: v('company.website', ''),
    taxNumber: v('company.taxNumber', ''),
    logo: v('company.logo', ''),
    paymentInstructions: v('company.paymentInstructions', ''),
    footerNote: v('company.footerNote', ''),
    country: v('region.country', ''),
    currency: v('region.currency', NEUTRAL.currency),
    currencySymbol: v('region.currencySymbol', v('region.currency', NEUTRAL.currencySymbol)),
    currencyWord: v('region.currencyWord', NEUTRAL.currencyWord),
    currencyMinorWord: v('region.currencyMinorWord', NEUTRAL.currencyMinorWord),
    locale: v('region.locale', NEUTRAL.locale),
    fiscalYearStart: month >= 1 && month <= 12 ? month : 1,
  };
}

export function setRegion(r: Region) {
  current = r;
  listeners.forEach((l) => l());
}

export function subscribeRegion(listener: () => void) {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

/** True once the administrator has entered the company. */
export const companyIsSetUp = (s: Record<string, string>) => !!(s['company.name'] ?? '').trim();

const lines = (v: string) => v.split(/\r?\n/).map((l) => l.trim()).filter(Boolean);

/** The letterhead and remittance details printed on invoices and statements. */
export function company() {
  const r = current;
  const contact = [lines(r.address).join(', '), r.phone && `Tel: ${r.phone}`, r.fax && `Fax: ${r.fax}`]
    .filter(Boolean)
    .join('   ');
  return {
    name: r.companyName,
    logo: r.logo,
    addressLines: lines(r.address),
    contactLine: contact,
    emailWebLine: [r.email, r.website].filter(Boolean).join('  |  '),
    taxNumber: r.taxNumber,
    paymentOptions: lines(r.paymentInstructions),
    footerNote: r.footerNote,
  };
}

/** The first day of the financial year that contains `today` (local `YYYY-MM-DD`). */
export function fiscalYearStart(today = new Date()): string {
  const m = current.fiscalYearStart;
  const year = today.getMonth() + 1 >= m ? today.getFullYear() : today.getFullYear() - 1;
  return `${year}-${String(m).padStart(2, '0')}-01`;
}

/** The local date as `YYYY-MM-DD`. */
export function todayIso(d = new Date()): string {
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`;
}
