import { useEffect, useState } from 'react';
import api from '../api/client';

/**
 * D26: tax codes and the same arithmetic the server uses, so a form shows exactly the figures
 * that will be saved: integer cents, rate in ten-thousandths of a percent, half-up per line.
 */

export interface TaxCode {
  id: string;
  name: string;
  /** Percent, e.g. 18 or 8.875. */
  rate: number;
  description: string | null;
  salesAccountId: string;
  purchaseAccountId: string;
  isActive: boolean;
}

let cache: TaxCode[] = [];

/** The company's tax codes (all of them; filter `isActive` for new lines). */
export function useTaxCodes(): TaxCode[] {
  const [codes, setCodes] = useState<TaxCode[]>(cache);
  useEffect(() => {
    api.get('/tax-codes')
      .then((r) => {
        cache = r.data;
        setCodes(r.data);
      })
      .catch(() => {});
  }, []);
  return codes;
}

export const taxCodeById = (id?: string | null) => (id ? cache.find((c) => c.id === id) : undefined);

const toCents = (v: number) => BigInt(Math.round((Number(v) || 0) * 100));
const fromCents = (c: bigint) => Number(c) / 100;
const roundHalfUp = (n: bigint, d: bigint) => {
  const q = n / d;
  const r = n % d;
  return 2n * (r < 0n ? -r : r) >= d ? q + (n < 0n ? -1n : 1n) : q;
};

/** The tax in `amount` at `ratePercent`, rounded half-up to the cent. */
export function taxOf(amount: number, ratePercent: number, inclusive: boolean): number {
  const rate = BigInt(Math.round(ratePercent * 10_000));
  const hundred = 1_000_000n;
  const cents = toCents(amount);
  return fromCents(inclusive ? roundHalfUp(cents * rate, hundred + rate) : roundHalfUp(cents * rate, hundred));
}

export interface TaxSummary {
  /** Total of the lines without tax. */
  subtotal: number;
  tax: number;
  total: number;
  /** Tax per code, in first-use order. */
  byCode: { id: string; name: string; tax: number }[];
  /** Tax per line, same order as the input. */
  lineTax: number[];
}

/** Subtotal, tax and total of document lines (`amount` = quantity × price as typed). */
export function summarize(lines: { amount: number; taxCodeId?: string | null }[], inclusive: boolean): TaxSummary {
  let subtotal = 0n;
  let tax = 0n;
  const byCode: TaxSummary['byCode'] = [];
  const lineTax: number[] = [];
  for (const l of lines) {
    const code = taxCodeById(l.taxCodeId);
    const t = code ? toCents(taxOf(l.amount, code.rate, inclusive)) : 0n;
    const amount = toCents(l.amount);
    subtotal += inclusive ? amount - t : amount;
    tax += t;
    lineTax.push(fromCents(t));
    if (code && t !== 0n) {
      const row = byCode.find((b) => b.id === code.id);
      if (row) row.tax = fromCents(toCents(row.tax) + t);
      else byCode.push({ id: code.id, name: code.name, tax: fromCents(t) });
    }
  }
  return { subtotal: fromCents(subtotal), tax: fromCents(tax), total: fromCents(subtotal + tax), byCode, lineTax };
}
