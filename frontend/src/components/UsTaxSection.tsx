import { useCallback, useEffect, useState } from 'react';
import api, { errMsg } from '../api/client';
import { FILING_LABEL, US_STATES, usd, type UsPreview, type UsTax } from '../lib/usPayroll';

const AMOUNTS = ['annualSalary', 'hourlyRate', 'dependentsCredit', 'otherIncome', 'deductions', 'extraWithholding', 'pretax401k', 'pretaxSection125'] as const;

/**
 * An employee's US pay and Form W-4, with a paycheck preview. Shown on the profile when the
 * company runs US payroll, for users with the `payroll` permission.
 */
export default function UsTaxSection({ employeeId }: { employeeId: string }) {
  const [tax, setTax] = useState<UsTax | null>(null);
  const [form, setForm] = useState<Record<string, string>>({});
  const [flags, setFlags] = useState({ multipleJobs: false, exemptFromWithholding: false, stateExempt: false });
  const [error, setError] = useState('');
  const [saved, setSaved] = useState(false);
  const [saving, setSaving] = useState(false);
  const [hours, setHours] = useState('80');
  const [preview, setPreview] = useState<UsPreview | null>(null);
  const [previewError, setPreviewError] = useState('');

  const fill = (t: UsTax) => {
    setTax(t);
    const f: Record<string, string> = {
      workState: t.workState ?? '',
      payType: t.payType,
      w4Version: t.w4Version,
      filingStatus: t.filingStatus,
      allowances: String(t.allowances),
      stateFilingStatus: t.stateFilingStatus,
      stateAllowances: String(t.stateAllowances),
      stateExtraWithholding: t.stateExtraWithholding ? String(t.stateExtraWithholding) : '',
    };
    for (const k of AMOUNTS) f[k] = t[k] ? String(t[k]) : '';
    setForm(f);
    setFlags({ multipleJobs: t.multipleJobs, exemptFromWithholding: t.exemptFromWithholding, stateExempt: t.stateExempt });
  };

  const load = useCallback(
    () => api.get<UsTax>(`/payroll/employees/${employeeId}/us-tax`).then((r) => fill(r.data)).catch((err) => setError(errMsg(err, 'Could not load the US pay details'))),
    [employeeId],
  );
  useEffect(() => { load(); }, [load]);

  const save = async (e: React.FormEvent) => {
    e.preventDefault();
    setSaving(true);
    setError('');
    setSaved(false);
    const num = (v: string) => (v.trim() === '' ? 0 : Number(v));
    try {
      const body: Record<string, unknown> = {
        workState: form.workState,
        payType: form.payType,
        w4Version: form.w4Version,
        filingStatus: form.filingStatus,
        allowances: num(form.allowances),
        stateFilingStatus: form.stateFilingStatus,
        stateAllowances: num(form.stateAllowances),
        stateExtraWithholding: num(form.stateExtraWithholding ?? ''),
        ...flags,
      };
      for (const k of AMOUNTS) body[k] = num(form[k] ?? '');
      const { data } = await api.put<UsTax>(`/payroll/employees/${employeeId}/us-tax`, body);
      fill(data);
      setSaved(true);
      setPreview(null);
    } catch (err) {
      setError(errMsg(err, 'Could not save'));
    } finally {
      setSaving(false);
    }
  };

  const runPreview = async () => {
    setPreviewError('');
    try {
      const { data } = await api.post<UsPreview>('/payroll/us/preview', {
        employeeId,
        ...(form.payType === 'HOURLY' ? { hours: Number(hours || 0) } : {}),
      });
      setPreview(data);
    } catch (err) {
      setPreview(null);
      setPreviewError(errMsg(err, 'Could not work out the paycheck'));
    }
  };

  const set = (k: string) => (e: React.ChangeEvent<HTMLInputElement | HTMLSelectElement>) => {
    setSaved(false);
    setForm({ ...form, [k]: e.target.value });
  };
  const legacy = form.w4Version === 'LEGACY';
  const hourly = form.payType === 'HOURLY';

  if (!tax) {
    return (
      <div className="card mb-5">
        <h2 className="section-title">US pay &amp; Form W-4</h2>
        {error ? <p className="text-sm text-red-600">{error}</p> : <p className="text-sm text-gray-400">Loading…</p>}
      </div>
    );
  }

  return (
    <div className="card mb-5">
      <div className="flex items-center justify-between mb-1">
        <h2 className="section-title mb-0">US pay &amp; Form W-4</h2>
        {!tax.configured && <span className="text-xs text-amber-700">Not set up — this employee is left out of pay runs</span>}
      </div>
      <p className="text-xs text-gray-500 mb-3">Copy the figures from the employee's Form W-4. Pre-tax deductions are taken from every paycheck.</p>

      <form onSubmit={save}>
        <p className="text-xs font-semibold text-gray-500 uppercase tracking-wide mb-1">Pay</p>
        <div className="form-grid mb-4" style={{ gridTemplateColumns: 'repeat(4, 1fr)' }}>
          <label className="block">
            <span className="text-xs font-medium text-gray-500">Works in state</span>
            <select required value={form.workState} onChange={set('workState')}>
              <option value="">Choose</option>
              {US_STATES.map((s) => <option key={s} value={s}>{s}</option>)}
            </select>
          </label>
          <label className="block">
            <span className="text-xs font-medium text-gray-500">Paid by</span>
            <select value={form.payType} onChange={set('payType')}>
              <option value="SALARY">Salary</option>
              <option value="HOURLY">The hour</option>
            </select>
          </label>
          {hourly ? <Money label="Hourly rate $" k="hourlyRate" form={form} set={set} required />
            : <Money label="Annual salary $" k="annualSalary" form={form} set={set} required />}
        </div>

        <p className="text-xs font-semibold text-gray-500 uppercase tracking-wide mb-1">Form W-4</p>
        <div className="form-grid mb-2" style={{ gridTemplateColumns: 'repeat(4, 1fr)' }}>
          <label className="block">
            <span className="text-xs font-medium text-gray-500">Form version</span>
            <select value={form.w4Version} onChange={set('w4Version')}>
              <option value="2020">2020 or later</option>
              <option value="LEGACY">2019 or earlier</option>
            </select>
          </label>
          <label className="block" style={{ gridColumn: 'span 2' }}>
            <span className="text-xs font-medium text-gray-500">{legacy ? 'Marital status (line 3)' : 'Filing status (Step 1c)'}</span>
            <select value={form.filingStatus} onChange={set('filingStatus')}>
              <option value="SINGLE">{legacy ? 'Single' : FILING_LABEL.SINGLE}</option>
              <option value="MARRIED_JOINTLY">{legacy ? 'Married' : FILING_LABEL.MARRIED_JOINTLY}</option>
              {!legacy && <option value="HEAD_OF_HOUSEHOLD">{FILING_LABEL.HEAD_OF_HOUSEHOLD}</option>}
            </select>
          </label>
          {legacy ? (
            <label className="block">
              <span className="text-xs font-medium text-gray-500">Allowances (line 5)</span>
              <input type="number" min="0" max="99" step="1" value={form.allowances} onChange={set('allowances')} />
            </label>
          ) : (
            <label className="flex items-center gap-2 text-sm mt-5">
              <input type="checkbox" checked={flags.multipleJobs} onChange={(e) => { setSaved(false); setFlags({ ...flags, multipleJobs: e.target.checked }); }} />
              Step 2(c) box checked
            </label>
          )}
          {!legacy && (
            <>
              <Money label="Step 3: dependents credit $ (a year)" k="dependentsCredit" form={form} set={set} />
              <Money label="Step 4(a): other income $ (a year)" k="otherIncome" form={form} set={set} />
              <Money label="Step 4(b): deductions $ (a year)" k="deductions" form={form} set={set} />
            </>
          )}
          <Money label={legacy ? 'Extra withholding $ per paycheck (line 6)' : 'Step 4(c): extra withholding $ per paycheck'} k="extraWithholding" form={form} set={set} />
        </div>
        <label className="flex items-center gap-2 text-sm mb-4">
          <input type="checkbox" checked={flags.exemptFromWithholding} onChange={(e) => { setSaved(false); setFlags({ ...flags, exemptFromWithholding: e.target.checked }); }} />
          Exempt from federal income tax withholding (claimed on the W-4; social security and Medicare still apply)
        </label>

        <p className="text-xs font-semibold text-gray-500 uppercase tracking-wide mb-1">State withholding certificate ({form.workState || 'work state'})</p>
        <div className="form-grid mb-2" style={{ gridTemplateColumns: 'repeat(4, 1fr)' }}>
          <label className="block">
            <span className="text-xs font-medium text-gray-500">State filing status</span>
            <select value={form.stateFilingStatus} onChange={set('stateFilingStatus')}>
              <option value="SINGLE">Single</option>
              <option value="MARRIED">Married</option>
            </select>
          </label>
          <label className="block">
            <span className="text-xs font-medium text-gray-500">State allowances</span>
            <input type="number" min="0" max="99" step="1" value={form.stateAllowances} onChange={set('stateAllowances')} />
          </label>
          <Money label="Extra state withholding $ per paycheck" k="stateExtraWithholding" form={form} set={set} />
        </div>
        <label className="flex items-center gap-2 text-sm mb-4">
          <input type="checkbox" checked={flags.stateExempt} onChange={(e) => { setSaved(false); setFlags({ ...flags, stateExempt: e.target.checked }); }} />
          Exempt from state income tax withholding
        </label>

        <p className="text-xs font-semibold text-gray-500 uppercase tracking-wide mb-1">Pre-tax deductions per paycheck</p>
        <div className="form-grid mb-3" style={{ gridTemplateColumns: 'repeat(4, 1fr)' }}>
          <Money label="401(k) $ (not income tax; still FICA)" k="pretax401k" form={form} set={set} />
          <Money label="Section 125 $ (not income tax or FICA)" k="pretaxSection125" form={form} set={set} />
        </div>

        {error && <p className="text-sm text-red-600 mb-2">{error}</p>}
        {saved && <p className="text-sm text-green-600 mb-2">Saved. New and refreshed pay run lines use these details.</p>}
        <button type="submit" className="btn btn-primary" disabled={saving}>{saving ? 'Saving…' : 'Save US pay & W-4'}</button>
      </form>

      {tax.configured && (
        <div className="mt-5 pt-4 border-t border-gray-100">
          <div className="flex flex-wrap items-end gap-3 mb-3">
            <span className="text-sm font-semibold text-gray-700">Paycheck preview</span>
            {hourly && (
              <label className="block">
                <span className="text-xs font-medium text-gray-500">Hours</span>
                <input type="number" min="0" step="0.01" value={hours} onChange={(e) => setHours(e.target.value)} style={{ width: 100 }} />
              </label>
            )}
            <button type="button" className="btn btn-secondary" onClick={runPreview}>Work it out</button>
            <span className="text-xs text-gray-500">Nothing is saved. Year-to-date wages are not counted here.</span>
          </div>
          {previewError && <p className="text-sm text-red-600">{previewError}</p>}
          {preview && (
            <div className="grid grid-cols-1 md:grid-cols-2 gap-4 text-sm">
              <dl className="space-y-1">
                <Row k="Gross pay" v={usd(preview.gross)} />
                {preview.pretax401k > 0 && <Row k="401(k)" v={`− ${usd(preview.pretax401k)}`} />}
                {preview.pretaxSection125 > 0 && <Row k="Section 125" v={`− ${usd(preview.pretaxSection125)}`} />}
                <Row k="Federal income tax" v={`− ${usd(preview.taxes.federalIncomeTax)}`} />
                <Row k="Social security 6.2%" v={`− ${usd(preview.taxes.socialSecurityEmployee)}`} />
                <Row k="Medicare 1.45%" v={`− ${usd(preview.taxes.medicareEmployee)}`} />
                {preview.taxes.additionalMedicare > 0 && <Row k="Additional Medicare 0.9%" v={`− ${usd(preview.taxes.additionalMedicare)}`} />}
                <Row k={`State income tax (${preview.stateTax.state})`} v={preview.stateTax.configured ? `− ${usd(preview.stateTax.tax)}` : 'not set up'} />
                <Row k="Net pay" v={usd(preview.taxes.netPay)} strong />
              </dl>
              <dl className="space-y-1">
                <Row k="Employer social security" v={usd(preview.taxes.socialSecurityEmployer)} />
                <Row k="Employer Medicare" v={usd(preview.taxes.medicareEmployer)} />
                <Row k="FUTA" v={usd(preview.taxes.futa)} />
                <Row k="State unemployment" v={usd(preview.taxes.suta)} />
                <Row k="Total cost to the company" v={usd(preview.taxes.costToEmployer)} strong />
                {preview.notes.map((note) => <p key={note} className="text-xs text-gray-500 pt-1">{note}</p>)}
              </dl>
            </div>
          )}
        </div>
      )}
    </div>
  );
}

function Money({ label, k, form, set, required = false }: {
  label: string; k: string; form: Record<string, string>; required?: boolean;
  set: (k: string) => (e: React.ChangeEvent<HTMLInputElement>) => void;
}) {
  return (
    <label className="block">
      <span className="text-xs font-medium text-gray-500">{label}</span>
      <input type="number" min="0" step="0.01" placeholder="0" required={required} value={form[k] ?? ''} onChange={set(k)} />
    </label>
  );
}

function Row({ k, v, strong = false }: { k: string; v: string; strong?: boolean }) {
  return (
    <div className={`flex justify-between ${strong ? 'font-semibold text-gray-900 border-t border-gray-200 pt-1' : 'text-gray-700'}`}>
      <dt>{k}</dt><dd className="tabular-nums">{v}</dd>
    </div>
  );
}
