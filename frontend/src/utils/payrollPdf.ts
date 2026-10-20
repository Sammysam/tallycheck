// Payroll documents as PDFs: salary slips, bank advice, EPF C Form, ETF
// remittance advice and the acknowledgement sheet. Same mechanics as the
// invoice PDF: HTML rendered off-screen, captured per page with html2canvas,
// placed into an A4 jsPDF document.
import jsPDF from 'jspdf';
import html2canvas from 'html2canvas';
import { amountInWords } from './amountInWords';
import { periodLabel, type CompanyDetails, type PayrollLine, type RunDetail } from '../lib/payroll';
import { amount } from '../lib/money';
import { company as companyDetails, region as companyRegion } from '../lib/region';

const companyName = () => companyDetails().name.toUpperCase();

const esc = (s?: string | null) =>
  String(s ?? '').replace(/[&<>"]/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' }[c] as string));
const num = (v: number | string | null | undefined) =>
  amount(v ?? 0);
const n = (v: number | string | null | undefined) => Number(v) || 0;
const fmtDate = (d?: string | null) => {
  if (!d) return '';
  const date = new Date(d);
  return isNaN(date.getTime()) ? d : date.toLocaleDateString(companyRegion().locale, { day: '2-digit', month: 'short', year: 'numeric' });
};
const today = () => fmtDate(new Date().toISOString().slice(0, 10));

const BASE_CSS = `
  * { box-sizing: border-box; }
  .pg { width: 794px; min-height: 1123px; padding: 36px 40px; background: #fff; color: #111; font: 12px/1.45 Arial, Helvetica, sans-serif; position: relative; }
  .pg.land { width: 1123px; min-height: 794px; }
  h1 { font-size: 16px; margin: 0 0 2px; text-align: center; letter-spacing: .5px; }
  h2 { font-size: 13px; margin: 0 0 14px; text-align: center; font-weight: normal; }
  table { border-collapse: collapse; width: 100%; }
  th, td { padding: 4px 6px; vertical-align: top; }
  .grid th, .grid td { border: 1px solid #333; }
  .grid th { background: #eee; font-weight: bold; text-align: left; }
  .r { text-align: right; } .c { text-align: center; } .b { font-weight: bold; }
  .muted { color: #555; } .small { font-size: 11px; }
  .draft { position: absolute; top: 40%; left: 0; right: 0; text-align: center; font-size: 90px; color: rgba(220,38,38,.12); transform: rotate(-25deg); font-weight: bold; pointer-events: none; }
  .sig { margin-top: 40px; display: flex; justify-content: space-between; }
  .sig div { width: 42%; border-top: 1px solid #333; padding-top: 4px; font-size: 11px; }
`;

// ─── Salary slip ─────────────────────────────────────────────────────────────

const SLIP_CSS = `
  .slips { display: flex; gap: 24px; }
  .slip { width: 50%; border: 1px solid #333; padding: 12px 14px; font-size: 11.5px; }
  .slip h1 { font-size: 13px; }
  .slip h2 { font-size: 11.5px; margin-bottom: 8px; }
  .slip .who td { padding: 1px 0; }
  .slip .sec { background: #eee; font-weight: bold; padding: 3px 6px; margin: 8px 0 2px; border: 1px solid #333; }
  .slip .ln td { padding: 2px 6px; border-bottom: 1px dotted #bbb; }
  .slip .ln td.r { width: 110px; }
  .slip .tot td { padding: 4px 6px; border-top: 1px solid #333; border-bottom: 2px double #333; font-weight: bold; }
  .slip .words { font-size: 10.5px; font-style: italic; margin-top: 6px; }
  .slip .hr { margin-top: 36px; border-top: 1px solid #333; width: 50%; padding-top: 3px; font-size: 10.5px; }
`;

function slipMarkup(run: RunDetail, l: PayrollLine, company: CompanyDetails): string {
  const row = (label: string, v: number | string | null | undefined, always = false) =>
    always || n(v) ? `<tr class="ln"><td>${esc(label)}</td><td class="r">${num(v)}</td></tr>` : '';
  const month = periodLabel(run.period);
  return `
  <div class="slip">
    <h1>${esc(company.companyName || companyName())}</h1>
    <h2>Salary for the Month of ${esc(month)}</h2>
    <table class="who">
      <tr><td><b>Name:</b> ${esc(l.employeeName)}</td><td class="r"><b>Emp No:</b> ${esc(l.employeeCode)}</td></tr>
      <tr><td class="muted small">${esc(l.jobTitle || '')}${l.department ? ' · ' + esc(l.department) : ''}</td><td class="r muted small">${l.epfNumber ? 'EPF No: ' + esc(l.epfNumber) : ''}</td></tr>
    </table>
    <div class="sec">EARNINGS <span style="float:right">${esc(companyRegion().currencySymbol)}</span></div>
    <table>
      ${row('Basic Salary', l.basicForMonth, true)}
      <tr class="ln"><td>No. of days worked</td><td class="r">${n(l.daysWorked)} / ${l.daysInMonth}</td></tr>
      ${row('BRA - 2005', l.bra2005)}
      ${row('BRA I & II - 2016', l.bra2016)}
      ${row('Vehicle Allowance', l.vehicleAllowance)}
      ${row('Other Allowance', l.otherAllowance)}
      ${row('Project Allowance / Bonus', l.projectBonus)}
      ${row('Arrears', l.arrears)}
      ${row('Medical', l.medical)}
      ${row('Other Earnings', l.otherEarnings)}
      ${row('No Pay', n(l.noPayAmount) ? -n(l.noPayAmount) : 0)}
      <tr class="tot"><td>Gross Salary</td><td class="r">${num(l.grossSalary)}</td></tr>
    </table>
    <div class="sec">DEDUCTIONS <span style="float:right">${esc(companyRegion().currencySymbol)}</span></div>
    <table>
      ${row('EPF 8%', l.epfEmployee, l.epfApplicable)}
      ${row('Staff Welfare', l.welfare)}
      ${row('PAYE', l.paye)}
      ${row('Phone Bill', l.phoneBill)}
      ${row('Salary Advance', l.salaryAdvance)}
      ${row('Staff Loan', l.loanRecovery)}
      ${row('Other Deductions', l.otherDeductions)}
      ${row('Stamp Duty', l.stampDuty, true)}
      <tr class="tot"><td>Total Deductions</td><td class="r">${num(l.deductionsTotal)}</td></tr>
      <tr class="tot"><td>Net Salary</td><td class="r">${num(l.netSalary)}</td></tr>
    </table>
    <div class="words">${esc(amountInWords(n(l.netSalary), companyRegion().currencyWord, companyRegion().currencyMinorWord))}</div>
    ${l.epfApplicable ? `
    <div class="sec">EMPLOYER'S CONTRIBUTION <span style="float:right">${esc(companyRegion().currencySymbol)}</span></div>
    <table>
      ${row('EPF 12%', l.epfEmployer, true)}
      ${row('ETF 3%', l.etf, true)}
    </table>` : ''}
    ${l.bankAccountNo ? `<div class="muted small" style="margin-top:6px">Paid to ${esc(l.bankName || '')} ${esc(l.bankBranch || '')} · A/C ${esc(l.bankAccountNo)}${run.payDate ? ' on ' + fmtDate(run.payDate) : ''}</div>` : ''}
    ${l.notes ? `<div class="muted small">${esc(l.notes)}</div>` : ''}
    <div class="hr">${esc(company.hrSignatory || 'HR')}</div>
    <div class="muted small" style="margin-top:4px">${esc(l.slipNumber)}</div>
  </div>`;
}

/** Two slips side by side per A4 page, as the company's workbook prints them. */
export function slipPages(run: RunDetail, company: CompanyDetails, lines = run.lines): string[] {
  const pages: string[] = [];
  for (let i = 0; i < lines.length; i += 2) {
    const pair = lines.slice(i, i + 2).map((l) => slipMarkup(run, l, company)).join('');
    pages.push(`<div class="pg">${run.status === 'DRAFT' ? '<div class="draft">DRAFT</div>' : ''}<div class="slips">${pair}</div></div>`);
  }
  return pages;
}

// ─── Bank advice ─────────────────────────────────────────────────────────────

export function bankAdvicePage(run: RunDetail, company: CompanyDetails): string {
  const groups = new Map<string, PayrollLine[]>();
  for (const l of run.lines) {
    const key = l.bankAccountNo ? (l.bankName || 'Other banks') : 'No bank details (cash / cheque)';
    groups.set(key, [...(groups.get(key) ?? []), l]);
  }
  let body = '';
  for (const [bank, lines] of groups) {
    const total = lines.reduce((s, l) => s + n(l.netSalary), 0);
    body += `
      <tr><th colspan="5" style="background:#ddd">${esc(bank)}</th></tr>
      ${lines.map((l, i) => `
      <tr>
        <td class="c">${i + 1}</td>
        <td>${esc(l.employeeName)}<div class="muted small">${esc(l.employeeCode)}</div></td>
        <td>${esc(l.bankAccountNo || '—')}</td>
        <td>${esc(l.bankBranch || '—')}</td>
        <td class="r">${num(l.netSalary)}</td>
      </tr>`).join('')}
      <tr><td colspan="4" class="r b">Sub-total — ${esc(bank)}</td><td class="r b">${num(total)}</td></tr>`;
  }
  return `
  <div class="pg">
    ${run.status === 'DRAFT' ? '<div class="draft">DRAFT</div>' : ''}
    <h1>${esc(company.companyName || companyName())}</h1>
    <h2>Bank Advice — Salaries for ${esc(periodLabel(run.period))}</h2>
    <table class="small" style="margin-bottom:10px"><tr><td>Date: ${esc(run.payDate ? fmtDate(run.payDate) : today())}</td><td class="r">Ref: ${esc(run.runNumber)}</td></tr></table>
    <p class="small">Please transfer the following amounts to the accounts listed below.</p>
    <table class="grid">
      <tr><th style="width:36px">#</th><th>Name</th><th>Account No</th><th>Branch</th><th class="r" style="width:120px">Amount (${esc(companyRegion().currencySymbol)})</th></tr>
      ${body}
      <tr><td colspan="4" class="r b">TOTAL</td><td class="r b">${num(run.netTotal)}</td></tr>
    </table>
    <p class="small" style="margin-top:10px"><i>${esc(amountInWords(n(run.netTotal), companyRegion().currencyWord, companyRegion().currencyMinorWord))}</i></p>
    <div class="sig"><div>Prepared by</div><div>Authorised signatory</div></div>
  </div>`;
}

// ─── EPF C Form ──────────────────────────────────────────────────────────────

export function epfFormPage(run: RunDetail, company: CompanyDetails): string {
  const lines = run.lines.filter((l) => l.epfApplicable && n(l.epfBase) > 0);
  const sum = (k: keyof PayrollLine) => lines.reduce((s, l) => s + n(l[k] as number), 0);
  const total = sum('epfEmployee') + sum('epfEmployer');
  return `
  <div class="pg">
    ${run.status === 'DRAFT' ? '<div class="draft">DRAFT</div>' : ''}
    <h1>C FORM — EPF Act No. 15 of 1958</h1>
    <h2>Employees' Provident Fund — Monthly Contribution Return</h2>
    <table class="small" style="margin-bottom:12px">
      <tr><td style="width:60%"><b>${esc(company.companyName || '')}</b><br>${esc(company.careOf || '')}<br>${esc(company.epfAddress || '')}</td>
          <td><table class="small">
            <tr><td>E.P.F. Reg. No.</td><td class="b">${esc(company.epfRegNo || '')}</td></tr>
            <tr><td>Month</td><td class="b">${esc(periodLabel(run.period))}</td></tr>
            <tr><td>Contribution</td><td class="r b">${num(total)}</td></tr>
            <tr><td>Surcharges</td><td class="r">0.00</td></tr>
            <tr><td>Total Remittance</td><td class="r b">${num(total)}</td></tr>
          </table></td></tr>
    </table>
    <p class="small">Cheque No: ______________ &nbsp;&nbsp; Bank / Branch: ____________________________</p>
    <table class="grid small">
      <tr><th style="width:30px">#</th><th>Employee's Name</th><th>National ID No.</th><th>Memb. No.</th><th class="r">Total (Rs.)</th><th class="r">Employer 12%</th><th class="r">Employee 8%</th><th class="r">Total Earnings</th></tr>
      ${lines.map((l, i) => `
      <tr><td class="c">${i + 1}</td><td>${esc(l.employeeName)}</td><td>${esc(l.nationalId || '')}</td><td>${esc(l.epfNumber || '')}</td>
          <td class="r">${num(n(l.epfEmployee) + n(l.epfEmployer))}</td><td class="r">${num(l.epfEmployer)}</td><td class="r">${num(l.epfEmployee)}</td><td class="r">${num(l.epfBase)}</td></tr>`).join('')}
      <tr class="b"><td colspan="4" class="r">Total</td><td class="r">${num(total)}</td><td class="r">${num(sum('epfEmployer'))}</td><td class="r">${num(sum('epfEmployee'))}</td><td class="r">${num(sum('epfBase'))}</td></tr>
    </table>
    <p class="small" style="margin-top:14px">No. of employees: ${lines.length}. I certify that the information given above is correct.</p>
    <div class="sig"><div>Signature of Employer<br><span class="muted">Telephone: ${esc(company.telephone || '')}</span></div><div>Date</div></div>
    <p class="muted small" style="margin-top:16px">This form should be returned duly completed along with the contributions to the Superintendent / EPF.</p>
  </div>`;
}

// ─── ETF Form (R4) ───────────────────────────────────────────────────────────

export function etfFormPage(run: RunDetail, company: CompanyDetails): string {
  const lines = run.lines.filter((l) => l.epfApplicable && n(l.etf) > 0);
  const total = lines.reduce((s, l) => s + n(l.etf), 0);
  return `
  <div class="pg">
    ${run.status === 'DRAFT' ? '<div class="draft">DRAFT</div>' : ''}
    <h1>Employees' Trust Fund Board — Advice of Remittance (R 4)</h1>
    <h2>&nbsp;</h2>
    <table class="small" style="margin-bottom:12px">
      <tr><td style="width:60%"><b>${esc(company.companyName || '')}</b><br>${esc(company.careOf || '')}<br>${esc(company.etfAddress || '')}</td>
          <td><table class="small">
            <tr><td>E.T.F. Reg. No.</td><td class="b">${esc(company.etfRegNo || '')}</td></tr>
            <tr><td>Month of Contribution</td><td class="b">${esc(periodLabel(run.period))}</td></tr>
            <tr><td>No. of employees</td><td class="b">${lines.length}</td></tr>
            <tr><td>Contributions</td><td class="r b">${num(total)}</td></tr>
            <tr><td>Surcharges</td><td class="r">0.00</td></tr>
            <tr><td>Total Remittance</td><td class="r b">${num(total)}</td></tr>
          </table></td></tr>
    </table>
    <p class="small">Cheque No: ______________ &nbsp;&nbsp; Bank and Branch Name: ____________________________</p>
    <table class="grid small">
      <tr><th style="width:30px">#</th><th>Employee's Name</th><th>National ID No.</th><th>Memb. No.</th><th class="r">Contribution 3% (Rs.)</th></tr>
      ${lines.map((l, i) => `
      <tr><td class="c">${i + 1}</td><td>${esc(l.employeeName)}</td><td>${esc(l.nationalId || '')}</td><td>${esc(l.epfNumber || '')}</td><td class="r">${num(l.etf)}</td></tr>`).join('')}
      <tr class="b"><td colspan="4" class="r">TOTAL</td><td class="r">${num(total)}</td></tr>
    </table>
    <p class="small" style="margin-top:14px">I certify that the information given above is correct.</p>
    <div class="sig"><div>Signature of Employer<br><span class="muted">Telephone: ${esc(company.telephone || '')}</span></div><div>Date</div></div>
  </div>`;
}

// ─── Acknowledgement sheet ───────────────────────────────────────────────────

export function acknowledgementPage(run: RunDetail, company: CompanyDetails): string {
  const half = Math.ceil(run.lines.length / 2);
  const left = run.lines.slice(0, half);
  const right = run.lines.slice(half);
  const rows = Array.from({ length: half }, (_, i) => `
    <tr>
      <td class="c">${i + 1}</td><td>${esc(left[i]?.employeeName || '')}</td><td style="height:34px"></td>
      <td class="c">${right[i] ? half + i + 1 : ''}</td><td>${esc(right[i]?.employeeName || '')}</td><td></td>
    </tr>`).join('');
  return `
  <div class="pg">
    <h1>${esc(company.companyName || companyName())}</h1>
    <h2>Salary &amp; Pay Slip Acknowledgement for the Month of ${esc(periodLabel(run.period))}</h2>
    <table class="grid">
      <tr><th style="width:30px">#</th><th>Name</th><th style="width:28%">Signature</th><th style="width:30px">#</th><th>Name</th><th style="width:28%">Signature</th></tr>
      ${rows}
    </table>
    <p class="muted small" style="margin-top:10px">I acknowledge receipt of my salary and salary slip for the month stated above.</p>
  </div>`;
}

// ─── Rendering ───────────────────────────────────────────────────────────────

export type PdfOutput = 'download' | 'blob' | 'open';

/**
 * Renders each page's markup off-screen, captures it, and places one image
 * per PDF page. Returns the blob (always) and downloads/opens it if asked.
 */
export async function renderPdf(pages: string[], filename: string, output: PdfOutput = 'download'): Promise<Blob> {
  const root = document.createElement('div');
  root.style.position = 'fixed';
  root.style.left = '-99999px';
  root.style.top = '0';
  root.innerHTML = `<style>${BASE_CSS}${SLIP_CSS}</style>` + pages.join('');
  document.body.appendChild(root);

  try {
    const pdf = new jsPDF({ unit: 'mm', format: 'a4', orientation: 'portrait' });
    const pageEls = Array.from(root.querySelectorAll<HTMLElement>('.pg'));
    for (let i = 0; i < pageEls.length; i++) {
      const canvas = await html2canvas(pageEls[i], { scale: 2, useCORS: true, backgroundColor: '#ffffff' });
      const img = canvas.toDataURL('image/jpeg', 0.92);
      const w = 210;
      const h = Math.min(297, (canvas.height * w) / canvas.width);
      if (i > 0) pdf.addPage();
      pdf.addImage(img, 'JPEG', 0, 0, w, h);
    }
    const blob = pdf.output('blob');
    const safe = filename.replace(/[\\/:*?"<>|]+/g, '-');
    if (output === 'download') pdf.save(`${safe}.pdf`);
    if (output === 'open') {
      const url = URL.createObjectURL(blob);
      window.open(url, '_blank', 'noopener');
      setTimeout(() => URL.revokeObjectURL(url), 60_000);
    }
    return blob;
  } finally {
    document.body.removeChild(root);
  }
}
