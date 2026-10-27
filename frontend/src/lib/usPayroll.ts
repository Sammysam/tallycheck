// US payroll (D42/D43, ported from the web edition): types and labels for /api/payroll/us/*.
import { useEffect, useState } from 'react';
import api from '../api/client';
import { amt, n } from './payroll';

export type PayFrequency = 'WEEKLY' | 'BIWEEKLY' | 'SEMIMONTHLY' | 'MONTHLY';
export type UsRunStatus = 'DRAFT' | 'POSTED' | 'PAID' | 'VOID';
export type UsRemitType =
  | 'FORM_941' | 'FEDERAL_INCOME_TAX' | 'SOCIAL_SECURITY' | 'MEDICARE' | 'FUTA' | 'SUTA' | 'RETIREMENT_401K' | 'SECTION_125'
  | 'STATE_INCOME_TAX';

export interface UsSettings {
  country: 'LK' | 'US';
  payFrequency: PayFrequency;
  periodsPerYear: number;
  state: string | null;
  sutaRate: number;
  sutaWageBase: number | null;
  futaCreditReduction: number;
  ein: string | null;
  stateAccountNo: string | null;
  taxYears: number[];
  /** D43: the account each part of a pay run posts to (null account = the default code). */
  accounts: UsAccountRole[];
}

export interface UsAccountRole {
  role: string;
  label: string;
  type: 'LIABILITY' | 'EXPENSE';
  defaultCode: string;
  defaultName: string;
  accountId: string | null;
  code: string | null;
  name: string | null;
}

export interface UsTax {
  employeeId: string;
  configured: boolean;
  workState: string | null;
  payType: 'SALARY' | 'HOURLY';
  annualSalary: number;
  hourlyRate: number;
  w4Version: '2020' | 'LEGACY';
  filingStatus: 'SINGLE' | 'MARRIED_JOINTLY' | 'HEAD_OF_HOUSEHOLD';
  multipleJobs: boolean;
  dependentsCredit: number;
  otherIncome: number;
  deductions: number;
  extraWithholding: number;
  allowances: number;
  exemptFromWithholding: boolean;
  pretax401k: number;
  pretaxSection125: number;
  stateFilingStatus: 'SINGLE' | 'MARRIED';
  stateAllowances: number;
  stateExtraWithholding: number;
  stateExempt: boolean;
  updatedAt: string | null;
}

/** A company's withholding rule for one state (`method` null: employees work there, no rule yet). */
export interface StateTaxRule {
  state: string;
  builtIn: boolean;
  method: 'NONE' | 'FLAT' | 'BRACKETS' | null;
  flatRate?: number;
  brackets?: [number | null, number][];
  bracketsMarried?: [number | null, number][] | null;
  standardDeductionSingle?: number;
  standardDeductionMarried?: number;
  allowanceDeduction?: number;
  allowanceCredit?: number;
  excludes401k?: boolean;
  notes?: string | null;
  updatedAt?: string | null;
}

export interface UsTaxFigures {
  incomeTaxWages: number;
  federalIncomeTax: number;
  socialSecurityWages: number;
  socialSecurityEmployee: number;
  socialSecurityEmployer: number;
  medicareWages: number;
  medicareEmployee: number;
  additionalMedicare: number;
  medicareEmployer: number;
  futaWages: number;
  futa: number;
  sutaWages: number;
  suta: number;
  employeeTaxes: number;
  netPay: number;
  employerTaxes: number;
  costToEmployer: number;
}

export interface UsPreview {
  name: string;
  payDate: string;
  payFrequency: PayFrequency;
  gross: number;
  pretax401k: number;
  pretaxSection125: number;
  taxes: UsTaxFigures;
  stateTax: { state: string; configured: boolean; wages: number; tax: number };
  notes: string[];
}

export interface UsRun {
  id: string;
  runNumber: string;
  periodStart: string;
  periodEnd: string;
  payDate: string;
  payFrequency: PayFrequency;
  status: UsRunStatus;
  notes: string | null;
  employeeCount: number;
  grossTotal: number;
  pretax401kTotal: number;
  pretaxSection125Total: number;
  federalIncomeTaxTotal: number;
  socialSecurityEmployeeTotal: number;
  socialSecurityEmployerTotal: number;
  medicareEmployeeTotal: number;
  additionalMedicareTotal: number;
  medicareEmployerTotal: number;
  futaTotal: number;
  sutaTotal: number;
  netPayTotal: number;
  employerTaxesTotal: number;
  stateIncomeTaxTotal: number;
  journalEntryId: string | null;
  paymentJournalEntryId: string | null;
  paymentAccountId: string | null;
  paidDate: string | null;
  postedAt: string | null;
  createdByName: string | null;
}

