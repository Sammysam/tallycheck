import { useEffect, useState } from 'react';
import api from '../api/client';
import { currencyDigits } from './money';

/**
 * D26/D27: tax codes and the same arithmetic the server uses, so a form shows exactly the
 * figures that will be saved: integer cents, rates in ten-thousandths of a percent, each part
 * of the tax rounded half-up per line, groups split into their parts.
 */

export type TaxKind = 'STANDARD' | 'GROUP' | 'WITHHOLDING';

export interface TaxCode {
  id: string;
  name: string;
  kind: TaxKind;
  /** Percent, e.g. 18 or 8.875 (for a group: the combined rate). */
  rate: number;
  description: string | null;
  salesAccountId: string;
  purchaseAccountId: string;
  isActive: boolean;
  components: { taxCodeId: string; isCompound: boolean }[];
}

let cache: TaxCode[] = [];

/** The company's tax codes (all of them; filter `isActive` for new documents). */
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

/** Codes that can go on an invoice, bill, expense or note line (not withholding). */
export const lineCodes = (codes: TaxCode[], keep?: string) =>
  codes.filter((c) => c.kind !== 'WITHHOLDING' && (c.isActive || c.id === keep));

const toCents = (v: number) => BigInt(Math.round((Number(v) || 0) * 100));
const fromCents = (c: bigint) => Number(c) / 100;
const roundHalfUp = (n: bigint, d: bigint) => {
  const q = n / d;
  const r = n % d;
  return 2n * (r < 0n ? -r : r) >= d ? q + (n < 0n ? -1n : 1n) : q;
};
const HUNDRED = 1_000_000n;
/** Rounds n / d half-up to the cent, or to a whole unit for a currency without decimals (D29). */
const roundMoney = (n: bigint, d: bigint) => {
  const step = currencyDigits() === 0 ? 100n : 1n;
  return roundHalfUp(n, d * step) * step;
};
const rateUnits = (percent: number) => BigInt(Math.round(percent * 10_000));

/** The tax in `amount` at one `ratePercent`, rounded half-up to the cent. */
export function taxOf(amount: number, ratePercent: number, inclusive: boolean): number {
  const rate = rateUnits(ratePercent);
  const cents = toCents(amount);
  return fromCents(inclusive ? roundMoney(cents * rate, HUNDRED + rate) : roundMoney(cents * rate, HUNDRED));
}

/** `[code id, rate units, compound]` of each part a line code charges. */
function partsOf(code: TaxCode): [string, bigint, boolean][] {
  if (code.kind !== 'GROUP') return [[code.id, rateUnits(code.rate), false]];
  return code.components.flatMap((c) => {
    const part = taxCodeById(c.taxCodeId);
    return part ? [[part.id, rateUnits(part.rate), c.isCompound] as [string, bigint, boolean]] : [];
  });
}

/** The parts of the tax on a line amount, in cents — the server's `split_parts`. */
function splitParts(amount: number, rates: [string, bigint, boolean][], inclusive: boolean): [string, bigint][] {
  const cents = toCents(amount);
  const exclusiveOn = (net: bigint) => {
    let before = 0n;
    return rates.map(([id, rate, compound]) => {
      const t = roundMoney((net + (compound ? before : 0n)) * rate, HUNDRED);
      before += t;
      return [id, t] as [string, bigint];
    });
  };
  if (!inclusive) return exclusiveOn(cents);
  if (rates.length === 1) return [[rates[0][0], roundMoney(cents * rates[0][1], HUNDRED + rates[0][1])]];
  const S = 1_000_000_000_000n;
  let combined = 0n;
  for (const [, rate, compound] of rates) {
    combined += ((S + (compound ? combined : 0n)) * (rate * 1_000_000n)) / S;
  }
  const net = roundMoney(cents * S, S + combined);
  const parts = exclusiveOn(net);
  const sum = parts.reduce((s, [, t]) => s + t, 0n);
  if (parts.length) parts[parts.length - 1][1] += cents - net - sum;
  return parts;
}

export interface TaxSummary {
  /** Total of the lines without tax. */
  subtotal: number;
  tax: number;
  total: number;
  /** Tax per standard code (a group's parts separately), in first-use order. */
  byCode: { id: string; name: string; tax: number }[];
  /** Tax per line, same order as the input. */
  lineTax: number[];
}

/** Subtotal, tax and total of document lines (`amount` = quantity × price as typed). */
export function summarize(lines: { amount: number; taxCodeId?: string | null }[], inclusive: boolean): TaxSummary {
  let subtotal = 0n;
  let tax = 0n;
  const byCode = new Map<string, bigint>();
  const lineTax: number[] = [];
  for (const l of lines) {
    const code = taxCodeById(l.taxCodeId);
    const parts = code && code.kind !== 'WITHHOLDING' ? splitParts(l.amount, partsOf(code), inclusive) : [];
    const t = parts.reduce((s, [, p]) => s + p, 0n);
    const amount = toCents(l.amount);
    subtotal += inclusive ? amount - t : amount;
    tax += t;
    lineTax.push(fromCents(t));
    for (const [id, p] of parts) if (p !== 0n) byCode.set(id, (byCode.get(id) ?? 0n) + p);
  }
  return {
    subtotal: fromCents(subtotal),
    tax: fromCents(tax),
    total: fromCents(subtotal + tax),
    byCode: [...byCode].map(([id, t]) => ({ id, name: taxCodeById(id)?.name ?? 'Tax', tax: fromCents(t) })),
    lineTax,
  };
}

/** The tax inside a gross amount (expenses, notes) at a line code, all parts together. */
export function taxInGross(amount: number, code: TaxCode | undefined): number {
  if (!code || code.kind === 'WITHHOLDING') return 0;
  return fromCents(splitParts(amount, partsOf(code), true).reduce((s, [, p]) => s + p, 0n));
}

/** A line's tax by standard code, as the invoice prints it. */
export function lineParts(amount: number, codeId: string | null | undefined, inclusive: boolean) {
  const code = taxCodeById(codeId);
  if (!code || code.kind === 'WITHHOLDING') return [];
  return splitParts(amount, partsOf(code), inclusive).map(([taxCodeId, t]) => ({ taxCodeId, taxAmount: fromCents(t) }));
}
