import { useCallback, useEffect, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import api, { errMsg } from '../api/client';
import PageHeader from '../components/PageHeader';
import Table from '../components/Table';
import StatusBadge from '../components/StatusBadge';
import { rs } from '../lib/employees';
import { amt, periodLabel, type CompanyDetails, type PayeSlab, type PayrollRates, type PayrollRun } from '../lib/payroll';
import { region } from '../lib/region';
import { usePayrollCountry } from '../lib/usPayroll';

const COMPANY_FIELDS: { key: keyof CompanyDetails; label: string; hint?: string }[] = [
  { key: 'companyName', label: 'Company name (as printed)' },
  { key: 'epfRegNo', label: 'EPF registration no.' },
  { key: 'etfRegNo', label: 'ETF registration no.' },
  { key: 'careOf', label: 'Care of (C/O)' },
  { key: 'epfAddress', label: 'Address on EPF C Form' },
  { key: 'etfAddress', label: 'Address on ETF form' },
  { key: 'telephone', label: 'Telephone' },
  { key: 'hrSignatory', label: 'Signature line on slips', hint: 'e.g. HR' },
];

const thisMonth = () => new Date().toISOString().slice(0, 7);

export default function Payroll() {
  const navigate = useNavigate();
  const [runs, setRuns] = useState<PayrollRun[]>([]);
  const [error, setError] = useState('');

  const [period, setPeriod] = useState(thisMonth());
  const [creating, setCreating] = useState(false);
  const [createError, setCreateError] = useState('');

  const [showRates, setShowRates] = useState(false);
  const [rates, setRates] = useState<PayrollRates | null>(null);
  const [ratesForm, setRatesForm] = useState<Record<string, string>>({});
  const [slabs, setSlabs] = useState<{ upper: string; rate: string }[]>([]);
  const [ratesError, setRatesError] = useState('');
  const [ratesSaved, setRatesSaved] = useState(false);
  const [companyForm, setCompanyForm] = useState<Record<string, string>>({});
  const [companyError, setCompanyError] = useState('');
  const [companySaved, setCompanySaved] = useState(false);

  // A company on US payroll works on the US screens.
  const country = usePayrollCountry();
  useEffect(() => {
    if (country === 'US') navigate('/payroll/us', { replace: true });
  }, [country, navigate]);

  const load = useCallback(
    () =>
      api
        .get('/payroll/runs')
        .then((r) => { setRuns(r.data); setError(''); })
        .catch((err) => setError(errMsg(err, 'Could not load payroll runs'))),
    [],
  );
  useEffect(() => { load(); }, [load]);

  const createRun = async (e: React.FormEvent) => {
    e.preventDefault();
    setCreating(true);
    setCreateError('');
    try {
      const { data } = await api.post('/payroll/runs', { period });
      navigate(`/payroll/${data.id}`, { state: { skipped: data.skipped } });
    } catch (err) {
      setCreateError(errMsg(err, 'Could not create the run'));
    } finally {
      setCreating(false);
    }
  };

  const openRates = async () => {
    setRatesError('');
    setRatesSaved(false);
    try {
      const { data } = await api.get<PayrollRates>('/payroll/rates');
      setRates(data);
      setRatesForm({
        epfEmployeeRate: String(data.epfEmployeeRate * 100),
        epfEmployerRate: String(data.epfEmployerRate * 100),
        etfRate: String(data.etfRate * 100),
        stampDuty: String(data.stampDuty),
        employerWelfare: String(data.employerWelfare),
        daysInMonth: String(data.daysInMonth),
      });
      setSlabs(data.payeSlabs.map(([u, r]) => ({ upper: u === null ? '' : String(u), rate: String(r * 100) })));
      const c = await api.get<CompanyDetails>('/payroll/company');
      setCompanyForm(Object.fromEntries(COMPANY_FIELDS.map((f) => [f.key, c.data[f.key] ?? ''])));
      setShowRates(true);
    } catch (err) {
      setRatesError(errMsg(err, 'Could not load rates'));
      setShowRates(true);
    }
  };

  const saveRates = async (e: React.FormEvent) => {
    e.preventDefault();
    setRatesError('');
    setRatesSaved(false);
    const pct = (s: string) => Number(s) / 100;
    const payeSlabs: PayeSlab[] = slabs.map((s, i) => [
      s.upper.trim() === '' && i === slabs.length - 1 ? null : Number(s.upper),
      pct(s.rate),
    ]);
    try {
      const { data } = await api.patch<PayrollRates>('/payroll/rates', {
        epfEmployeeRate: pct(ratesForm.epfEmployeeRate),
        epfEmployerRate: pct(ratesForm.epfEmployerRate),
        etfRate: pct(ratesForm.etfRate),
        stampDuty: Number(ratesForm.stampDuty),
        employerWelfare: Number(ratesForm.employerWelfare),
        daysInMonth: Number(ratesForm.daysInMonth),
        payeSlabs,
      });
      setRates(data);
      setRatesSaved(true);
    } catch (err) {
      setRatesError(errMsg(err, 'Could not save rates'));
    }
  };

  const saveCompany = async (e: React.FormEvent) => {
    e.preventDefault();
    setCompanyError('');
    setCompanySaved(false);
    try {
      await api.patch('/payroll/company', companyForm);
      setCompanySaved(true);
    } catch (err) {
      setCompanyError(errMsg(err, 'Could not save company details'));
    }
  };

  const setR = (k: string) => (e: React.ChangeEvent<HTMLInputElement>) => {
    setRatesSaved(false);
    setRatesForm({ ...ratesForm, [k]: e.target.value });
  };

  return (
    <div>
      <PageHeader
        title="Payroll"
        action={
          <>
            <button onClick={() => navigate('/payroll/us')} className="btn btn-secondary">US payroll</button>
            <button onClick={showRates ? () => setShowRates(false) : openRates} className="btn btn-secondary">
              {showRates ? 'Hide rates' : 'Rates & PAYE slabs'}
            </button>
          </>
        }
      />

      {error && <div className="card mb-4 text-sm text-red-600">{error}</div>}

      {showRates && (
        <div className="card mb-5">
          <h3 className="text-sm font-semibold text-gray-700 mb-1">Statutory rates</h3>
          <p className="text-xs text-gray-500 mb-3">
            Used for new runs. Existing runs keep the rates they were created with.
            {rates && ` PAYE is progressive on gross salary; the first slab is ${rates.payeSlabs[0][1] * 100}% up to Rs. ${amt(rates.payeSlabs[0][0])}.`}
          </p>
          <form onSubmit={saveRates}>
            <div className="form-grid mb-3" style={{ gridTemplateColumns: 'repeat(3, 1fr)' }}>
              <Pct label="EPF employee %" value={ratesForm.epfEmployeeRate ?? ''} onChange={setR('epfEmployeeRate')} />
              <Pct label="EPF employer %" value={ratesForm.epfEmployerRate ?? ''} onChange={setR('epfEmployerRate')} />
              <Pct label="ETF employer %" value={ratesForm.etfRate ?? ''} onChange={setR('etfRate')} />
              <Pct label={`Stamp duty (${region().currencySymbol}) per slip`} value={ratesForm.stampDuty ?? ''} onChange={setR('stampDuty')} step="1" />
              <Pct label={`Employer welfare (${region().currencySymbol}) per employee`} value={ratesForm.employerWelfare ?? ''} onChange={setR('employerWelfare')} step="1" />
              <Pct label="Days in month (for proration)" value={ratesForm.daysInMonth ?? ''} onChange={setR('daysInMonth')} step="1" />
            </div>

            <p className="text-xs font-semibold text-gray-500 uppercase tracking-wide mb-1">PAYE slabs (monthly, progressive)</p>
            <table className="text-sm mb-2">
              <thead>
                <tr className="text-xs text-gray-500">
                  <th className="text-left pr-3 py-1">From</th>
                  <th className="text-left pr-3 py-1">Up to (blank = no limit, last row only)</th>
                  <th className="text-left pr-3 py-1">Rate %</th>
                  <th />
                </tr>
              </thead>
              <tbody>
                {slabs.map((s, i) => (
                  <tr key={i}>
                    <td className="pr-3 py-1 text-gray-500">{i === 0 ? '0' : amt(slabs[i - 1].upper || 0)}</td>
                    <td className="pr-3 py-1">
                      <input type="number" min="0" step="1" value={s.upper} style={{ width: 140 }}
                        onChange={(e) => { setRatesSaved(false); setSlabs(slabs.map((x, j) => (j === i ? { ...x, upper: e.target.value } : x))); }} />
                    </td>
                    <td className="pr-3 py-1">
                      <input type="number" min="0" max="100" step="0.5" value={s.rate} style={{ width: 90 }}
                        onChange={(e) => { setRatesSaved(false); setSlabs(slabs.map((x, j) => (j === i ? { ...x, rate: e.target.value } : x))); }} />
                    </td>
                    <td className="py-1">
                      {slabs.length > 1 && (
                        <button type="button" className="btn-link text-xs" style={{ color: '#dc2626' }}
                          onClick={() => { setRatesSaved(false); setSlabs(slabs.filter((_, j) => j !== i)); }}>Remove</button>
                      )}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
            <button type="button" className="btn-link text-xs mb-3" onClick={() => setSlabs([...slabs, { upper: '', rate: '' }])}>+ Add slab</button>

            {ratesError && <p className="text-sm text-red-600 mb-2">{ratesError}</p>}
            {ratesSaved && <p className="text-sm text-green-600 mb-2">Rates saved.</p>}
            <button type="submit" className="btn btn-primary">Save rates</button>
          </form>

          <hr className="my-5 border-gray-200" />
          <h3 className="text-sm font-semibold text-gray-700 mb-1">Company details for statutory forms</h3>
          <p className="text-xs text-gray-500 mb-3">Printed on the EPF C Form, ETF remittance advice, bank advice and salary slips.</p>
          <form onSubmit={saveCompany}>
            <div className="form-grid mb-3" style={{ gridTemplateColumns: 'repeat(2, 1fr)' }}>
              {COMPANY_FIELDS.map((f) => (
                <label key={f.key} className="block">
                  <span className="text-xs font-medium text-gray-500">{f.label}</span>
                  <input placeholder={f.hint} value={companyForm[f.key] ?? ''} onChange={(e) => { setCompanySaved(false); setCompanyForm({ ...companyForm, [f.key]: e.target.value }); }} />
                </label>
              ))}
            </div>
            {companyError && <p className="text-sm text-red-600 mb-2">{companyError}</p>}
            {companySaved && <p className="text-sm text-green-600 mb-2">Company details saved.</p>}
            <button type="submit" className="btn btn-primary">Save company details</button>
          </form>
        </div>
      )}

      <div className="card mb-5">
        <form onSubmit={createRun} className="flex flex-wrap items-end gap-3">
          <label className="block">
            <span className="text-xs font-medium text-gray-500">Month</span>
            <input type="month" required value={period} onChange={(e) => setPeriod(e.target.value)} />
          </label>
          <button type="submit" className="btn btn-primary" disabled={creating}>
            {creating ? 'Creating…' : `Start payroll for ${periodLabel(period)}`}
          </button>
          <span className="text-xs text-gray-500">
            Pulls every active employee with a salary in force for that month into a draft you can adjust.
          </span>
        </form>
        {createError && <p className="text-sm text-red-600 mt-2">{createError}</p>}
      </div>

      <Table
        data={runs}
        emptyMessage="No payroll runs yet. Start one for the current month above."
        columns={[
          {
            header: 'Month',
            accessor: (r) => (
              <button className="text-left group" onClick={() => navigate(`/payroll/${r.id}`)}>
                <div className="font-medium text-gray-900 group-hover:text-blue-700">{periodLabel(r.period)}</div>
                <div className="text-xs text-gray-400 font-mono">{r.runNumber}</div>
              </button>
            ),
          },
          { header: 'Status', accessor: (r) => <StatusBadge status={r.status} /> },
          { header: 'Employees', accessor: (r) => String(r.employeeCount) },
          { header: 'Gross', accessor: (r) => rs(r.grossTotal) },
          { header: 'Deductions', accessor: (r) => rs(r.deductionsTotal) },
          { header: 'Net pay', accessor: (r) => <span className="font-semibold">{rs(r.netTotal)}</span> },
          { header: 'Employer EPF/ETF', accessor: (r) => rs(Number(r.epfEmployerTotal) + Number(r.etfTotal)) },
          { header: 'Paid on', accessor: (r) => r.payDate || '—' },
          {
            header: '',
            accessor: (r) => <button onClick={() => navigate(`/payroll/${r.id}`)} className="btn-link text-xs">Open</button>,
          },
        ]}
      />
    </div>
  );
}

function Pct({ label, value, onChange, step = '0.5' }: { label: string; value: string; onChange: (e: React.ChangeEvent<HTMLInputElement>) => void; step?: string }) {
  return (
    <label className="block">
      <span className="text-xs font-medium text-gray-500">{label}</span>
      <input type="number" min="0" step={step} required value={value} onChange={onChange} />
    </label>
  );
}
