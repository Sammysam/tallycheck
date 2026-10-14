export type PayrollRunStatus = 'DRAFT' | 'POSTED' | 'PAID' | 'VOID';
export type RemittanceType = 'EPF' | 'ETF' | 'PAYE' | 'STAMP_DUTY' | 'WELFARE';

export type PayeSlab = [number | null, number];

export interface PayrollRates {
  epfEmployeeRate: number;
  epfEmployerRate: number;
  etfRate: number;
  stampDuty: number;
  employerWelfare: number;
  daysInMonth: number;
  payeSlabs: PayeSlab[];
}

export interface PayrollRun {
  id: string;
  runNumber: string;
  period: string;
  periodStart: string;
  periodEnd: string;
  status: PayrollRunStatus;
  notes: string | null;
  ratesSnapshot: PayrollRates;
  employeeCount: number;
  earningsTotal: number;
  noPayTotal: number;
  grossTotal: number;
  epfEmployeeTotal: number;
  payeTotal: number;
  stampDutyTotal: number;
  welfareTotal: number;
  loanRecoveryTotal: number;
  salaryAdvanceTotal: number;
  otherDeductionsTotal: number;
  deductionsTotal: number;
  netTotal: number;
  epfEmployerTotal: number;
  etfTotal: number;
  employerWelfareTotal: number;
  costToCompanyTotal: number;
  journalEntryId: string | null;
  paymentJournalEntryId: string | null;
  paymentAccountId: string | null;
  payDate: string | null;
  postedAt: string | null;
  paidAt: string | null;
  slipsFiledAt: string | null;
  createdByName: string | null;
  createdAt: string;
}

/** Printed on the EPF C Form, ETF form and the slips. Editable on the Payroll page. */
export interface CompanyDetails {
  companyName: string | null;
  epfRegNo: string | null;
  etfRegNo: string | null;
  careOf: string | null;
  epfAddress: string | null;
  etfAddress: string | null;
  telephone: string | null;
  hrSignatory: string | null;
}

export interface PayrollLine {
  id: string;
  employeeId: string | null;
  slipNumber: string;
  employeeCode: string;
  employeeName: string;
  jobTitle: string | null;
  department: string | null;
  employmentType: string;
  epfNumber: string | null;
  nationalId: string | null;
  bankName: string | null;
  bankBranch: string | null;
  bankAccountNo: string | null;
  epfApplicable: boolean;
  daysInMonth: number;
  daysWorked: number;
  basic: number;
  basicForMonth: number;
  bra2005: number;
  bra2016: number;
  vehicleAllowance: number;
  otherAllowance: number;
  projectBonus: number;
  arrears: number;
  medical: number;
  otherEarnings: number;
  earningsTotal: number;
  noPayAmount: number;
  grossSalary: number;
  epfBase: number;
  epfEmployee: number;
  payeOverride: number | null;
  paye: number;
  stampDuty: number;
  welfare: number;
  loanRecovery: number;
  salaryAdvance: number;
  phoneBill: number;
  otherDeductions: number;
  deductionsTotal: number;
  netSalary: number;
  epfEmployer: number;
  etf: number;
  employerWelfare: number;
  costToCompany: number;
  notes: string | null;
}

export interface Remittance {
  id: string;
  type: RemittanceType;
  amount: number;
  date: string;
  reference: string | null;
  createdByName: string | null;
}

export interface RunDetail extends PayrollRun {
  lines: PayrollLine[];
  remittances: Remittance[];
  owed: Record<RemittanceType, { total: number; remitted: number; outstanding: number }>;
  skipped?: { employeeId: string; employeeCode: string; name: string; reason: string }[];
}

export const REMIT_LABEL: Record<RemittanceType, string> = {
  EPF: 'EPF (8% + 12%)',
  ETF: 'ETF (3%)',
  PAYE: 'PAYE',
  STAMP_DUTY: 'Stamp duty',
  WELFARE: 'Staff welfare fund',
};

export const RUN_STATUS_HELP: Record<PayrollRunStatus, string> = {
  DRAFT: 'Editable. Nothing has reached the ledger.',
  POSTED: 'Booked to the ledger as expense and payables. Pay salaries next.',
  PAID: 'Salaries paid from the bank. Remit EPF, ETF and PAYE when due.',
  VOID: 'Reversed. The month can be run again.',
};

/** The line fields an admin may type in a DRAFT run. */
export const EDITABLE_LINE_FIELDS = [
  'daysWorked',
  'projectBonus',
  'arrears',
  'medical',
  'otherEarnings',
  'noPayAmount',
  'welfare',
  'loanRecovery',
  'salaryAdvance',
  'phoneBill',
  'otherDeductions',
  'payeOverride',
] as const;
export type EditableLineField = (typeof EDITABLE_LINE_FIELDS)[number];

export function periodLabel(period: string): string {
  const [y, m] = period.split('-').map(Number);
  return new Date(Date.UTC(y, m - 1, 1)).toLocaleDateString(undefined, { month: 'long', year: 'numeric', timeZone: 'UTC' });
}

export function n(v: number | string | null | undefined): number {
  return Number(v ?? 0);
}

/** Rupees without the prefix, for dense tables. */
export function amt(v: number | string | null | undefined): string {
  return n(v).toLocaleString('en-LK', { minimumFractionDigits: 2, maximumFractionDigits: 2 });
}