export interface UsLine extends UsTaxFigures {
  id: string;
  employeeId: string | null;
  employeeCode: string;
  employeeName: string;
  workState: string;
  payType: 'SALARY' | 'HOURLY';
  annualSalary: number;
  hourlyRate: number;
  hours: number;
  overtimeHours: number;
  regularPay: number;
  overtimePay: number;
  bonus: number;
  otherEarnings: number;
  gross: number;
  pretax401k: number;
  pretaxSection125: number;
  stateTaxConfigured: boolean;
  stateIncomeTaxWages: number;
  stateIncomeTax: number;
  notes: string | null;
}

export interface UsRunDetail extends UsRun {
  lines: UsLine[];
  remittances: { id: string; type: UsRemitType; amount: number; date: string; reference: string | null; journalEntryId: string; createdByName: string | null }[];
  taxes: { type: Exclude<UsRemitType, 'FORM_941'>; total: number; remitted: number; outstanding: number }[];
  skipped?: { employeeId: string; employeeCode: string; name: string; reason: string }[];
}

export const PAY_FREQUENCY_LABEL: Record<PayFrequency, string> = {
  WEEKLY: 'Weekly (52 a year)',
  BIWEEKLY: 'Every two weeks (26 a year)',
  SEMIMONTHLY: 'Twice a month (24 a year)',
  MONTHLY: 'Monthly (12 a year)',
};

export const FILING_LABEL: Record<UsTax['filingStatus'], string> = {
  SINGLE: 'Single or married filing separately',
  MARRIED_JOINTLY: 'Married filing jointly',
  HEAD_OF_HOUSEHOLD: 'Head of household',
};

export const US_REMIT_LABEL: Record<UsRemitType, string> = {
  FORM_941: 'Form 941 deposit (federal income tax + social security + Medicare)',
  FEDERAL_INCOME_TAX: 'Federal income tax',
  SOCIAL_SECURITY: 'Social security',
  MEDICARE: 'Medicare',
  FUTA: 'FUTA',
  SUTA: 'State unemployment (SUTA)',
  RETIREMENT_401K: '401(k)',
  SECTION_125: 'Section 125',
  STATE_INCOME_TAX: 'State income tax',
};

export const US_RUN_STATUS_HELP: Record<UsRunStatus, string> = {
  DRAFT: 'Editable. Nothing has reached the ledger.',
  POSTED: 'Booked to the ledger as wages, payroll tax expense and payables. Pay employees next.',
  PAID: 'Net pay sent. Deposit the taxes when they are due.',
  VOID: 'Reversed. The pay period can be run again.',
};

export const US_STATES = [
  'AL', 'AK', 'AZ', 'AR', 'CA', 'CO', 'CT', 'DE', 'DC', 'FL', 'GA', 'HI', 'ID', 'IL', 'IN', 'IA', 'KS', 'KY', 'LA',
  'ME', 'MD', 'MA', 'MI', 'MN', 'MS', 'MO', 'MT', 'NE', 'NV', 'NH', 'NJ', 'NM', 'NY', 'NC', 'ND', 'OH', 'OK', 'OR',
  'PA', 'RI', 'SC', 'SD', 'TN', 'TX', 'UT', 'VT', 'VA', 'WA', 'WV', 'WI', 'WY', 'PR', 'VI',
];

/** Dollars with the sign, for the US screens (whatever the company's home currency). */
export function usd(v: number | string | null | undefined): string {
  const x = n(v);
  return x < 0 ? `-$${amt(-x)}` : `$${amt(x)}`;
}

/** The company's payroll country, or null while loading / without the payroll permission. */
export function usePayrollCountry(enabled = true): 'LK' | 'US' | null {
  const [country, setCountry] = useState<'LK' | 'US' | null>(null);
  useEffect(() => {
    if (!enabled) return;
    api.get<UsSettings>('/payroll/us/settings').then((r) => setCountry(r.data.country)).catch(() => setCountry(null));
  }, [enabled]);
  return country;
}
