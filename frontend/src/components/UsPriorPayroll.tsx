import { useCallback, useEffect, useState } from 'react';
import api, { errMsg } from '../api/client';
import { askConfirm } from '../lib/confirm';
import { fmtDate, todayIso } from '../lib/region';
import { toast } from '../lib/toast';
import { usd } from '../lib/usPayroll';

/**
 * D83: payroll paid before the company started using TallyCheck this year (another provider or
 * by hand). It never reaches the ledger; it counts towards the year's wage limits and is part of
 * Forms 941, 940, W-2 and the state forms, as already deposited by the previous provider.
 */
interface Employee { id: string; employeeId: string; name: string; status?: string }
interface PriorRow {
  id: string; employeeId: string; employeeCode: string; employeeName: string; payDate: string;
  workState: string; sutaState: string; notes: string | null;
  [figure: string]: string | number | null;
}

const MAIN: [string, string][] = [
  ['gross', 'Gross pay'],
  ['federalIncomeTax', 'Federal income tax withheld'],
  ['socialSecurityEmployee', 'Social security withheld'],
  ['medicareEmployee', 'Medicare withheld'],
  ['stateIncomeTax', 'State income tax withheld'],
  ['pretax401k', '401(k) deferrals'],
];
const MORE: [string, string][] = [
  ['pretaxSection125', 'Section 125 (cafeteria) deductions'],
  ['incomeTaxWages', 'Federal income tax wages (W-2 box 1)'],
  ['socialSecurityWages', 'Social security wages'],
  ['medicareWages', 'Medicare wages'],
  ['additionalMedicareWages', 'Additional Medicare wages (over $200,000)'],
  ['additionalMedicare', 'Additional Medicare withheld'],
  ['socialSecurityEmployer', "Employer's social security"],
  ['medicareEmployer', "Employer's Medicare"],
  ['futaWages', 'FUTA wages'],
  ['futa', 'FUTA tax'],
  ['sutaWages', 'State unemployment wages'],
  ['suta', 'State unemployment tax (employer)'],
  ['stateIncomeTaxWages', 'State income tax wages'],
];
const ALL = [...MAIN, ...MORE].map(([k]) => k);

const blank = () => ({
  employeeId: '', payDate: '', workState: '', notes: '',
  ...Object.fromEntries(ALL.map((k) => [k, ''])),
}) as Record<string, string>;

