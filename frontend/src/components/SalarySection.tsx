import { useCallback, useEffect, useState } from 'react';
import api, { errMsg } from '../api/client';
import Table from './Table';
import ActionMenu from './ActionMenu';
import {
  emptySalaryForm,
  formatDate,
  rs,
  salaryToForm,
  toSalaryBody,
  type EmployeePayroll,
  type SalaryForm,
  type SalaryRecord,
} from '../lib/employees';
import { region } from '../lib/region';

const STATE_BADGE: Record<SalaryRecord['state'], { label: string; cls: string }> = {
  CURRENT: { label: 'Current', cls: 'bg-green-100 text-green-700' },
  UPCOMING: { label: 'Upcoming', cls: 'bg-blue-100 text-blue-700' },
  PAST: { label: 'Past', cls: 'bg-gray-100 text-gray-500' },
};

const emptyBank = { bankName: '', bankBranch: '', bankAccountNo: '', staffLoanBalance: '' };

/**
 * Salary, increments and bank details for one employee. Only rendered for
 * users with the `payroll` permission; the API refuses everyone else anyway.
 */
export default function SalarySection({ employeeId, isContract }: { employeeId: string; isContract: boolean }) {
  const [data, setData] = useState<EmployeePayroll | null>(null);
  const [error, setError] = useState('');

  // Salary form (new / edit)
  const [mode, setMode] = useState<'closed' | 'new' | 'edit'>('closed');
  const [target, setTarget] = useState<SalaryRecord | null>(null);
  const [form, setForm] = useState<SalaryForm>(emptySalaryForm);
  const [formError, setFormError] = useState('');
  const [busy, setBusy] = useState(false);

  // Bank form
  const [bankOpen, setBankOpen] = useState(false);
  const [bank, setBank] = useState(emptyBank);
  const [bankError, setBankError] = useState('');

  const load = useCallback(
    () =>
      api
        .get(`/payroll/employees/${employeeId}`)
        .then((r) => { setData(r.data); setError(''); })
        .catch((err) => setError(errMsg(err, 'Could not load salary details'))),
    [employeeId],
  );
  useEffect(() => { load(); }, [load]);

  // ─── Salary ───────────────────────────────────────────────────────────────

  const openNew = () => {
    // Start an increment from the current structure so only the changed numbers need typing.
    const base = data?.current ? salaryToForm(data.current) : { ...emptySalaryForm, epfApplicable: !isContract };
    setForm({ ...base, effectiveFrom: '', reason: data?.current ? '' : 'Joined', notes: '' });
    setTarget(null);
    setFormError('');
    setMode('new');
  };

  const openEdit = (s: SalaryRecord) => {
    setForm(salaryToForm(s));
    setTarget(s);
    setFormError('');
    setMode('edit');
  };

  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    setBusy(true);
    setFormError('');
    try {
      if (mode === 'edit' && target) {
        await api.patch(`/payroll/employees/${employeeId}/salaries/${target.id}`, toSalaryBody(form));
      } else {
        await api.post(`/payroll/employees/${employeeId}/salaries`, toSalaryBody(form));
      }
      setMode('closed');
      await load();
    } catch (err) {
      setFormError(errMsg(err, 'Failed to save salary'));
    } finally {
      setBusy(false);
    }
  };

  const remove = async (s: SalaryRecord) => {
    if (!window.confirm(`Delete the salary record effective ${formatDate(s.effectiveFrom)}?`)) return;
    try {
      await api.delete(`/payroll/employees/${employeeId}/salaries/${s.id}`);
      await load();
    } catch (err) {
      alert(errMsg(err, 'Failed to delete salary record'));
    }
  };

  // ─── Bank ─────────────────────────────────────────────────────────────────

  const openBank = () => {
    if (!data) return;
    setBank({
      bankName: data.bank.bankName ?? '',
      bankBranch: data.bank.bankBranch ?? '',
      bankAccountNo: data.bank.bankAccountNo ?? '',
      staffLoanBalance: data.staffLoanBalance ? String(data.staffLoanBalance) : '',
    });
    setBankError('');
    setBankOpen(true);
  };

  const saveBank = async (e: React.FormEvent) => {
    e.preventDefault();
    setBankError('');
    try {
      await api.patch(`/payroll/employees/${employeeId}/bank`, {
        bankName: bank.bankName.trim() || null,
        bankBranch: bank.bankBranch.trim() || null,
        bankAccountNo: bank.bankAccountNo.trim() || null,
        staffLoanBalance: bank.staffLoanBalance.trim() === '' ? 0 : Number(bank.staffLoanBalance),
      });
      setBankOpen(false);
      await load();
    } catch (err) {
      setBankError(errMsg(err, 'Failed to save bank details'));
    }
  };

  // ─── Render ───────────────────────────────────────────────────────────────

  if (error) return <div className="card mb-5 text-sm text-red-600">{error}</div>;
  if (!data) return null;

  const cur = data.current;
  const setNum = (key: keyof SalaryForm) => (e: React.ChangeEvent<HTMLInputElement>) =>
    setForm({ ...form, [key]: e.target.value });

  return (
    <div className="mb-6">
      <div className="flex items-center justify-between mb-3">
        <h2 className="section-title mb-0">Salary &amp; payroll</h2>
        <div className="flex gap-2">
          <button onClick={openBank} className="btn btn-secondary">Bank details</button>
          <button onClick={openNew} className="btn btn-primary">
            {cur ? '+ Add increment' : '+ Set salary'}
          </button>
        </div>
      </div>

      {/* Current salary + bank summary */}
      <div className="grid grid-cols-1 lg:grid-cols-3 gap-4 mb-4">
        <div className="card lg:col-span-2">
          {cur ? (
            <>
              <div className="flex items-baseline justify-between mb-3">
                <div>
                  <div className="text-xs text-gray-500">Monthly salary (fixed components)</div>
                  <div className="text-2xl font-bold text-gray-900">{rs(cur.monthlyTotal)}</div>
                </div>
                <div className="text-right text-xs text-gray-500">
                  Effective {formatDate(cur.effectiveFrom)}
                  {cur.reason && <div className="text-gray-400">{cur.reason}</div>}
                </div>
              </div>
              <dl className="grid grid-cols-2 sm:grid-cols-3 gap-x-6 gap-y-2 text-sm">
                <Item label="Basic" value={rs(cur.basic)} />
                <Item label="BRA 2005" value={rs(cur.bra2005)} />
                <Item label="BRA 2016" value={rs(cur.bra2016)} />
                <Item label="Vehicle allowance" value={rs(cur.vehicleAllowance)} />
                <Item label="Other allowance" value={rs(cur.otherAllowance)} />
                <Item label="EPF / ETF base" value={cur.epfApplicable ? rs(cur.epfBase) : 'Not applicable'} />
                <Item label="Welfare deduction" value={rs(cur.welfareDeduction)} />
                <Item label="Loan instalment" value={rs(cur.loanInstallment)} />
                <Item label="Est. EPF 8% / 12% / ETF 3%" value={
                  cur.epfApplicable
                    ? `${rs(cur.epfBase * 0.08)} / ${rs(cur.epfBase * 0.12)} / ${rs(cur.epfBase * 0.03)}`
                    : '—'
                } />
              </dl>
              {data.upcoming && (
                <p className="mt-3 text-xs text-blue-700 bg-blue-50 rounded px-2 py-1.5">
                  Increment scheduled: {rs(data.upcoming.monthlyTotal)} from {formatDate(data.upcoming.effectiveFrom)}
                  {data.upcoming.changePercent !== null && ` (${data.upcoming.changePercent > 0 ? '+' : ''}${data.upcoming.changePercent}%)`}
                </p>
              )}
            </>
          ) : (
            <p className="text-sm text-gray-500">No salary set yet. Use “Set salary” to record the starting salary.</p>
          )}
        </div>

        <div className="card">
          <div className="text-xs font-semibold text-gray-500 uppercase tracking-wide mb-2">Bank transfer</div>
          <dl className="text-sm space-y-1.5">
            <Item label="Bank" value={data.bank.bankName || '—'} />
            <Item label="Branch" value={data.bank.bankBranch || '—'} />
            <Item label="Account no." value={data.bank.bankAccountNo || '—'} mono />
            <Item label="Staff loan balance" value={rs(data.staffLoanBalance)} />
          </dl>
        </div>
      </div>

      {bankOpen && (
        <div className="card mb-4">
          <form onSubmit={saveBank} className="form-grid" style={{ gridTemplateColumns: '1fr 1fr' }}>
            <label className="block">
              <span className="text-xs font-medium text-gray-500">Bank</span>
              <input placeholder="e.g. Sampath Bank" value={bank.bankName} onChange={(e) => setBank({ ...bank, bankName: e.target.value })} />
            </label>
            <label className="block">
              <span className="text-xs font-medium text-gray-500">Branch</span>
              <input placeholder="e.g. Borella Super" value={bank.bankBranch} onChange={(e) => setBank({ ...bank, bankBranch: e.target.value })} />
            </label>
            <label className="block">
              <span className="text-xs font-medium text-gray-500">Account number</span>
              <input value={bank.bankAccountNo} onChange={(e) => setBank({ ...bank, bankAccountNo: e.target.value })} />
            </label>
            <label className="block">
              <span className="text-xs font-medium text-gray-500">Staff loan balance ({region().currencySymbol})</span>
              <input type="number" min="0" step="0.01" value={bank.staffLoanBalance} onChange={(e) => setBank({ ...bank, staffLoanBalance: e.target.value })} />
            </label>
            {bankError && <p className="text-sm text-red-600" style={{ gridColumn: '1 / -1' }}>{bankError}</p>}
            <div className="flex gap-2" style={{ gridColumn: '1 / -1' }}>
              <button type="submit" className="btn btn-primary">Save</button>
              <button type="button" onClick={() => setBankOpen(false)} className="btn btn-secondary">Cancel</button>
            </div>
          </form>
        </div>
      )}

      {mode !== 'closed' && (
        <div className="card mb-4">
          <p className="text-sm font-medium text-gray-800 mb-3">
            {mode === 'edit' ? `Edit salary effective ${formatDate(target?.effectiveFrom ?? null)}` : cur ? 'Add an increment' : 'Set the starting salary'}
          </p>
          <form onSubmit={submit} className="form-grid" style={{ gridTemplateColumns: '1fr 1fr 1fr' }}>
            <label className="block">
              <span className="text-xs font-medium text-gray-500">Effective from *</span>
              <input type="date" required value={form.effectiveFrom} onChange={setNum('effectiveFrom')} />
            </label>
            <label className="block">
              <span className="text-xs font-medium text-gray-500">Reason</span>
              <input placeholder="e.g. Annual increment 2026" value={form.reason} onChange={setNum('reason')} />
            </label>
            <label className="flex items-end gap-2 text-sm text-gray-700 pb-2">
              <input type="checkbox" checked={form.epfApplicable} onChange={(e) => setForm({ ...form, epfApplicable: e.target.checked })} />
              EPF / ETF applies
            </label>

            <Money label="Basic *" value={form.basic} onChange={setNum('basic')} required />
            <Money label="BRA 2005" value={form.bra2005} onChange={setNum('bra2005')} />
            <Money label="BRA 2016" value={form.bra2016} onChange={setNum('bra2016')} />
            <Money label="Vehicle allowance" value={form.vehicleAllowance} onChange={setNum('vehicleAllowance')} />
            <Money label="Other allowance" value={form.otherAllowance} onChange={setNum('otherAllowance')} />
            <Money label="Welfare deduction / month" value={form.welfareDeduction} onChange={setNum('welfareDeduction')} />
            <Money label="Loan instalment / month" value={form.loanInstallment} onChange={setNum('loanInstallment')} />

            <label className="block" style={{ gridColumn: '2 / -1' }}>
              <span className="text-xs font-medium text-gray-500">Notes</span>
              <input placeholder="Optional" value={form.notes} onChange={setNum('notes')} />
            </label>

            <p className="text-sm text-gray-600" style={{ gridColumn: '1 / -1' }}>
              Monthly total: <span className="font-semibold">{rs(toSalaryBody(form).basic + toSalaryBody(form).bra2005 + toSalaryBody(form).bra2016 + toSalaryBody(form).vehicleAllowance + toSalaryBody(form).otherAllowance)}</span>
              {cur && mode === 'new' && (() => {
                const b = toSalaryBody(form);
                const total = b.basic + b.bra2005 + b.bra2016 + b.vehicleAllowance + b.otherAllowance;
                const diff = total - cur.monthlyTotal;
                const pct = cur.monthlyTotal ? (diff / cur.monthlyTotal) * 100 : 0;
                return <span className={`ml-2 ${diff >= 0 ? 'text-green-700' : 'text-red-600'}`}>({diff >= 0 ? '+' : ''}{rs(diff)}, {pct.toFixed(1)}%)</span>;
              })()}
            </p>
            {formError && <p className="text-sm text-red-600" style={{ gridColumn: '1 / -1' }}>{formError}</p>}
            <div className="flex gap-2" style={{ gridColumn: '1 / -1' }}>
              <button type="submit" className="btn btn-primary" disabled={busy}>{busy ? 'Saving…' : 'Save'}</button>
              <button type="button" onClick={() => setMode('closed')} className="btn btn-secondary">Cancel</button>
            </div>
          </form>
        </div>
      )}

      <Table
        data={data.history}
        emptyMessage="No salary history yet."
        columns={[
          {
            header: 'Effective',
            accessor: (s) => (
              <div>
                <div className="font-medium text-gray-900">{formatDate(s.effectiveFrom)}</div>
                <span className={`px-1.5 py-0.5 rounded text-[11px] font-medium ${STATE_BADGE[s.state].cls}`}>{STATE_BADGE[s.state].label}</span>
              </div>
            ),
          },
          { header: 'Basic', accessor: (s) => rs(s.basic) },
          { header: 'Allowances', accessor: (s) => rs(s.bra2005 + s.bra2016 + s.vehicleAllowance + s.otherAllowance) },
          { header: 'Monthly total', accessor: (s) => <span className="font-semibold">{rs(s.monthlyTotal)}</span> },
          {
            header: 'Change',
            accessor: (s) =>
              s.changeAmount === null ? (
                <span className="text-gray-400">—</span>
              ) : (
                <span className={s.changeAmount >= 0 ? 'text-green-700' : 'text-red-600'}>
                  {s.changeAmount >= 0 ? '+' : ''}{rs(s.changeAmount)}
                  {s.changePercent !== null && <span className="text-xs ml-1">({s.changePercent > 0 ? '+' : ''}{s.changePercent}%)</span>}
                </span>
              ),
          },
          { header: 'EPF', accessor: (s) => (s.epfApplicable ? 'Yes' : 'No') },
          {
            header: 'Reason',
            accessor: (s) => (
              <div className="text-xs">
                <div>{s.reason || '—'}</div>
                <div className="text-gray-400">{s.createdByName ? `by ${s.createdByName}` : ''}</div>
              </div>
            ),
          },
          {
            header: '',
            accessor: (s) => (
              <ActionMenu
                items={[
                  { label: 'Edit', onSelect: () => openEdit(s) },
                  { label: 'Delete', danger: true, onSelect: () => remove(s) },
                ]}
              />
            ),
          },
        ]}
      />
    </div>
  );
}

function Item({ label, value, mono = false }: { label: string; value: string; mono?: boolean }) {
  return (
    <div>
      <dt className="text-xs text-gray-500">{label}</dt>
      <dd className={`text-gray-800 ${mono ? 'font-mono' : ''}`}>{value}</dd>
    </div>
  );
}

function Money({
  label, value, onChange, required = false,
}: { label: string; value: string; onChange: (e: React.ChangeEvent<HTMLInputElement>) => void; required?: boolean }) {
  return (
    <label className="block">
      <span className="text-xs font-medium text-gray-500">{label}</span>
      <input type="number" min="0" step="0.01" required={required} placeholder="0.00" value={value} onChange={onChange} />
    </label>
  );
}
