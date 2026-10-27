import { useCallback, useEffect, useState } from 'react';
import { useLocation, useNavigate, useParams } from 'react-router-dom';
import api, { errMsg } from '../api/client';
import PageHeader from '../components/PageHeader';
import StatusBadge from '../components/StatusBadge';
import { amt } from '../lib/payroll';
import { fmtDate } from '../lib/region';
import { US_REMIT_LABEL, US_RUN_STATUS_HELP, usd, type UsLine, type UsRemitType, type UsRunDetail } from '../lib/usPayroll';

interface Account { id: string; code: string; name: string; type: string }
interface Employee { id: string; employeeId: string; name: string }

const today = () => new Date().toISOString().slice(0, 10);

export default function UsPayrollRun() {
  const { id = '' } = useParams();
  const navigate = useNavigate();
  const location = useLocation();
  const [skipped, setSkipped] = useState((location.state as UsRunDetail | null)?.skipped ?? []);

  const [run, setRun] = useState<UsRunDetail | null>(null);
  const [error, setError] = useState('');
  const [accounts, setAccounts] = useState<Account[]>([]);
  const [employees, setEmployees] = useState<Employee[]>([]);

  const [editing, setEditing] = useState<UsLine | null>(null);
  const [draft, setDraft] = useState<Record<string, string>>({});
  const [lineError, setLineError] = useState('');
  const [saving, setSaving] = useState(false);

  const [addEmployeeId, setAddEmployeeId] = useState('');
  const [payForm, setPayForm] = useState({ paymentAccountId: '', date: '' });
  const [remitForm, setRemitForm] = useState({ type: 'FORM_941' as UsRemitType, paymentAccountId: '', date: today(), amount: '', reference: '' });
  const [actionError, setActionError] = useState('');
  const [busy, setBusy] = useState(false);

  const load = useCallback(
    () =>
      api
        .get<UsRunDetail>(`/payroll/us/runs/${id}`)
        .then((r) => { setRun(r.data); setError(''); })
        .catch((err) => setError(errMsg(err, 'Pay run not found'))),
    [id],
  );
  useEffect(() => { load(); }, [load]);
  useEffect(() => {
    api.get('/accounts').then((r) => setAccounts(r.data.filter((a: Account) => a.type === 'ASSET'))).catch(() => undefined);
    api.get('/employees').then((r) => setEmployees(r.data)).catch(() => undefined);
  }, []);

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

  const startEdit = (l: UsLine) => {
    setEditing(l);
    setLineError('');
    const s = (v: number) => (v ? String(v) : '');
    setDraft({
      hours: String(l.hours),
      overtimeHours: s(l.overtimeHours),
      regularPay: String(l.regularPay),
      bonus: s(l.bonus),
      otherEarnings: s(l.otherEarnings),
      notes: l.notes ?? '',
    });
  };

  const saveLine = async (e: React.FormEvent, refresh = false) => {
    e.preventDefault();
    if (!editing) return;
    setSaving(true);
    setLineError('');
    const num = (v: string) => (v.trim() === '' ? 0 : Number(v));
    const hourly = editing.payType === 'HOURLY';
    try {
      await api.patch(`/payroll/us/runs/${id}/lines/${editing.id}`, {
        ...(hourly ? { hours: num(draft.hours), overtimeHours: num(draft.overtimeHours) } : { regularPay: num(draft.regularPay) }),
        bonus: num(draft.bonus),
        otherEarnings: num(draft.otherEarnings),
        notes: draft.notes.trim() || null,
        ...(refresh ? { refresh: true } : {}),
      });
      setEditing(null);
      await load();
    } catch (err) {
      setLineError(errMsg(err, 'Could not save the line'));
    } finally {
      setSaving(false);
    }
  };

  if (error) {
    return (
      <div>
        <PageHeader title="US pay run" />
        <div className="card text-sm text-red-600">{error}</div>
        <button onClick={() => navigate('/payroll/us')} className="btn-link mt-3">← Back to US payroll</button>
      </div>
    );
  }
  if (!run) {
    return <div className="flex items-center justify-center py-20"><div className="w-6 h-6 border-2 border-blue-600 border-t-transparent rounded-full animate-spin" /></div>;
  }

  const isDraft = run.status === 'DRAFT';
  const live = run.status === 'POSTED' || run.status === 'PAID';
  const inRun = new Set(run.lines.map((l) => l.employeeId));
  const addable = employees.filter((e) => !inRun.has(e.id));
  const accountName = (aid: string | null) => accounts.find((a) => a.id === aid)?.name ?? '';
  const title = `US pay run — ${fmtDate(run.periodStart)} to ${fmtDate(run.periodEnd)}`;
  const sum = (f: (l: UsLine) => number) => run.lines.reduce((s, l) => s + f(l), 0);
  const ss = (l: UsLine) => l.socialSecurityEmployee;
  const med = (l: UsLine) => l.medicareEmployee + l.additionalMedicare;

  return (
    <div>
      <div className="mb-3"><button onClick={() => navigate('/payroll/us')} className="btn-link text-xs">← All US pay runs</button></div>

      <PageHeader
        title={title}
        action={
          <>
            <StatusBadge status={run.status} />
            {isDraft && (
              <>
                <button className="btn btn-primary" disabled={busy || run.lines.length === 0}
                  onClick={() => window.confirm('Post this pay run to the ledger? Lines can no longer be edited afterwards.') && act(() => api.post(`/payroll/us/runs/${id}/post`), 'Could not post the run')}>
                  Post to ledger
                </button>
                <button className="btn btn-danger" disabled={busy}
                  onClick={() => window.confirm('Delete this draft pay run?') && act(async () => { await api.delete(`/payroll/us/runs/${id}`); navigate('/payroll/us'); }, 'Could not delete the run')}>
                  Delete draft
                </button>
              </>
            )}
            {live && (
              <button className="btn btn-danger" disabled={busy}
                onClick={() => window.confirm('Void this pay run? Every ledger entry it made (posting, net pay, tax deposits) will be reversed.') && act(() => api.post(`/payroll/us/runs/${id}/void`), 'Could not void the run')}>
                Void
              </button>
            )}
          </>
        }
      />

      <p className="text-xs text-gray-500 mb-4">
        {run.runNumber} · Pay date {fmtDate(run.payDate)} · {US_RUN_STATUS_HELP[run.status]}
        {run.paidDate && ` · Paid ${fmtDate(run.paidDate)} from ${accountName(run.paymentAccountId)}`}
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

      <div className="grid grid-cols-2 md:grid-cols-4 lg:grid-cols-6 gap-3 mb-5">
        <Stat label="Employees" value={String(run.employeeCount)} />
        <Stat label="Gross pay" value={usd(run.grossTotal)} />
        <Stat label="Federal income tax" value={usd(run.federalIncomeTaxTotal)} />
        <Stat label="Net pay" value={usd(run.netPayTotal)} strong />
        <Stat label="Employer taxes" value={usd(run.employerTaxesTotal)} />
        <Stat label="Total cost" value={usd(run.grossTotal + run.employerTaxesTotal)} />
      </div>

      {live && (
        <div className="grid grid-cols-1 lg:grid-cols-2 gap-4 mb-5">
          {run.status === 'POSTED' && (
            <div className="card">
              <h3 className="text-sm font-semibold text-gray-700 mb-2">Pay employees</h3>
              <form className="form-grid" style={{ gridTemplateColumns: '1fr 1fr' }}
                onSubmit={(e) => { e.preventDefault(); act(() => api.post(`/payroll/us/runs/${id}/pay`, { paymentAccountId: payForm.paymentAccountId, date: payForm.date || undefined }), 'Could not record the payment'); }}>
                <label className="block">
                  <span className="text-xs font-medium text-gray-500">From account</span>
                  <select required value={payForm.paymentAccountId} onChange={(e) => setPayForm({ ...payForm, paymentAccountId: e.target.value })}>
                    <option value="">Choose bank / cash account</option>
                    {accounts.map((a) => <option key={a.id} value={a.id}>{a.code} – {a.name}</option>)}
                  </select>
                </label>
                <label className="block">
                  <span className="text-xs font-medium text-gray-500">Date (blank = pay date)</span>
                  <input type="date" value={payForm.date} onChange={(e) => setPayForm({ ...payForm, date: e.target.value })} />
                </label>
                <div style={{ gridColumn: '1 / -1' }}>
                  <button type="submit" className="btn btn-primary" disabled={busy}>Record net pay of {usd(run.netPayTotal)}</button>
                </div>
              </form>
            </div>
          )}

          <div className="card">
            <h3 className="text-sm font-semibold text-gray-700 mb-2">Tax deposits</h3>
            <table className="text-sm w-full mb-3">
              <thead><tr className="text-xs text-gray-500"><th className="text-left py-1">Tax</th><th className="text-right py-1">Owed</th><th className="text-right py-1">Paid</th><th className="text-right py-1">Outstanding</th></tr></thead>
              <tbody>
                {run.taxes.filter((t) => t.total > 0).map((t) => (
                  <tr key={t.type} className="border-t border-gray-100">
                    <td className="py-1">{US_REMIT_LABEL[t.type]}</td>
                    <td className="py-1 text-right">{amt(t.total)}</td>
                    <td className="py-1 text-right">{amt(t.remitted)}</td>
                    <td className={`py-1 text-right ${t.outstanding > 0 ? 'font-medium text-amber-700' : 'text-gray-400'}`}>{amt(t.outstanding)}</td>
                  </tr>
                ))}
              </tbody>
            </table>
            <form className="form-grid" style={{ gridTemplateColumns: '1fr 1fr 1fr' }}
              onSubmit={(e) => {
                e.preventDefault();
                act(() => api.post(`/payroll/us/runs/${id}/remit`, {
                  type: remitForm.type,
                  paymentAccountId: remitForm.paymentAccountId,
                  date: remitForm.date,
                  amount: remitForm.type === 'FORM_941' || remitForm.amount.trim() === '' ? undefined : Number(remitForm.amount),
                  reference: remitForm.reference.trim() || null,
                }).then(() => setRemitForm({ ...remitForm, amount: '', reference: '' })), 'Could not record the payment');
              }}>
              <label className="block" style={{ gridColumn: '1 / -1' }}>
                <span className="text-xs font-medium text-gray-500">Pay</span>
                <select value={remitForm.type} onChange={(e) => setRemitForm({ ...remitForm, type: e.target.value as UsRemitType })}>
                  {(Object.keys(US_REMIT_LABEL) as UsRemitType[]).map((t) => <option key={t} value={t}>{US_REMIT_LABEL[t]}</option>)}
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
                <input type="number" min="0" step="0.01" disabled={remitForm.type === 'FORM_941'} value={remitForm.amount} onChange={(e) => setRemitForm({ ...remitForm, amount: e.target.value })} />
              </label>
              <label className="block" style={{ gridColumn: '1 / 3' }}>
                <span className="text-xs font-medium text-gray-500">Reference (e.g. EFTPS confirmation)</span>
                <input value={remitForm.reference} onChange={(e) => setRemitForm({ ...remitForm, reference: e.target.value })} />
              </label>
              <div className="flex items-end">
                <button type="submit" className="btn btn-secondary" disabled={busy}>Record payment</button>
              </div>
            </form>
          </div>
        </div>
      )}

      <div className="flex items-center justify-between mb-2">
        <h2 className="section-title mb-0">Employees</h2>
        {isDraft && addable.length > 0 && (
          <form className="flex items-center gap-2" onSubmit={(e) => { e.preventDefault(); if (addEmployeeId) act(() => api.post(`/payroll/us/runs/${id}/lines`, { employeeId: addEmployeeId }).then(() => setAddEmployeeId('')), 'Could not add the employee'); }}>
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
            <span className="text-xs text-gray-500 ml-2">
              {editing.payType === 'HOURLY' ? `Hourly ${usd(editing.hourlyRate)}` : `Salary ${usd(editing.annualSalary)} a year`} · {editing.workState}
            </span>
          </p>
          <form onSubmit={(e) => saveLine(e)} className="form-grid" style={{ gridTemplateColumns: 'repeat(4, 1fr)' }}>
            {editing.payType === 'HOURLY' ? (
              <>
                <Num label="Hours" k="hours" draft={draft} setDraft={setDraft} />
                <Num label="Overtime hours (× 1.5)" k="overtimeHours" draft={draft} setDraft={setDraft} />
              </>
            ) : (
              <Num label="Regular pay this period" k="regularPay" draft={draft} setDraft={setDraft} />
            )}
            <Num label="Bonus" k="bonus" draft={draft} setDraft={setDraft} />
            <Num label="Other earnings" k="otherEarnings" draft={draft} setDraft={setDraft} />
            <label className="block" style={{ gridColumn: '1 / -1' }}>
              <span className="text-xs font-medium text-gray-500">Notes</span>
              <input value={draft.notes ?? ''} onChange={(e) => setDraft({ ...draft, notes: e.target.value })} />
            </label>
            {lineError && <p className="text-sm text-red-600" style={{ gridColumn: '1 / -1' }}>{lineError}</p>}
            <div className="flex flex-wrap gap-2" style={{ gridColumn: '1 / -1' }}>
              <button type="submit" className="btn btn-primary" disabled={saving}>{saving ? 'Saving…' : 'Save line'}</button>
              <button type="button" className="btn btn-secondary" disabled={saving} onClick={(e) => saveLine(e, true)}
                title="Use the employee's current pay, Form W-4 and deductions">
                Save and refresh W-4 / pay
              </button>
              <button type="button" className="btn btn-secondary" onClick={() => setEditing(null)}>Cancel</button>
            </div>
          </form>
        </div>
      )}

      <div className="overflow-x-auto rounded-lg border border-gray-200 bg-white shadow-sm">
        <table className="min-w-full text-xs border-collapse">
          <thead>
            <tr className="bg-gray-50 border-b border-gray-200 text-gray-500 uppercase tracking-wide">
              <Th left>Employee</Th><Th>Regular</Th><Th>Extra pay</Th><Th>Gross</Th>
              <Th>Pre-tax</Th><Th>Federal tax</Th><Th>State tax</Th><Th>Soc. sec.</Th><Th>Medicare</Th><Th strong>Net</Th>
              <Th>Employer tax</Th><Th />
            </tr>
          </thead>
          <tbody>
            {run.lines.length === 0 && (
              <tr><td colSpan={12} className="px-4 py-8 text-center text-gray-400">No employees in this run.</td></tr>
            )}
            {run.lines.map((l) => (
              <tr key={l.id} className="border-b border-gray-100 hover:bg-blue-50/40">
                <td className="px-3 py-2 whitespace-nowrap">
                  <div className="font-medium text-gray-900">{l.employeeName}</div>
                  <div className="text-gray-400 font-mono">
                    {l.employeeCode} · {l.workState} · {l.payType === 'HOURLY'
                      ? `${amt(l.hours)} h${l.overtimeHours ? ` + ${amt(l.overtimeHours)} OT` : ''} × ${usd(l.hourlyRate)}`
                      : 'salary'}
                  </div>
                  {l.notes && <div className="text-gray-500 italic">{l.notes}</div>}
                </td>
                <Td>{amt(l.regularPay)}</Td>
                <Td muted={!l.overtimePay && !l.bonus && !l.otherEarnings}>{amt(l.overtimePay + l.bonus + l.otherEarnings)}</Td>
                <Td>{amt(l.gross)}</Td>
                <Td muted={!l.pretax401k && !l.pretaxSection125}>{amt(l.pretax401k + l.pretaxSection125)}</Td>
                <Td>{amt(l.federalIncomeTax)}</Td>
                <Td>{l.stateTaxConfigured ? amt(l.stateIncomeTax) : <span className="text-amber-700" title={`No ${l.workState} rule yet`}>{l.workState}?</span>}</Td>
                <Td>{amt(ss(l))}</Td>
                <Td>{amt(med(l))}</Td>
                <Td strong>{amt(l.netPay)}</Td>
                <Td>{amt(l.employerTaxes)}</Td>
                <td className="px-2 py-2 whitespace-nowrap">
                  {isDraft && (
                    <>
                      <button className="btn-link text-xs" onClick={() => startEdit(l)}>Edit</button>
                      <button className="btn-link text-xs ml-2" style={{ color: '#dc2626' }}
                        onClick={() => window.confirm(`Remove ${l.employeeName} from this run?`) && act(() => api.delete(`/payroll/us/runs/${id}/lines/${l.id}`), 'Could not remove the line')}>
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
                <Td>{amt(sum((l) => l.regularPay))}</Td>
                <Td>{amt(sum((l) => l.overtimePay + l.bonus + l.otherEarnings))}</Td>
                <Td>{amt(run.grossTotal)}</Td>
                <Td>{amt(run.pretax401kTotal + run.pretaxSection125Total)}</Td>
                <Td>{amt(run.federalIncomeTaxTotal)}</Td>
                <Td>{amt(run.stateIncomeTaxTotal)}</Td>
                <Td>{amt(run.socialSecurityEmployeeTotal)}</Td>
                <Td>{amt(run.medicareEmployeeTotal + run.additionalMedicareTotal)}</Td>
                <Td strong>{amt(run.netPayTotal)}</Td>
                <Td>{amt(run.employerTaxesTotal)}</Td>
                <td />
              </tr>
            </tfoot>
          )}
        </table>
      </div>
      <p className="text-xs text-gray-500 mt-2">
        Extra pay is overtime (time and a half), bonus and other earnings. Employer tax is the company's social security and Medicare shares, FUTA and state unemployment. State tax follows each state's rule in US payroll settings ("XX?" = no rule yet; posting waits for it). Local income tax is not included.
      </p>

      {run.remittances.length > 0 && (
        <div className="mt-5">
          <h2 className="section-title">Tax payments recorded</h2>
          <div className="card text-sm">
            <ul className="space-y-1">
              {run.remittances.map((r) => (
                <li key={r.id} className="flex justify-between">
                  <span>{fmtDate(r.date)} · {US_REMIT_LABEL[r.type]}{r.reference ? ` · ${r.reference}` : ''}{r.createdByName ? ` · by ${r.createdByName}` : ''}</span>
                  <span className="font-medium">{usd(r.amount)}</span>
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
  return <th className={`px-2 py-2 font-semibold whitespace-nowrap ${left ? 'text-left' : 'text-right'} ${strong ? 'text-gray-800' : ''}`}>{children}</th>;
}

function Td({ children, strong = false, muted = false }: { children?: React.ReactNode; strong?: boolean; muted?: boolean }) {
  return <td className={`px-2 py-2 text-right whitespace-nowrap tabular-nums ${strong ? 'font-semibold text-gray-900' : ''} ${muted ? 'text-gray-300' : 'text-gray-700'}`}>{children}</td>;
}

function Num({ label, k, draft, setDraft }: { label: string; k: string; draft: Record<string, string>; setDraft: (d: Record<string, string>) => void }) {
  return (
    <label className="block">
      <span className="text-xs font-medium text-gray-500">{label}</span>
      <input type="number" min="0" step="0.01" placeholder="0" value={draft[k] ?? ''} onChange={(e) => setDraft({ ...draft, [k]: e.target.value })} />
    </label>
  );
}
