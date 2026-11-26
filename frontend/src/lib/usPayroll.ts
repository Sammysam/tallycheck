// US payroll (D42/D43, ported from the web edition): types and labels for /api/payroll/us/*.
import { useEffect, useState } from 'react';
import api, { errMsg } from '../api/client';
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
  stateFilingStatus: 'SINGLE' | 'MARRIED' | 'HEAD_OF_HOUSEHOLD';
  /** D64. */
  stateSecondAllowances: number;
  stateDeductionOverride: number | null;
  stateAnnualCredit: number;
  stateElectedRate: number | null;
  /** D65: exemption dollars (Mississippi Form 89-350 line 6). */
  stateAnnualDeduction: number;
  /** D66: the state form's status code or rate-table letter (null = from the filing status). */
  stateCode: string | null;
  stateAllowances: number;
  stateExtraWithholding: number;
  stateExempt: boolean;
  /** D63: home state (null = lives in the work state) and the other states worked in. */
  residentState: string | null;
  otherWorkStates: { state: string; percent: number }[];
  updatedAt: string | null;
}

/** A company's withholding rule for one state (`method` null: employees work there, no rule yet). */
export interface StateTaxRule {
  state: string;
  builtIn: boolean;
  method: 'NONE' | 'FLAT' | 'BRACKETS' | 'EMPLOYEE_RATE' | null;
  /** D64: NO_TAX built in; BUILT_IN 2026 rule from the state's guide; COMPANY entered; MISSING. */
  kind?: 'NO_TAX' | 'BUILT_IN' | 'COMPANY' | 'MISSING';
  source?: string;
  from?: string;
  builtInAvailable?: boolean;
  rounding?: 'CENT' | 'DOLLAR';
  standardDeductionHeadOfHousehold?: number | null;
  secondAllowanceDeduction?: number;
  secondAllowanceCredit?: number;
  flatRate?: number;
  brackets?: [number | null, number][];
  bracketsMarried?: [number | null, number][] | null;
  standardDeductionSingle?: number;
  standardDeductionMarried?: number;
  allowanceDeduction?: number;
  allowanceCredit?: number;
  excludes401k?: boolean;
  residentCredit?: 'NONE' | 'DIFFERENCE';
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
  stateTaxes?: UsStateTax[];
  unemploymentState?: string;
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
  /** D63. */
  residentState: string | null;
  workSplit: { state: string; percent: number }[];
  stateTaxes: UsStateTax[];
  sutaState: string | null;
  notes: string | null;
}

/** One state's part of a paycheck: WORK (work done there) or RESIDENT (taxed as the home state). */
export interface UsStateTax { state: string; kind: 'WORK' | 'RESIDENT'; percent: number; wages: number; tax: number }

/** D63: the company's details in one state. */
export interface UsStateRegistration {
  state: string;
  withholdingAccountNo: string | null;
  unemploymentAccountNo: string | null;
  sutaRate: number;
  sutaWageBase: number | null;
  stateTaxAccountId: string | null;
  sutaAccountId: string | null;
}

export interface UsStatesOverview {
  registrations: UsStateRegistration[];
  reciprocity: { workState: string; residentState: string }[];
  statesInUse: string[];
}