export default function UsPriorPayroll() {
  const [year, setYear] = useState(Number(todayIso().slice(0, 4)));
  const [rows, setRows] = useState<PriorRow[]>([]);
  const [employees, setEmployees] = useState<Employee[]>([]);
  const [form, setForm] = useState<Record<string, string> | null>(null);
  const [editing, setEditing] = useState<string | null>(null);
  const [more, setMore] = useState(false);
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(false);

  const load = useCallback(() => {
    api.get('/payroll/us/prior', { params: { year } })
      .then((r) => setRows(r.data.rows))
      .catch((e) => setError(errMsg(e, 'Could not load payroll before TallyCheck')));
  }, [year]);
  useEffect(() => { load(); }, [load]);
  useEffect(() => {
    api.get('/employees', { params: { limit: 100 } })
      .then((r) => setEmployees(Array.isArray(r.data) ? r.data : r.data.data ?? []))
      .catch(() => undefined);
  }, []);

  const startNew = () => { setEditing(null); setMore(false); setError(''); setForm(blank()); };
  const startEdit = (p: PriorRow) => {
    setEditing(p.id);
    setMore(true);
    setError('');
    setForm({
      ...blank(), employeeId: p.employeeId, payDate: p.payDate, workState: p.workState, notes: p.notes ?? '',
      ...Object.fromEntries(ALL.map((k) => [k, p[k] === null || p[k] === undefined ? '' : String(p[k])])),
    });
  };

  const save = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!form) return;
    setBusy(true);
    setError('');
    // Figures left empty are worked out by TallyCheck (wages from the gross, the year's limits).
    const body: Record<string, unknown> = { employeeId: form.employeeId, payDate: form.payDate, notes: form.notes || null };
    if (form.workState.trim()) body.workState = form.workState.trim().toUpperCase();
    for (const k of ALL) if (form[k].trim() !== '') body[k] = Number(form[k]);
    try {
      if (editing) await api.put(`/payroll/us/prior/${editing}`, body);
      else await api.post('/payroll/us/prior', body);
      toast(editing ? 'Payroll before TallyCheck changed' : 'Payroll before TallyCheck entered');
      setForm(null);
      load();
    } catch (err) {
      setError(errMsg(err, 'Could not save it'));
    } finally {
      setBusy(false);
    }
  };

  const remove = async (p: PriorRow) => {
    if (!(await askConfirm(`Remove ${p.employeeName}'s payroll of ${fmtDate(p.payDate)} from before TallyCheck? The forms and wage limits will no longer count it.`))) return;
    try {
      await api.delete(`/payroll/us/prior/${p.id}`);
      toast('Removed');
      load();
    } catch (err) {
      toast(errMsg(err, 'Could not remove it'), 'error');
    }
  };

  const field = ([k, label]: [string, string]) => form && (
    <label key={k} className="block">
      <span className="text-xs font-medium text-gray-500">{label}{k === 'gross' ? ' *' : ''}</span>
      <input type="number" min="0" step="0.01" required={k === 'gross'} value={form[k]} aria-label={label}
        placeholder={k === 'gross' ? '' : 'worked out if empty'}
        onChange={(e) => setForm({ ...form, [k]: e.target.value })} />
    </label>
  );
  const total = (k: string) => rows.reduce((s, r) => s + Number(r[k] ?? 0), 0);

  return (
    <div className="card mb-5" aria-label="Payroll before TallyCheck">
      <div className="flex items-center justify-between mb-1">
        <h3 className="text-sm font-semibold text-gray-700">Payroll before TallyCheck</h3>
        <div className="flex items-center gap-2">
          <select value={year} onChange={(e) => setYear(Number(e.target.value))} aria-label="Year" className="!w-24">
            {[0, 1, 2].map((d) => Number(todayIso().slice(0, 4)) - d).map((y) => <option key={y} value={y}>{y}</option>)}
          </select>
          {!form && <button type="button" className="btn btn-secondary" onClick={startNew}>+ Enter payroll paid before TallyCheck</button>}
        </div>
      </div>
      <p className="text-xs text-gray-500 mb-3">
        Started TallyCheck part-way through the year? Enter what each employee was already paid this year — from your
        previous provider's payroll register, one line per pay date or one per month or quarter. It is not posted to the
        ledger (your opening balances already hold it), but it counts towards the social security, FUTA and state
        unemployment limits, and it is included in Forms 941, 940 and W-2 as already deposited.
      </p>
      {error && <p className="text-sm text-red-600 mb-2" role="alert">{error}</p>}

      {form && (
        <form onSubmit={save} className="border border-gray-200 rounded p-3 mb-3 space-y-3">
          <div className="form-grid" style={{ gridTemplateColumns: '2fr 1fr 1fr' }}>
            <label className="block">
              <span className="text-xs font-medium text-gray-500">Employee *</span>
              <select required value={form.employeeId} onChange={(e) => setForm({ ...form, employeeId: e.target.value })} aria-label="Employee">
                <option value="">Choose…</option>
                {employees.map((e) => <option key={e.id} value={e.id}>{e.employeeId} — {e.name}</option>)}
              </select>
            </label>
            <label className="block">
              <span className="text-xs font-medium text-gray-500">Pay date (or last day of the month / quarter) *</span>
              <input type="date" required value={form.payDate} max={todayIso()} onChange={(e) => setForm({ ...form, payDate: e.target.value })} aria-label="Pay date" />
            </label>
            <label className="block">
              <span className="text-xs font-medium text-gray-500">Work state</span>
              <input maxLength={2} placeholder="from the employee" value={form.workState} onChange={(e) => setForm({ ...form, workState: e.target.value })} aria-label="Work state" />
            </label>
          </div>
          <div className="form-grid" style={{ gridTemplateColumns: '1fr 1fr 1fr' }}>{MAIN.map(field)}</div>
          <button type="button" className="btn-link text-xs" onClick={() => setMore(!more)}>
            {more ? 'Fewer figures' : 'More figures (taxable wages, employer taxes, unemployment)'}
          </button>
          {more && <div className="form-grid" style={{ gridTemplateColumns: '1fr 1fr 1fr' }}>{MORE.map(field)}</div>}
          <label className="block">
            <span className="text-xs font-medium text-gray-500">Note</span>
            <input maxLength={500} value={form.notes} placeholder="e.g. From the old provider's register, January–September" onChange={(e) => setForm({ ...form, notes: e.target.value })} />
          </label>
          <div className="flex gap-2">
            <button type="submit" className="btn btn-primary" disabled={busy}>{busy ? 'Saving…' : editing ? 'Save changes' : 'Save'}</button>
            <button type="button" className="btn btn-secondary" onClick={() => setForm(null)}>Cancel</button>
          </div>
        </form>
      )}

      <table className="report-table w-full">
        <thead>
          <tr>
            <th>Employee</th><th>Pay date</th><th>State</th>
            <th className="num">Gross</th><th className="num">Federal tax</th><th className="num">Social security</th>
            <th className="num">Medicare</th><th className="num">State tax</th><th />
          </tr>
        </thead>
        <tbody>
          {rows.length === 0 && <tr><td colSpan={9} className="empty">Nothing entered for {year}.</td></tr>}
          {rows.map((p) => (
            <tr key={p.id}>
              <td>{p.employeeCode} — {p.employeeName}</td>
              <td>{fmtDate(p.payDate)}</td>
              <td>{p.workState}</td>
              <td className="num">{usd(Number(p.gross))}</td>
              <td className="num">{usd(Number(p.federalIncomeTax))}</td>
              <td className="num">{usd(Number(p.socialSecurityEmployee))}</td>
              <td className="num">{usd(Number(p.medicareEmployee) + Number(p.additionalMedicare))}</td>
              <td className="num">{usd(Number(p.stateIncomeTax))}</td>
              <td className="whitespace-nowrap">
                <button type="button" className="btn-link text-xs mr-3" onClick={() => startEdit(p)}>Edit</button>
                <button type="button" className="btn-link text-xs" style={{ color: '#dc2626' }} onClick={() => remove(p)}>Remove</button>
              </td>
            </tr>
          ))}
          {rows.length > 0 && (
            <tr className="total">
              <td colSpan={3}>Total {year}</td>
              <td className="num">{usd(total('gross'))}</td>
              <td className="num">{usd(total('federalIncomeTax'))}</td>
              <td className="num">{usd(total('socialSecurityEmployee'))}</td>
              <td className="num">{usd(total('medicareEmployee') + total('additionalMedicare'))}</td>
              <td className="num">{usd(total('stateIncomeTax'))}</td>
              <td />
            </tr>
          )}
        </tbody>
      </table>
    </div>
  );
}
