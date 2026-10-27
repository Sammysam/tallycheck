import { useCallback, useEffect, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import api, { errMsg } from '../api/client';
import PageHeader from '../components/PageHeader';
import Table from '../components/Table';
import StateTaxRules from '../components/StateTaxRules';
import StatusBadge from '../components/StatusBadge';
import { fmtDate } from '../lib/region';
import { PAY_FREQUENCY_LABEL, US_STATES, usd, type PayFrequency, type UsRun, type UsSettings } from '../lib/usPayroll';

interface LedgerAccount { id: string; code: string; name: string; type: string; currency: string | null }

const iso = (d: Date) => d.toISOString().slice(0, 10);
const addDays = (s: string, days: number) => {
  const d = new Date(`${s}T00:00:00Z`);
  d.setUTCDate(d.getUTCDate() + days);
  return iso(d);
};

/** A sensible next pay period: the day after the last run, sized by the pay frequency. */
function nextPeriod(runs: UsRun[], freq: PayFrequency) {
  const live = runs.filter((r) => r.status !== 'VOID').sort((a, b) => b.periodEnd.localeCompare(a.periodEnd));
  const start = live.length ? addDays(live[0].periodEnd, 1) : iso(new Date());
  let end: string;
  if (freq === 'WEEKLY') end = addDays(start, 6);
  else if (freq === 'BIWEEKLY') end = addDays(start, 13);
  else {
    const [y, m, d] = start.split('-').map(Number);
    if (freq === 'SEMIMONTHLY') end = d <= 15 ? `${start.slice(0, 8)}15` : iso(new Date(Date.UTC(y, m, 0)));
    else end = iso(new Date(Date.UTC(y, m, d - 1)));
  }
  return { periodStart: start, periodEnd: end, payDate: addDays(end, 5) };
}

export default function UsPayroll() {
  const navigate = useNavigate();
  const [settings, setSettings] = useState<UsSettings | null>(null);
  const [runs, setRuns] = useState<UsRun[]>([]);
  const [error, setError] = useState('');

  const [form, setForm] = useState<Record<string, string>>({});
  const [showSettings, setShowSettings] = useState(false);
  const [settingsError, setSettingsError] = useState('');
  const [saved, setSaved] = useState(false);

  // D43: the account each role posts to ('' = the default code).
  const [roleAccounts, setRoleAccounts] = useState<Record<string, string>>({});
  const [ledgerAccounts, setLedgerAccounts] = useState<LedgerAccount[]>([]);

  const [period, setPeriod] = useState({ periodStart: '', periodEnd: '', payDate: '' });
  const [creating, setCreating] = useState(false);
  const [createError, setCreateError] = useState('');

  const fill = (s: UsSettings) => {
    setSettings(s);
    setForm({
      country: s.country,
      payFrequency: s.payFrequency,
      state: s.state ?? '',
      sutaRate: String(s.sutaRate),
      sutaWageBase: s.sutaWageBase === null ? '' : String(s.sutaWageBase),
      futaCreditReduction: String(s.futaCreditReduction),
      ein: s.ein ?? '',
      stateAccountNo: s.stateAccountNo ?? '',
    });
    setRoleAccounts(Object.fromEntries(s.accounts.map((a) => [a.role, a.accountId ?? ''])));
  };

  const load = useCallback(
    () =>
      Promise.all([api.get<UsSettings>('/payroll/us/settings'), api.get<UsRun[]>('/payroll/us/runs')])
        .then(([s, r]) => {
          fill(s.data);
          setRuns(r.data);
          setPeriod(nextPeriod(r.data, s.data.payFrequency));
          if (s.data.country !== 'US' || !s.data.state) setShowSettings(true);
          setError('');
        })
        .then(() => api.get<LedgerAccount[]>('/accounts'))
        .then((a) => setLedgerAccounts(a.data))
        .catch((err) => setError(errMsg(err, 'Could not load US payroll'))),
    [],
  );
  useEffect(() => { load(); }, [load]);

  const saveSettings = async (e: React.FormEvent) => {
    e.preventDefault();
    setSettingsError('');
    setSaved(false);
    try {
      const { data } = await api.patch<UsSettings>('/payroll/us/settings', {
        country: form.country,
        payFrequency: form.payFrequency,
        state: form.state || null,
        sutaRate: Number(form.sutaRate || 0),
        sutaWageBase: form.sutaWageBase.trim() === '' ? null : Number(form.sutaWageBase),
        futaCreditReduction: Number(form.futaCreditReduction || 0),
        ein: form.ein.trim() || null,
        stateAccountNo: form.stateAccountNo.trim() || null,
        accounts: Object.fromEntries(Object.entries(roleAccounts).map(([role, id]) => [role, id || null])),
      });
      fill(data);
      setPeriod(nextPeriod(runs, data.payFrequency));
      setSaved(true);
      if (data.country !== 'US') navigate('/payroll');
    } catch (err) {
      setSettingsError(errMsg(err, 'Could not save the settings'));
    }
  };

  const createRun = async (e: React.FormEvent) => {
    e.preventDefault();
    setCreating(true);
    setCreateError('');
    try {
      const { data } = await api.post('/payroll/us/runs', period);
      navigate(`/payroll/us/${data.id}`, { state: { skipped: data.skipped } });
    } catch (err) {
      setCreateError(errMsg(err, 'Could not start the pay run'));
    } finally {
      setCreating(false);
    }
  };

  const set = (k: string) => (e: React.ChangeEvent<HTMLInputElement | HTMLSelectElement>) => {
    setSaved(false);
    setForm({ ...form, [k]: e.target.value });
  };
  const isUs = settings?.country === 'US';

  return (
    <div>
      <PageHeader
        title="US payroll"
        action={
          <>
            <button onClick={() => navigate('/payroll/us/forms')} className="btn btn-secondary">Tax forms (941 / 940 / W-2)</button>
            <button onClick={() => setShowSettings(!showSettings)} className="btn btn-secondary">
              {showSettings ? 'Hide settings' : 'Payroll settings'}
            </button>
          </>
        }
      />

      {error && <div className="card mb-4 text-sm text-red-600">{error}</div>}

      {showSettings && settings && (
        <div className="card mb-5">
          <h3 className="text-sm font-semibold text-gray-700 mb-1">Company payroll settings</h3>
          <p className="text-xs text-gray-500 mb-3">
            Federal income tax, social security, Medicare and FUTA use the IRS figures for {settings.taxYears.join(', ')}.
            State unemployment uses the rate your state sends you each year. State income tax rules are set per state below.
          </p>
          <form onSubmit={saveSettings}>
            <div className="form-grid mb-3" style={{ gridTemplateColumns: 'repeat(3, 1fr)' }}>
              <label className="block">
                <span className="text-xs font-medium text-gray-500">Payroll country</span>
                <select value={form.country} onChange={set('country')}>
                  <option value="US">United States</option>
                  <option value="LK">Sri Lanka (EPF / ETF / PAYE)</option>
                </select>
              </label>
              <label className="block">
                <span className="text-xs font-medium text-gray-500">Pay frequency</span>
                <select value={form.payFrequency} onChange={set('payFrequency')}>
                  {(Object.keys(PAY_FREQUENCY_LABEL) as PayFrequency[]).map((f) => <option key={f} value={f}>{PAY_FREQUENCY_LABEL[f]}</option>)}
                </select>
              </label>
              <label className="block">
                <span className="text-xs font-medium text-gray-500">State (for unemployment tax)</span>
                <select value={form.state} onChange={set('state')}>
                  <option value="">Choose a state</option>
                  {US_STATES.map((s) => <option key={s} value={s}>{s}</option>)}
                </select>
              </label>
              <label className="block">
                <span className="text-xs font-medium text-gray-500">SUTA rate %</span>
                <input type="number" min="0" max="100" step="0.0001" value={form.sutaRate} onChange={set('sutaRate')} />
              </label>
              <label className="block">
                <span className="text-xs font-medium text-gray-500">SUTA wage base $ (blank = no limit)</span>
                <input type="number" min="0" step="0.01" value={form.sutaWageBase} onChange={set('sutaWageBase')} />
              </label>
              <label className="block">
                <span className="text-xs font-medium text-gray-500">FUTA credit reduction % (usually 0)</span>
                <input type="number" min="0" max="5.4" step="0.0001" value={form.futaCreditReduction} onChange={set('futaCreditReduction')} />
              </label>
              <label className="block">
                <span className="text-xs font-medium text-gray-500">Employer ID number (EIN)</span>
                <input placeholder="12-3456789" value={form.ein} onChange={set('ein')} />
              </label>
              <label className="block">
                <span className="text-xs font-medium text-gray-500">State unemployment account no.</span>
                <input value={form.stateAccountNo} onChange={set('stateAccountNo')} />
              </label>
            </div>
            <h4 className="text-xs font-semibold text-gray-700 mb-1">Accounts</h4>
            <p className="text-xs text-gray-500 mb-2">
              Where each part of a pay run is booked. Left on the default, the account with that code is used, or added
              if the code is free. If the code already belongs to another account (for example your own sales tax account),
              choose the account here.
            </p>
            <div className="form-grid mb-3" style={{ gridTemplateColumns: 'repeat(3, 1fr)' }}>
              {settings.accounts.map((r) => (
                <label key={r.role} className="block">
                  <span className="text-xs font-medium text-gray-500">{r.label}</span>
                  <select
                    value={roleAccounts[r.role] ?? ''}
                    onChange={(e) => { setSaved(false); setRoleAccounts({ ...roleAccounts, [r.role]: e.target.value }); }}
                  >
                    <option value="">Default: {r.defaultCode} {r.defaultName}</option>
                    {ledgerAccounts
                      .filter((a) => a.type === r.type && !a.currency)
                      .map((a) => <option key={a.id} value={a.id}>{a.code} {a.name}</option>)}
                  </select>
                </label>
              ))}
            </div>
            {settingsError && <p className="text-sm text-red-600 mb-2">{settingsError}</p>}
            {saved && <p className="text-sm text-green-600 mb-2">Settings saved.</p>}
            <button type="submit" className="btn btn-primary">Save settings</button>
          </form>
        </div>
      )}

      {showSettings && isUs && <StateTaxRules />}

      {isUs && (
        <div className="card mb-5">
          <form onSubmit={createRun} className="flex flex-wrap items-end gap-3">
            <label className="block">
              <span className="text-xs font-medium text-gray-500">Period from</span>
              <input type="date" required value={period.periodStart} onChange={(e) => setPeriod({ ...period, periodStart: e.target.value })} />
            </label>
            <label className="block">
              <span className="text-xs font-medium text-gray-500">to</span>
              <input type="date" required value={period.periodEnd} onChange={(e) => setPeriod({ ...period, periodEnd: e.target.value })} />
            </label>
            <label className="block">
              <span className="text-xs font-medium text-gray-500">Pay date</span>
              <input type="date" required value={period.payDate} onChange={(e) => setPeriod({ ...period, payDate: e.target.value })} />
            </label>
            <button type="submit" className="btn btn-primary" disabled={creating}>{creating ? 'Starting…' : 'Start pay run'}</button>
            <span className="text-xs text-gray-500">
              Adds every active employee with US pay and Form W-4 details. Taxes follow the pay date's year.
            </span>
          </form>
          {createError && <p className="text-sm text-red-600 mt-2">{createError}</p>}
        </div>
      )}

      <Table
        data={runs}
        emptyMessage={isUs ? 'No US pay runs yet. Start one above.' : 'Switch the payroll country to United States to start US pay runs.'}
        columns={[
          {
            header: 'Pay period',
            accessor: (r) => (
              <button className="text-left group" onClick={() => navigate(`/payroll/us/${r.id}`)}>
                <div className="font-medium text-gray-900 group-hover:text-blue-700">{fmtDate(r.periodStart)} – {fmtDate(r.periodEnd)}</div>
                <div className="text-xs text-gray-400 font-mono">{r.runNumber}</div>
              </button>
            ),
          },
          { header: 'Pay date', accessor: (r) => fmtDate(r.payDate) },
          { header: 'Status', accessor: (r) => <StatusBadge status={r.status} /> },
          { header: 'Employees', accessor: (r) => String(r.employeeCount) },
          { header: 'Gross', accessor: (r) => usd(r.grossTotal) },
          { header: 'Net pay', accessor: (r) => <span className="font-semibold">{usd(r.netPayTotal)}</span> },
          { header: 'Employer taxes', accessor: (r) => usd(r.employerTaxesTotal) },
          { header: '', accessor: (r) => <button onClick={() => navigate(`/payroll/us/${r.id}`)} className="btn-link text-xs">Open</button> },
        ]}
      />
    </div>
  );
}