export interface UsRunDetail extends UsRun {
  lines: UsLine[];
  remittances: { id: string; type: UsRemitType; state?: string | null; amount: number; date: string; reference: string | null; journalEntryId: string; createdByName: string | null }[];
  taxes: { type: Exclude<UsRemitType, 'FORM_941'>; state?: string | null; total: number; remitted: number; outstanding: number }[];
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

/**
 * D64: what each state's own withholding form calls the fields of the state certificate, for the
 * states whose built-in rules use them. States not listed use the plain labels.
 */
export const STATE_FORM: Record<string, {
  form: string; allowances?: string; second?: string; override?: string; credit?: string; rate?: boolean;
  /** D65: exemption dollars field label; labels for the SINGLE / MARRIED / HEAD_OF_HOUSEHOLD choices; a hint. */
  deduction?: string; status?: [string, string, string]; hint?: string;
  /** D66: the state form's own code: field label and [value, label] choices ('' = none given). */
  code?: { label: string; options: [string, string][] };
}> = {
  AZ: { form: 'Form A-4', rate: true },
  CO: { form: 'Form DR 0004', override: 'DR 0004 line 2: annual withholding allowance $ (blank = standard)' },
  GA: { form: 'Form G-4', allowances: 'G-4 total allowances (lines 4 + 5)' },
  IA: { form: 'IA W-4', allowances: 'Allowances (only on an IA W-4 from 2023 or earlier)', credit: 'IA W-4 line 7: total allowances $ (2024 or later)' },
  IL: { form: 'IL-W-4', allowances: 'IL-W-4 line 1 allowances', second: 'IL-W-4 line 2 allowances (65+ / blind)' },
  IN: { form: 'WH-4', allowances: 'WH-4 line 5 personal exemptions', second: 'WH-4 dependents (lines 6 + 7; an adopted child counts 2)' },
  MI: { form: 'MI-W4', allowances: 'MI-W4 line 6 exemptions' },
  NC: { form: 'NC-4', allowances: 'NC-4 allowances' },
  // D65, step 2.
  DE: { form: 'DE-W4', allowances: 'DE-W4 line 4 allowances' },
  HI: { form: 'HW-4', allowances: 'HW-4 allowances' },
  KS: { form: 'K-4', allowances: 'K-4 personal allowances (lines B + C)', second: 'K-4 dependents and head of household (lines D + E)', status: ['Single rate', 'Joint rate', 'Single rate (head of household)'] },
  MN: { form: 'W-4MN', allowances: 'W-4MN allowances' },
  MS: { form: 'Form 89-350', deduction: 'Form 89-350 line 6: total exemption $', status: ['Single (also married, spouse employed)', 'Married, spouse not employed', 'Head of family'], allowances: 'Not used (enter the exemption dollars)' },
  MO: { form: 'MO W-4', status: ['Single, married with a working spouse, or married filing separately', 'Married, spouse does not work', 'Head of household'] },
  MT: { form: 'MW-4', status: ['Single, married filing separately, or both spouses working (line 2)', 'Married filing jointly', 'Head of household'] },
  NE: { form: 'W-4N', allowances: 'W-4N allowances' },
  ND: { form: 'federal W-4', allowances: 'Allowances (only on a W-4 from 2019 or earlier)' },
  OH: { form: 'IT 4', allowances: 'IT 4 exemptions' },
  OK: { form: 'OK-W-4', allowances: 'OK-W-4 allowances (line 5)' },
  RI: { form: 'RI W-4', allowances: 'RI W-4 line 1E exemptions' },
  VT: { form: 'W-4VT', allowances: 'W-4VT allowances (line 5)' },
  VA: { form: 'VA-4', allowances: 'VA-4 personal and dependent exemptions', second: 'VA-4 age 65+ / blind exemptions' },
  // D66, step 3.
  AL: { form: 'A4', allowances: 'A4 line 4 dependents', code: { label: 'A4 status', options: [['', 'From the filing status (S / M / H)'], ['0', '0 — no personal exemption'], ['S', 'S — single'], ['MS', 'MS — married filing separately'], ['M', 'M — married filing jointly'], ['H', 'H — head of family']] } },
  AR: { form: 'AR4EC', allowances: 'AR4EC line 3 total exemptions', hint: 'The low-income tables (AR4EC line 5) are not offered.' },
  CA: { form: 'DE 4', allowances: 'DE 4 line 1a regular allowances', second: 'DE 4 line 1b estimated-deduction allowances', status: ['Single, or married with two or more incomes', 'Married (one income)', 'Head of household'] },
  CT: { form: 'CT-W4', allowances: 'Not used (choose the withholding code)', hint: 'Without a CT-W4 code, 6.99% of wages is withheld.', code: { label: 'CT-W4 line 1 withholding code', options: [['', 'No CT-W4 on file (6.99%)'], ['A', 'A'], ['B', 'B'], ['C', 'C'], ['D', 'D'], ['E', 'E — no withholding'], ['F', 'F']] } },
  MD: { form: 'MW507', allowances: 'MW507 line 1 exemptions', status: ['Single, or married withholding at the single rate', 'Married (joint)', 'Head of household (joint rate)'], code: { label: 'County of residence (local tax)', options: [['', 'Not given (3.30%)'], ['ALLE', 'Allegany'], ['ANNE', 'Anne Arundel'], ['BALT', 'Baltimore County'], ['BCITY', 'Baltimore City'], ['CALV', 'Calvert'], ['CARO', 'Caroline'], ['CARR', 'Carroll'], ['CECI', 'Cecil'], ['CHAR', 'Charles'], ['DORC', 'Dorchester'], ['FRED', 'Frederick'], ['GARR', 'Garrett'], ['HARF', 'Harford'], ['HOWA', 'Howard'], ['KENT', 'Kent'], ['MONT', 'Montgomery'], ['PRIN', "Prince George's"], ['QUEE', "Queen Anne's"], ['STMA', "St. Mary's"], ['SOME', 'Somerset'], ['TALB', 'Talbot'], ['WASH', 'Washington'], ['WICO', 'Wicomico'], ['WORC', 'Worcester'], ['NR', 'Lives outside Maryland (nonresident, 2.25%)']] } },
  ME: { form: 'W-4ME', allowances: 'W-4ME line 4 allowances', status: ['Single or head of household, or married at the single rate', 'Married', 'Head of household (single rate)'] },
  NJ: { form: 'NJ-W4', allowances: 'NJ-W4 line 4 allowances', code: { label: 'NJ-W4 line 3 rate table', options: [['', 'From the filing status (A single / B joint, head of household)'], ['A', 'Rate A'], ['B', 'Rate B'], ['C', 'Rate C'], ['D', 'Rate D'], ['E', 'Rate E']] } },
  NY: { form: 'IT-2104', allowances: 'IT-2104 line 1 allowances', status: ['Single or head of household, or married at the higher single rate', 'Married', 'Head of household (single rate)'], hint: 'New York City and Yonkers taxes are not included.' },
  WI: { form: 'WT-4', allowances: 'WT-4 line 1(d) exemptions', status: ['Single, or married at the higher single rate', 'Married', 'Head of household (single rate)'] },
  WV: { form: 'IT-104', allowances: 'IT-104 exemptions (line 4)', status: ['Two-earner table (default)', 'One-earner table (IT-104 line 5 box checked)', 'Two-earner table'] },
};

export const ARIZONA_RATES = [0, 0.5, 1, 1.5, 2, 2.5, 3, 3.5];

/** Dollars with the sign, for the US screens (whatever the company's home currency). */
export function usd(v: number | string | null | undefined): string {
  const x = n(v);
  return x < 0 ? `-$${amt(-x)}` : `$${amt(x)}`;
}

/**
 * D57: payroll follows the company's country — Sri Lanka uses EPF / ETF / PAYE, the United States
 * US payroll. Called after the country is saved; answers a message when payroll could not be
 * switched (e.g. a US payroll account is already used for something else), otherwise null.
 */
export async function syncPayrollCountry(country: string | undefined): Promise<string | null> {
  if (country !== 'LK' && country !== 'US') return null;
  try {
    const { data } = await api.get<UsSettings>('/payroll/us/settings');
    if (data.country === country) return null;
    await api.patch('/payroll/us/settings', { country });
    return null;
  } catch (err) {
    const name = country === 'US' ? 'United States' : 'Sri Lanka';
    return `The company was saved, but payroll could not switch to ${name}: ${errMsg(err, 'unknown error')}`;
  }
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
