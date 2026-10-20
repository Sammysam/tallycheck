import { useCallback, useEffect, useState } from 'react';
import { useLocation, useNavigate, useParams } from 'react-router-dom';
import api, { errMsg } from '../api/client';
import PageHeader from '../components/PageHeader';
import StatusBadge from '../components/StatusBadge';
import { rs } from '../lib/employees';
import {
  REMIT_LABEL,
  RUN_STATUS_HELP,
  amt,
  n,
  periodLabel,
  type CompanyDetails,
  type EditableLineField,
  type PayrollLine,
  type RemittanceType,
  type RunDetail,
} from '../lib/payroll';
import { acknowledgementPage, bankAdvicePage, epfFormPage, etfFormPage, renderPdf, slipPages } from '../utils/payrollPdf';
import { region } from '../lib/region';

const EMPTY_COMPANY: CompanyDetails = {
  companyName: null, epfRegNo: null, etfRegNo: null, careOf: null,
  epfAddress: null, etfAddress: null, telephone: null, hrSignatory: null,
};

interface Account { id: string; code: string; name: string; type: string }
interface Employee { id: string; employeeId: string; name: string }

const today = () => new Date().toISOString().slice(0, 10);

export default function PayrollRun() {
  const { id = '' } = useParams();
  const navigate = useNavigate();
  const location = useLocation();
  const initialSkipped = (location.state as RunDetail | null)?.skipped ?? [];

  const [run, setRun] = useState<RunDetail | null>(null);
  const [error, setError] = useState('');
  const [skipped, setSkipped] = useState(initialSkipped);
  const [accounts, setAccounts] = useState<Account[]>([]);
  const [employees, setEmployees] = useState<Employee[]>([]);
  const [company, setCompany] = useState<CompanyDetails>(EMPTY_COMPANY);
  const [docBusy, setDocBusy] = useState('');
  const [filing, setFiling] = useState<{ done: number; total: number; errors: string[] } | null>(null);

  // Editing one line at a time keeps the table simple and the saves small.
  const [editing, setEditing] = useState<PayrollLine | null>(null);
  const [draft, setDraft] = useState<Record<string, string>>({});
  const [lineError, setLineError] = useState('');
  const [saving, setSaving] = useState(false);

  const [addEmployeeId, setAddEmployeeId] = useState('');
  const [payForm, setPayForm] = useState({ paymentAccountId: '', payDate: today() });
  const [remitForm, setRemitForm] = useState({ type: 'EPF' as RemittanceType, paymentAccountId: '', date: today(), amount: '', reference: '' });
  const [actionError, setActionError] = useState('');
  const [busy, setBusy] = useState(false);

  const load = useCallback(
    () =>
      api
        .get<RunDetail>(`/payroll/runs/${id}`)
        .then((r) => { setRun(r.data); setError(''); })
        .catch((err) => setError(errMsg(err, 'Payroll run not found'))),
    [id],
  );
  useEffect(() => { load(); }, [load]);
  useEffect(() => {
    api.get('/accounts').then((r) => setAccounts(r.data.filter((a: Account) => a.type === 'ASSET'))).catch(() => undefined);
    api.get('/employees').then((r) => setEmployees(r.data)).catch(() => undefined);
    api.get<CompanyDetails>('/payroll/company').then((r) => setCompany(r.data)).catch(() => undefined);
  }, []);

  // ─── Documents ───────────────────────────────────────────────────────────

  const makeDoc = async (kind: 'slips' | 'bank' | 'epf' | 'etf' | 'ack', open = false) => {
    if (!run) return;
    setDocBusy(kind);
    setActionError('');
    const month = periodLabel(run.period);
    try {
      const pages =
        kind === 'slips' ? slipPages(run, company)
        : kind === 'bank' ? [bankAdvicePage(run, company)]
        : kind === 'epf' ? [epfFormPage(run, company)]
        : kind === 'etf' ? [etfFormPage(run, company)]
        : [acknowledgementPage(run, company)];
      const name =
        kind === 'slips' ? `Salary slips ${month}`
        : kind === 'bank' ? `Bank advice ${month}`
        : kind === 'epf' ? `EPF C Form ${month}`
        : kind === 'etf' ? `ETF Form ${month}`
        : `Salary acknowledgement ${month}`;
      await renderPdf(pages, name, open ? 'open' : 'download');
    } catch (err) {
      setActionError(errMsg(err, 'Could not build the PDF'));
    } finally {
      setDocBusy('');
    }
  };

  const slipFor = async (l: PayrollLine) => {
    if (!run) return;
    setDocBusy(l.id);
    try {
      await renderPdf(slipPages(run, company, [l]), `Salary slip ${l.employeeCode} ${periodLabel(run.period)}`, 'open');
    } catch (err) {
      setActionError(errMsg(err, 'Could not build the slip'));
    } finally {
      setDocBusy('');
    }
  };

  /** One slip PDF per employee, uploaded into their documents under "Salary slip". */
  const fileSlips = async () => {
    if (!run) return;
    const lines = run.lines.filter((l) => l.employeeId);
    if (!lines.length) return;
    if (run.slipsFiledAt && !window.confirm('Slips were already filed for this run. File them again as new versions?')) return;
    setFiling({ done: 0, total: lines.length, errors: [] });
    const errors: string[] = [];
    const month = periodLabel(run.period);
    for (let i = 0; i < lines.length; i++) {
      const l = lines[i];
      try {
        const blob = await renderPdf(slipPages(run, company, [l]), 'slip', 'blob');
        const body = new FormData();
        body.append('file', new File([blob], `Salary slip ${month} - ${l.employeeCode}.pdf`, { type: 'application/pdf' }));
        body.append('title', `Salary slip - ${month}`);
        body.append('category', 'PAYSLIP');
        body.append('notes', `${run.runNumber} / net ${region().currencySymbol} ${amt(l.netSalary)}`);
        await api.post(`/employees/${l.employeeId}/documents`, body);
      } catch (err) {
        errors.push(`${l.employeeName}: ${errMsg(err, 'upload failed')}`);
      }
      setFiling({ done: i + 1, total: lines.length, errors: [...errors] });
    }
    if (errors.length < lines.length) {
      await api.post(`/payroll/runs/${id}/slips-filed`).catch(() => undefined);
      await load();
    }
  };

  const act = async (fn: () => Promise<unknown>, fallback: string) => {
    setBusy(true);
    setActionError('');
    try {
      await fn();
      await load();
    } catch (err) {
      setActionError(errMsg(err, fallback));
    } finally {
      setBusy(false);
    }
  };

  // ─── Line editing ────────────────────────────────────────────────────────

  const startEdit = (l: PayrollLine) => {
    setEditing(l);
    setLineError('');
    const d: Record<string, string> = {};
    for (const f of ['daysWorked', 'projectBonus', 'arrears', 'medical', 'otherEarnings', 'noPayAmount', 'welfare', 'loanRecovery', 'salaryAdvance', 'phoneBill', 'otherDeductions'] as EditableLineField[]) {
      d[f] = n(l[f]) ? String(n(l[f])) : f === 'daysWorked' ? String(n(l[f])) : '';
    }
    d.payeOverride = l.payeOverride === null ? '' : String(n(l.payeOverride));
    d.notes = l.notes ?? '';
  setDraft(d);
  };

  const saveLine = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!editing) return;
    setSaving(true);
    setLineError('');
    const num = (s: string) => (s.trim() === '' ? 0 : Number(s));
    try {
      await api.patch(`/payroll/runs/${id}/lines/${editing.id}`, {
        daysWorked: num(draft.daysWorked),
        projectBonus: num(draft.projectBonus),
        arrears: num(draft.arrears),
        medical: num(draft.medical),
        otherEarnings: num(draft.otherEarnings),
        noPayAmount: num(draft.noPayAmount),
        welfare: num(draft.welfare),
        loanRecovery: num(draft.loanRecovery),
        salaryAdvance: num(draft.salaryAdvance),
        phoneBill: num(draft.phoneBill),
        otherDeductions: num(draft.otherDeductions),
        payeOverride: draft.payeOverride.trim() === '' ? null : Number(draft.payeOverride),
        notes: draft.notes.trim() || null,
      });
      setEditing(null);
      await load();
    } catch (err) {
      setLineError(errMsg(err, 'Could not save the line'));
    } finally {
      setSaving(false);
    }
  };

  // ─── Render ──────────────────────────────────────────────────────────────

  if (error) {
    return (
      <div>
        <PageHeader title="Payroll run" />
        <div className="card text-sm text-red-600">{error}</div>
        <button onClick={() => navigate('/payroll')} className="btn-link mt-3">← Back to payroll</button>
      </div>
    );
  }
  if (!run) {
    return <div className="flex items-center justify-center py-20"><div className="w-6 h-6 border-2 border-blue-600 border-t-transparent rounded-full animate-spin" /></div>;
  }

  const isDraft = run.status === 'DRAFT';
  const inRun = new Set(run.lines.map((l) => l.employeeId));
  const addable = employees.filter((e) => !inRun.has(e.id));
  const accountName = (aid: string | null) => accounts.find((a) => a.id === aid)?.name ?? '';

  return (
    <div>
      <div className="mb-3"><button onClick={() => navigate('/payroll')} className="btn-link text-xs">← All payroll runs</button></div>

      <PageHeader
        title={`Payroll — ${periodLabel(run.period)}`}
        action={
          <>
            <StatusBadge status={run.status} />
            {isDraft && (
              <>
                <button className="btn btn-primary" disabled={busy || run.lines.length === 0}
                  onClick={() => window.confirm(`Post ${periodLabel(run.period)} payroll to the ledger? Lines can no longer be edited afterwards.`) && act(() => api.post(`/payroll/runs/${id}/post`), 'Could not post the run')}>
                  Post to ledger
                </button>
                <button className="btn btn-danger" disabled={busy}
                  onClick={() => window.confirm('Delete this draft run?') && act(async () => { await api.delete(`/payroll/runs/${id}`); navigate('/payroll'); }, 'Could not delete the run')}>
                  Delete draft
                </button>
              </>
            )}
            {(run.status === 'POSTED' || run.status === 'PAID') && (
              <button className="btn btn-danger" disabled={busy}
                onClick={() => window.confirm('Void this run? Every ledger entry it created (posting, salary payment, remittances) will be reversed and loan balances restored.') && act(() => api.post(`/payroll/runs/${id}/void`), 'Could not void the run')}>
                Void
              </button>
            )}
          </>
        }
      />

      <p className="text-xs text-gray-500 mb-4">
        {run.runNumber} · {RUN_STATUS_HELP[run.status]}
        {run.postedAt && ` · Posted ${new Date(run.postedAt).toLocaleDateString()}`}
        {run.payDate && ` · Paid ${run.payDate} from ${accountName(run.paymentAccountId)}`}
      </p>

      {actionError && <div className="card mb-4 text-sm text-red-600">{actionError}</div>}

      {skipped.length > 0 && (
        <div className="card mb-4 bg-amber-50 border-amber-200">
          <div className="flex items-start justify-between gap-4">
            <div className="text-sm">
              <div className="font-medium text-amber-800 mb-1">Not included in this run</div>
              <ul className="text-amber-800 space-y-0.5">
                {skipped.map((s) => <li key={s.employeeId}>{s.name} ({s.employeeCode}) — {s.reason}</li>)}
              </ul>
            </div>
            <button className="btn-link text-xs" onClick={() => setSkipped([])}>Dismiss</button>
          </div>
        </div>
      )}

      {/* ── Documents ── */}
      <div className="card mb-5">
        <div className="flex flex-wrap items-center gap-2">
          <span className="text-xs font-semibold text-gray-500 uppercase tracking-wide mr-2">Documents</span>
          <button className="btn btn-secondary" disabled={!!docBusy || run.lines.length === 0} onClick={() => makeDoc('slips')}>{docBusy === 'slips' ? 'Building…' : 'Salary slips (PDF)'}</button>
          <button className="btn btn-secondary" disabled={!!docBusy || run.lines.length === 0} onClick={() => makeDoc('bank')}>{docBusy === 'bank' ? 'Building…' : 'Bank advice'}</button>
          <button className="btn btn-secondary" disabled={!!docBusy || run.lines.length === 0} onClick={() => makeDoc('epf')}>{docBusy === 'epf' ? 'Building…' : 'EPF C Form'}</button>
          <button className="btn btn-secondary" disabled={!!docBusy || run.lines.length === 0} onClick={() => makeDoc('etf')}>{docBusy === 'etf' ? 'Building…' : 'ETF Form'}</button>
          <button className="btn btn-secondary" disabled={!!docBusy || run.lines.length === 0} onClick={() => makeDoc('ack')}>{docBusy === 'ack' ? 'Building…' : 'Acknowledgement sheet'}</button>
          {(run.status === 'POSTED' || run.status === 'PAID') && (
            <button className="btn btn-primary ml-auto" disabled={!!filing && filing.done < filing.total} onClick={fileSlips}>
              {filing && filing.done < filing.total ? `Filing ${filing.done}/${filing.total}…` : run.slipsFiledAt ? 'Re-file slips to employee records' : 'File slips to employee records'}
            </button>
          )}
        </div>
        <p className="text-xs text-gray-500 mt-2">
          {run.status === 'DRAFT'
            ? 'Documents from a draft carry a DRAFT watermark. Post the run for the final versions.'
            : run.slipsFiledAt
              ? `Slips filed into each employee's documents on ${new Date(run.slipsFiledAt).toLocaleString()}.`
              : 'Filing puts a PDF slip under "Salary slip" in each employee\'s documents, so their pay history sits with their file.'}
        </p>
        {filing && filing.done === filing.total && filing.errors.length > 0 && (
          <ul className="text-xs text-red-600 mt-2">{filing.errors.map((e) => <li key={e}>{e}</li>)}</ul>
        )}
      </div>

      {/* ── Totals ── */}
      <div className="grid grid-cols-2 md:grid-cols-4 lg:grid-cols-6 gap-3 mb-5">
        <Stat label="Employees" value={String(run.employeeCount)} />
        <Stat label="Gross" value={rs(run.grossTotal)} />
        <Stat label="Deductions" value={rs(run.deductionsTotal)} />
        <Stat label="Net pay" value={rs(run.netTotal)} strong />
        <Stat label="Employer EPF + ETF" value={rs(n(run.epfEmployerTotal) + n(run.etfTotal))} />
        <Stat label="Cost to company" value={rs(run.costToCompanyTotal)} />
      </div>

      {/* ── Pay / remit ── */}
      {(run.status === 'POSTED' || run.status === 'PAID') && (
        <div className="grid grid-cols-1 lg:grid-cols-2 gap-4 mb-5">
          {run.status === 'POSTED' && (
            <div className="card">
              <h3 className="text-sm font-semibold text-gray-700 mb-2">Pay salaries</h3>
              <form className="form-grid" style={{ gridTemplateColumns: '1fr 1fr' }}
                onSubmit={(e) => { e.preventDefault(); act(() => api.post(`/payroll/runs/${id}/pay`, payForm), 'Could not record the payment'); }}>
                <label className="block">
                  <span className="text-xs font-medium text-gray-500">From account</span>
                  <select required value={payForm.paymentAccountId} onChange={(e) => setPayForm({ ...payForm, paymentAccountId: e.target.value })}>
                    <option value="">Choose bank / cash account</option>
                    {accounts.map((a) => <option key={a.id} value={a.id}>{a.code} – {a.name}</option>)}
                  </select>
                </label>
                <label className="block">
                  <span className="text-xs font-medium text-gray-500">Pay date</span>
                  <input type="date" required value={payForm.payDate} onChange={(e) => setPayForm({ ...payForm, payDate: e.target.value })} />
                </label>
                <div style={{ gridColumn: '1 / -1' }}>
                  <button type="submit" className="btn btn-primary" disabled={busy}>Record payment of {rs(run.netTotal)}</button>
                </div>
              </form>
            </div>
          )}

          <div className="card">
            <h3 className="text-sm font-semibold text-gray-700 mb-2">Statutory remittances</h3>
            <table className="text-sm w-full mb-3">
              <thead><tr className="text-xs text-gray-500"><th className="text-left py-1">Fund</th><th className="text-right py-1">Due</th><th className="text-right py-1">Remitted</th><th className="text-right py-1">Outstanding</th></tr></thead>
              <tbody>
                {(Object.keys(REMIT_LABEL) as RemittanceType[]).map((t) => (
                  <tr key={t} className="border-t border-gray-100">
                    <td className="py-1">{REMIT_LABEL[t]}</td>
                    <td className="py-1 text-right">{amt(run.owed[t].total)}</td>
                    <td className="py-1 text-right">{amt(run.owed[t].remitted)}</td>
                    <td className={`py-1 text-right ${run.owed[t].outstanding > 0 ? 'font-medium text-amber-700' : 'text-gray-400'}`}>{amt(run.owed[t].outstanding)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
            <form className="form-grid" style={{ gridTemplateColumns: '1fr 1fr 1fr' }}
              onSubmit={(e) => {
                e.preventDefault();
                act(() => api.post(`/payroll/runs/${id}/remit`, {
                  type: remitForm.type,
                  paymentAccountId: remitForm.paymentAccountId,
                  date: remitForm.date,
                  amount: remitForm.amount.trim() === '' ? undefined : Number(remitForm.amount),
                  reference: remitForm.reference.trim() || null,
                }), 'Could not record the remittance');
              }}>
              <label className="block">
                <span className="text-xs font-medium text-gray-500">Fund</span>
                <select value={remitForm.type} onChange={(e) => setRemitForm({ ...remitForm, type: e.target.value as RemittanceType })}>
                  {(Object.keys(REMIT_LABEL) as RemittanceType[]).map((t) => <option key={t} value={t}>{REMIT_LABEL[t]}</option>)}
                </select>
              </label>
              <label className="block">
                <span className="text-xs font-medium text-gray-500">From account</span>
                <select required value={remitForm.paymentAccountId} onChange={(e) => setRemitForm({ ...remitForm, paymentAccountId: e.target.value })}>
                  <option value="">Choose</option>
                  {accounts.map((a) => <option key={a.id} value={a.id}>{a.code} – {a.name}</option>)}
                </select>
              </label>
              <label className="block">
                <span className="text-xs font-medium text-gray-500">Date</span>
                <input type="date" required value={remitForm.date} onChange={(e) => setRemitForm({ ...remitForm, date: e.target.value })} />
              </label>
              <label className="block">
                <span className="text-xs font-medium text-gray-500">Amount (blank = all outstanding)</span>
                <input type="number" min="0" step="0.01" value={remitForm.amount} onChange={(e) => setRemitForm({ ...remitForm, amount: e.target.value })} />
              </label>
              <label className="block">
                <span className="text-xs font-medium text-gray-500">Reference (cheque / ref no.)</span>
                <input value={remitForm.reference} onChange={(e) => setRemitForm({ ...remitForm, reference: e.target.value })} />
              </label>
              <div className="flex items-end">
                <button type="submit" className="btn btn-secondary" disabled={busy}>Record remittance</button>
              </div>
            </form>
          </div>
        </div>
      )}

      {/* ── Lines ── */}
      <div className="flex items-center justify-between mb-2">
        <h2 className="section-title mb-0">Salary sheet</h2>
        {isDraft && addable.length > 0 && (
          <form className="flex items-center gap-2" onSubmit={(e) => { e.preventDefault(); if (addEmployeeId) act(() => api.post(`/payroll/runs/${id}/lines`, { employeeId: addEmployeeId }).then(() => setAddEmployeeId('')), 'Could not add the employee'); }}>
            <select value={addEmployeeId} onChange={(e) => setAddEmployeeId(e.target.value)}>
              <option value="">Add an employee…</option>
              {addable.map((e) => <option key={e.id} value={e.id}>{e.employeeId} – {e.name}</option>)}
            </select>
            <button type="submit" className="btn btn-secondary" disabled={!addEmployeeId || busy}>Add</button>
          </form>
        )}
      </div>

      {editing && (
        <div className="card mb-4">
          <p className="text-sm font-medium text-gray-800 mb-3">
            {editing.employeeName} <span className="text-gray-400 font-mono text-xs">{editing.employeeCode}</span>
            <span className="text-xs text-gray-500 ml-2">Basic {amt(editing.basic)} · {editing.epfApplicable ? 'EPF applies' : 'No EPF'}</span>
          </p>
          <form onSubmit={saveLine} className="form-grid" style={{ gridTemplateColumns: 'repeat(4, 1fr)' }}>
            <Num label={`Days worked (of ${editing.daysInMonth})`} k="daysWorked" draft={draft} setDraft={setDraft} />
            <Num label="Project bonus / allowance" k="projectBonus" draft={draft} setDraft={setDraft} />
            <Num label="Arrears" k="arrears" draft={draft} setDraft={setDraft} />
            <Num label="Medical" k="medical" draft={draft} setDraft={setDraft} />
            <Num label="Other earnings" k="otherEarnings" draft={draft} setDraft={setDraft} />
            <Num label="No-pay amount" k="noPayAmount" draft={draft} setDraft={setDraft} />
            <Num label="Staff welfare" k="welfare" draft={draft} setDraft={setDraft} />
            <Num label="Loan recovery" k="loanRecovery" draft={draft} setDraft={setDraft} />
            <Num label="Salary advance" k="salaryAdvance" draft={draft} setDraft={setDraft} />
            <Num label="Phone bill" k="phoneBill" draft={draft} setDraft={setDraft} />
            <Num label="Other deductions" k="otherDeductions" draft={draft} setDraft={setDraft} />
            <Num label="PAYE override (blank = slab calc)" k="payeOverride" draft={draft} setDraft={setDraft} />
            <label className="block" style={{ gridColumn: '1 / -1' }}>
              <span className="text-xs font-medium text-gray-500">Notes (printed on the slip)</span>
              <input value={draft.notes ?? ''} onChange={(e) => setDraft({ ...draft, notes: e.target.value })} />
            </label>
            {lineError && <p className="text-sm text-red-600" style={{ gridColumn: '1 / -1' }}>{lineError}</p>}
            <div className="flex gap-2" style={{ gridColumn: '1 / -1' }}>
              <button type="submit" className="btn btn-primary" disabled={saving}>{saving ? 'Saving…' : 'Save line'}</button>
              <button type="button" className="btn btn-secondary" onClick={() => setEditing(null)}>Cancel</button>
            </div>
          </form>
        </div>
      )}

      <div className="overflow-x-auto rounded-lg border border-gray-200 bg-white shadow-sm">
        <table className="min-w-full text-xs border-collapse">
          <thead>
            <tr className="bg-gray-50 border-b border-gray-200 text-gray-500 uppercase tracking-wide">
              <Th left>Employee</Th><Th>Days</Th><Th>Basic (month)</Th><Th>Allowances</Th><Th>Bonus / other</Th><Th>No pay</Th><Th>Gross</Th>
              <Th>EPF 8%</Th><Th>PAYE</Th><Th>Stamp</Th><Th>Welfare</Th><Th>Loan / adv</Th><Th>Other ded.</Th><Th>Total ded.</Th><Th strong>Net</Th>
              <Th>EPF 12%</Th><Th>ETF 3%</Th><Th>CTC</Th><Th />
            </tr>
          </thead>
          <tbody>
            {run.lines.length === 0 && (
              <tr><td colSpan={19} className="px-4 py-8 text-center text-gray-400">No employees in this run.</td></tr>
            )}
            {run.lines.map((l) => (
              <tr key={l.id} className="border-b border-gray-100 hover:bg-blue-50/40">
                <td className="px-3 py-2 whitespace-nowrap">
                  <div className="font-medium text-gray-900">{l.employeeName}</div>
                  <div className="text-gray-400 font-mono">{l.employeeCode}{l.epfNumber ? ` · EPF ${l.epfNumber}` : ''}{!l.epfApplicable ? ' · no EPF' : ''}</div>
                  {l.notes && <div className="text-gray-500 italic">{l.notes}</div>}
                </td>
                <Td>{n(l.daysWorked)}</Td>
                <Td>{amt(l.basicForMonth)}</Td>
                <Td>{amt(n(l.bra2005) + n(l.bra2016) + n(l.vehicleAllowance) + n(l.otherAllowance))}</Td>
                <Td>{amt(n(l.projectBonus) + n(l.arrears) + n(l.medical) + n(l.otherEarnings))}</Td>
                <Td muted={!n(l.noPayAmount)}>{amt(l.noPayAmount)}</Td>
                <Td>{amt(l.grossSalary)}</Td>
                <Td>{amt(l.epfEmployee)}</Td>
                <Td>{amt(l.paye)}{l.payeOverride !== null && <span title="Overridden">*</span>}</Td>
                <Td>{amt(l.stampDuty)}</Td>
                <Td>{amt(l.welfare)}</Td>
                <Td muted={!n(l.loanRecovery) && !n(l.salaryAdvance)}>{amt(n(l.loanRecovery) + n(l.salaryAdvance))}</Td>
                <Td muted={!n(l.phoneBill) && !n(l.otherDeductions)}>{amt(n(l.phoneBill) + n(l.otherDeductions))}</Td>
                <Td>{amt(l.deductionsTotal)}</Td>
                <Td strong>{amt(l.netSalary)}</Td>
                <Td>{amt(l.epfEmployer)}</Td>
                <Td>{amt(l.etf)}</Td>
                <Td>{amt(l.costToCompany)}</Td>
                <td className="px-2 py-2 whitespace-nowrap">
                  <button className="btn-link text-xs mr-2" disabled={docBusy === l.id} onClick={() => slipFor(l)}>Slip</button>
                  {isDraft && (
                    <>
                      <button className="btn-link text-xs" onClick={() => startEdit(l)}>Edit</button>
                      <button className="btn-link text-xs ml-2" style={{ color: '#dc2626' }}
                        onClick={() => window.confirm(`Remove ${l.employeeName} from this run?`) && act(() => api.delete(`/payroll/runs/${id}/lines/${l.id}`), 'Could not remove the line')}>
                        Remove
                      </button>
                    </>
                  )}
                </td>
              </tr>
            ))}
          </tbody>
          {run.lines.length > 0 && (
            <tfoot>
              <tr className="bg-gray-50 border-t border-gray-200 font-semibold text-gray-800">
                <td className="px-3 py-2">Totals ({run.employeeCount})</td>
                <Td /> <Td>{amt(run.lines.reduce((s, l) => s + n(l.basicForMonth), 0))}</Td>
                <Td>{amt(run.lines.reduce((s, l) => s + n(l.bra2005) + n(l.bra2016) + n(l.vehicleAllowance) + n(l.otherAllowance), 0))}</Td>
                <Td>{amt(run.lines.reduce((s, l) => s + n(l.projectBonus) + n(l.arrears) + n(l.medical) + n(l.otherEarnings), 0))}</Td>
                <Td>{amt(run.noPayTotal)}</Td><Td>{amt(run.grossTotal)}</Td><Td>{amt(run.epfEmployeeTotal)}</Td><Td>{amt(run.payeTotal)}</Td>
                <Td>{amt(run.stampDutyTotal)}</Td><Td>{amt(run.welfareTotal)}</Td>
                <Td>{amt(n(run.loanRecoveryTotal) + n(run.salaryAdvanceTotal))}</Td><Td>{amt(run.otherDeductionsTotal)}</Td>
                <Td>{amt(run.deductionsTotal)}</Td><Td strong>{amt(run.netTotal)}</Td>
                <Td>{amt(run.epfEmployerTotal)}</Td><Td>{amt(run.etfTotal)}</Td><Td>{amt(run.costToCompanyTotal)}</Td><td />
              </tr>
            </tfoot>
          )}
        </table>
      </div>

      {run.remittances.length > 0 && (
        <div className="mt-5">
          <h2 className="section-title">Remittances recorded</h2>
          <div className="card text-sm">
            <ul className="space-y-1">
              {run.remittances.map((r) => (
                <li key={r.id} className="flex justify-between">
                  <span>{r.date} · {REMIT_LABEL[r.type]}{r.reference ? ` · ${r.reference}` : ''}{r.createdByName ? ` · by ${r.createdByName}` : ''}</span>
                  <span className="font-medium">{rs(r.amount)}</span>
                </li>
              ))}
            </ul>
          </div>
        </div>
      )}
    </div>
  );
}

function Stat({ label, value, strong = false }: { label: string; value: string; strong?: boolean }) {
  return (
    <div className="card py-3">
      <div className="text-xs text-gray-500">{label}</div>
      <div className={`${strong ? 'text-lg font-bold text-gray-900' : 'text-base font-semibold text-gray-800'}`}>{value}</div>
    </div>
  );
}

function Th({ children, left = false, strong = false }: { children?: React.ReactNode; left?: boolean; strong?: boolean }) {
  return <th className={`px-3 py-2 font-semibold whitespace-nowrap ${left ? 'text-left' : 'text-right'} ${strong ? 'text-gray-800' : ''}`}>{children}</th>;
}

function Td({ children, strong = false, muted = false }: { children?: React.ReactNode; strong?: boolean; muted?: boolean }) {
  return <td className={`px-3 py-2 text-right whitespace-nowrap tabular-nums ${strong ? 'font-semibold text-gray-900' : ''} ${muted ? 'text-gray-300' : 'text-gray-700'}`}>{children}</td>;
}

function Num({ label, k, draft, setDraft }: { label: string; k: string; draft: Record<string, string>; setDraft: (d: Record<string, string>) => void }) {
  return (
    <label className="block">
      <span className="text-xs font-medium text-gray-500">{label}</span>
      <input type="number" min="0" step="0.01" placeholder="0" value={draft[k] ?? ''} onChange={(e) => setDraft({ ...draft, [k]: e.target.value })} />
    </label>
  );
}
