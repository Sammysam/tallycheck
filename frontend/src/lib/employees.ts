import { money } from './money';

export type EmployeeStatus = 'ACTIVE' | 'ON_LEAVE' | 'LEFT';
export type EmploymentType = 'PERMANENT' | 'CONTRACT';

export interface Employee {
  id: string;
  employeeId: string;
  name: string;
  jobTitle: string | null;
  department: string | null;
  email: string | null;
  phone: string | null;
  joinDate: string | null;
  status: EmployeeStatus;
  notes: string | null;
  photoKey: string | null;
  employmentType: EmploymentType;
  epfNumber: string | null;
  nationalId: string | null;
  resignedDate: string | null;
}

export const EMPLOYMENT_TYPE_LABEL: Record<EmploymentType, string> = {
  PERMANENT: 'Permanent staff',
  CONTRACT: 'Contract labour (no EPF/ETF)',
};

// ─── Payroll ────────────────────────────────────────────────────────────────

export interface SalaryRecord {
  id: string;
  effectiveFrom: string;
  basic: number;
  bra2005: number;
  bra2016: number;
  vehicleAllowance: number;
  otherAllowance: number;
  epfApplicable: boolean;
  welfareDeduction: number;
  loanInstallment: number;
  reason: string | null;
  notes: string | null;
  createdByName: string | null;
  createdAt: string;
  monthlyTotal: number;
  epfBase: number;
  changeAmount: number | null;
  changePercent: number | null;
  state: 'CURRENT' | 'PAST' | 'UPCOMING';
}

export interface EmployeePayroll {
  employeeId: string;
  bank: { bankName: string | null; bankBranch: string | null; bankAccountNo: string | null };
  staffLoanBalance: number;
  current: SalaryRecord | null;
  upcoming: SalaryRecord | null;
  history: SalaryRecord[];
}

export const emptySalaryForm = {
  effectiveFrom: '',
  basic: '',
  bra2005: '',
  bra2016: '',
  vehicleAllowance: '',
  otherAllowance: '',
  epfApplicable: true,
  welfareDeduction: '300',
  loanInstallment: '',
  reason: '',
  notes: '',
};
export type SalaryForm = typeof emptySalaryForm;

const num = (s: string) => (s.trim() === '' ? 0 : Number(s));

export function toSalaryBody(f: SalaryForm) {
  return {
    effectiveFrom: f.effectiveFrom,
    basic: num(f.basic),
    bra2005: num(f.bra2005),
    bra2016: num(f.bra2016),
    vehicleAllowance: num(f.vehicleAllowance),
    otherAllowance: num(f.otherAllowance),
    epfApplicable: f.epfApplicable,
    welfareDeduction: num(f.welfareDeduction),
    loanInstallment: num(f.loanInstallment),
    reason: f.reason.trim() || null,
    notes: f.notes.trim() || null,
  };
}

export function salaryToForm(s: SalaryRecord): SalaryForm {
  const str = (n: number) => (n ? String(n) : '');
  return {
    effectiveFrom: s.effectiveFrom,
    basic: str(s.basic),
    bra2005: str(s.bra2005),
    bra2016: str(s.bra2016),
    vehicleAllowance: str(s.vehicleAllowance),
    otherAllowance: str(s.otherAllowance),
    epfApplicable: s.epfApplicable,
    welfareDeduction: String(s.welfareDeduction),
    loanInstallment: str(s.loanInstallment),
    reason: s.reason ?? '',
    notes: s.notes ?? '',
  };
}

export function rs(n: number | null | undefined): string {
  if (n === null || n === undefined) return '—';
  return money(Number(n));
}

export type DocumentCategory = 'CERTIFICATE' | 'ID' | 'CONTRACT' | 'VISA' | 'PAYSLIP' | 'OTHER';

export interface EmployeeDocument {
  id: string;
  category: DocumentCategory;
  title: string;
  originalName: string;
  mimeType: string;
  size: number;
  expiryDate: string | null;
  notes: string | null;
  isCurrent: boolean;
  replacesId: string | null;
  uploadedByName: string | null;
  createdAt: string;
}

export const STATUS_LABEL: Record<EmployeeStatus, string> = {
  ACTIVE: 'Active',
  ON_LEAVE: 'On leave',
  LEFT: 'Left',
};

export const CATEGORY_LABEL: Record<DocumentCategory, string> = {
  CERTIFICATE: 'Certificate',
  ID: 'ID copy',
  CONTRACT: 'Contract',
  VISA: 'Visa / permit',
  PAYSLIP: 'Salary slip',
  OTHER: 'Other',
};

export const emptyEmployeeForm = {
  employeeId: '',
  name: '',
  jobTitle: '',
  department: '',
  email: '',
  phone: '',
  joinDate: '',
  status: 'ACTIVE' as EmployeeStatus,
  notes: '',
  employmentType: 'PERMANENT' as EmploymentType,
  epfNumber: '',
  nationalId: '',
  resignedDate: '',
};

export type EmployeeForm = typeof emptyEmployeeForm;

export function toEmployeeForm(e: Employee): EmployeeForm {
  return {
    employeeId: e.employeeId,
    name: e.name,
    jobTitle: e.jobTitle ?? '',
    department: e.department ?? '',
    email: e.email ?? '',
    phone: e.phone ?? '',
    joinDate: e.joinDate ?? '',
    status: e.status,
    notes: e.notes ?? '',
    employmentType: e.employmentType ?? 'PERMANENT',
    epfNumber: e.epfNumber ?? '',
    nationalId: e.nationalId ?? '',
    resignedDate: e.resignedDate ?? '',
  };
}

/** Turns the form into the API body: blanks become null so they clear on edit. */
export function toEmployeeBody(form: EmployeeForm) {
  return {
    employeeId: form.employeeId.trim(),
    name: form.name.trim(),
    jobTitle: form.jobTitle.trim() || null,
    department: form.department.trim() || null,
    email: form.email.trim() || null,
    phone: form.phone.trim() || null,
    joinDate: form.joinDate || null,
    status: form.status,
    notes: form.notes.trim() || null,
    employmentType: form.employmentType,
    epfNumber: form.epfNumber.trim() || null,
    nationalId: form.nationalId.trim() || null,
    resignedDate: form.resignedDate || null,
  };
}

/** Up to two initials from a full name. */
export function initials(name: string): string {
  return name
    .split(/\s+/)
    .filter(Boolean)
    .slice(0, 2)
    .map((p) => p[0]?.toUpperCase() ?? '')
    .join('');
}

export function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(0)} KB`;
  return `${(n / (1024 * 1024)).toFixed(1)} MB`;
}

export function formatDate(iso: string | null): string {
  if (!iso) return '—';
  return new Date(iso).toLocaleDateString(undefined, { year: 'numeric', month: 'short', day: 'numeric' });
}

/** Expiry status: null when there is no date, else how it should be flagged. */
export function expiryState(date: string | null): { label: string; cls: string } | null {
  if (!date) return null;
  const days = Math.floor((new Date(date).getTime() - Date.now()) / 86_400_000);
  if (days < 0) return { label: `Expired ${formatDate(date)}`, cls: 'bg-red-100 text-red-700' };
  if (days <= 30) return { label: `Expires in ${days}d · ${formatDate(date)}`, cls: 'bg-amber-100 text-amber-700' };
  return { label: formatDate(date), cls: 'bg-gray-100 text-gray-600' };
}
