import { useCallback, useEffect, useState } from 'react';
import api, { errMsg } from '../api/client';
import { askConfirm } from '../lib/confirm';
import { US_STATES, usd, type Locality } from '../lib/usPayroll';

/**
 * D69: the company's local (city / county) taxes. Built-in localities (New York City, Yonkers,
 * Philadelphia, Portland Metro / Multnomah, Colorado occupational privilege taxes, …) are added
 * with one click; any other city, county, school district or PSD tax is entered here. Each
 * employee's US tax section lists the localities withheld for.
 */

interface LedgerAccount { id: string; code: string; name: string; type: string; currency: string | null }

type Draft = {
  code: string; name: string; state: string; method: string; formula: string;
  residentRate: string; nonresidentRate: string; employerRate: string; exemption: string; secondExemption: string;
  flatEmployee: string; flatEmployer: string; monthlyThreshold: string; employeeYearCap: string;
  includes401k: boolean; includes125: boolean; group: string; accountId: string; source: string; verified: boolean;
};

const pct = (ppm: number) => (ppm ? String(ppm / 10000) : '');
const dollars = (cents: number | null) => (cents ? String(cents / 100) : '');

const draftOf = (l?: Locality): Draft => ({
  code: l?.code ?? '', name: l?.name ?? '', state: l?.state ?? '', method: l?.method ?? 'PERCENT', formula: l?.formula ?? '',
  residentRate: pct(l?.residentRate ?? 0), nonresidentRate: pct(l?.nonresidentRate ?? 0), employerRate: pct(l?.employerRate ?? 0),
  exemption: dollars(l?.exemption ?? 0), secondExemption: dollars(l?.secondExemption ?? 0),
  flatEmployee: dollars(l?.flatEmployee ?? 0), flatEmployer: dollars(l?.flatEmployer ?? 0),
  monthlyThreshold: dollars(l?.monthlyThreshold ?? 0), employeeYearCap: dollars(l?.employeeYearCap ?? null),
  includes401k: l?.includes401k ?? true, includes125: l?.includes125 ?? false, group: l?.group ?? '',
  accountId: l?.accountId ?? '', source: l?.source ?? '', verified: l?.verified ?? true,
});

const METHOD_LABEL: Record<string, string> = {
  PERCENT: 'Percentage of wages',
  MONTHLY_FLAT: 'Flat amount per month (occupational privilege tax)',
  WEEKLY_FLAT: 'Flat amount per week (city service fee)',
  PERIOD_FLAT: 'Flat amount per year, spread over pay periods (e.g. PA LST)',
  FORMULA: 'Built-in formula',
};

function describeLocality(l: Locality): string {
  switch (l.method) {
    case 'PERCENT': {
      const parts = [];
      if (l.residentRate) parts.push(`residents ${l.residentRate / 10000}%`);
      if (l.nonresidentRate) parts.push(`nonresidents ${l.nonresidentRate / 10000}%`);
      if (l.employerRate) parts.push(`employer ${l.employerRate / 10000}%`);
      return parts.join(', ') || 'no rates yet';
    }
    case 'MONTHLY_FLAT':
      return `${usd(l.flatEmployee / 100)} + employer ${usd(l.flatEmployer / 100)} a month from ${usd(l.monthlyThreshold / 100)} of monthly wages`;
    case 'WEEKLY_FLAT':
      return `${usd(l.flatEmployee / 100)} a week${l.flatEmployer ? ` + employer ${usd(l.flatEmployer / 100)}` : ''}`;
    case 'PERIOD_FLAT':
      return `${usd(l.flatEmployee / 100)} a year, spread over the pay periods`;
    default:
      return 'built-in 2026 formula';
  }
}

export default function UsLocalities({ accounts }: { accounts: LedgerAccount[] }) {
  const [mine, setMine] = useState<Locality[]>([]);
  const [catalog, setCatalog] = useState<Locality[]>([]);
  const [error, setError] = useState('');
  const [editing, setEditing] = useState<Draft | null>(null);
  const [isNew, setIsNew] = useState(false);
  const [saveError, setSaveError] = useState('');
  const [pick, setPick] = useState('');

  const load = useCallback(
    () => api.get<{ localities: Locality[]; catalog: Locality[] }>('/payroll/us/localities')
      .then((r) => { setMine(r.data.localities); setCatalog(r.data.catalog); setError(''); })
      .catch((err) => setError(errMsg(err, 'Could not load the local taxes'))),
    [],
  );
  useEffect(() => { load(); }, [load]);

  const save = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!editing) return;
    setSaveError('');
    const n = (v: string) => (v.trim() === '' ? 0 : Number(v));
    try {
      await api.put(`/payroll/us/localities/${encodeURIComponent(editing.code.trim().toUpperCase())}`, {
        name: editing.name, state: editing.state, method: editing.method, formula: editing.formula || null,
        residentRate: n(editing.residentRate), nonresidentRate: n(editing.nonresidentRate), employerRate: n(editing.employerRate),
        exemption: n(editing.exemption), secondExemption: n(editing.secondExemption),
        flatEmployee: n(editing.flatEmployee), flatEmployer: n(editing.flatEmployer), monthlyThreshold: n(editing.monthlyThreshold),
        employeeYearCap: editing.employeeYearCap.trim() === '' ? null : Number(editing.employeeYearCap),
        includes401k: editing.includes401k, includes125: editing.includes125, group: editing.group || null,
        accountId: editing.accountId || null, source: editing.source || null, verified: editing.verified,
      });
      setEditing(null);
      await load();
    } catch (err) {
      setSaveError(errMsg(err, 'Could not save the local tax'));
    }
  };

  const remove = async (code: string) => {
    if (!(await askConfirm(`Remove the ${code} local tax?`))) return;
    try {
      await api.delete(`/payroll/us/localities/${encodeURIComponent(code)}`);
      await load();
    } catch (err) {
      setError(errMsg(err, 'Could not remove the local tax'));
    }
  };

  const liabilities = accounts.filter((a) => a.type === 'LIABILITY' && !a.currency);
  const available = catalog.filter((c) => !mine.some((m) => m.code === c.code));
  const set = (k: keyof Draft) => (e: React.ChangeEvent<HTMLInputElement | HTMLSelectElement>) =>
    editing && setEditing({ ...editing, [k]: e.target.type === 'checkbox' ? (e.target as HTMLInputElement).checked : e.target.value });
  const field = (label: string, k: keyof Draft, type = 'number') => (
    <label className="block"><span className="text-xs font-medium text-gray-500">{label}</span>
      <input type={type} min="0" step="0.0001" value={String(editing?.[k] ?? '')} onChange={set(k)} aria-label={label} /></label>
  );

  return (
    <div className="card mb-5" aria-label="Local taxes">
      <h3 className="text-sm font-semibold text-gray-700 mb-1">Local taxes (city, county, school district)</h3>
      <p className="text-xs text-gray-500 mb-3">
        Add the local taxes your employees owe — built-in ones with their 2026 rules, or your own from the city's or county's
        notice. Then choose on each employee which ones apply (as a resident or not).
      </p>
      {error && <p className="text-sm text-red-600 mb-2">{error}</p>}
      <table className="text-sm w-full mb-3">
        <tbody>
          {mine.length === 0 && <tr><td className="py-2 text-gray-400">No local taxes yet.</td></tr>}
          {mine.map((l) => (
            <tr key={l.code} className="border-t border-gray-100">
              <td className="py-2 pr-3 font-mono text-xs w-32">{l.code}</td>
              <td className="py-2 text-gray-700">
                {l.name} <span className="text-gray-400">({l.state})</span>
                <div className="text-xs text-gray-400">
                  {describeLocality(l)}{l.source ? ` · ${l.source}` : ''}
                  {!l.verified && <span className="text-amber-700"> · check this rate with the city</span>}
                </div>
              </td>
              <td className="py-2 text-right whitespace-nowrap">
                <button className="btn-link text-xs" onClick={() => { setEditing(draftOf(l)); setIsNew(false); setSaveError(''); }}>Edit</button>
                <button className="btn-link text-xs ml-2" style={{ color: '#dc2626' }} onClick={() => remove(l.code)}>Remove</button>
              </td>
            </tr>
          ))}
        </tbody>
      </table>

      {!editing && (
        <div className="flex flex-wrap items-center gap-2 mb-2">
          <select value={pick} onChange={(e) => setPick(e.target.value)} aria-label="Add a built-in local tax">
            <option value="">Add a built-in local tax…</option>
            {available.map((c) => <option key={c.code} value={c.code}>{c.name} ({c.state}){c.verified ? '' : ' — check rate'}</option>)}
          </select>
          <button className="btn btn-secondary" disabled={!pick}
            onClick={() => { const c = catalog.find((x) => x.code === pick); if (c) { setEditing(draftOf(c)); setIsNew(true); setSaveError(''); setPick(''); } }}>
            Add
          </button>
          <button className="btn btn-secondary" onClick={() => { setEditing(draftOf()); setIsNew(true); setSaveError(''); }}>+ Your own local tax</button>
        </div>
      )}

      {editing && (
        <form onSubmit={save} className="border-t border-gray-200 pt-4 mt-2" aria-label="Local tax">
          <div className="form-grid mb-3" style={{ gridTemplateColumns: 'repeat(3, 1fr)' }}>
            <label className="block"><span className="text-xs font-medium text-gray-500">Code (e.g. OH-COLUMBUS)</span>
              <input required value={editing.code} disabled={!isNew} onChange={set('code')} aria-label="Locality code" /></label>
            <label className="block"><span className="text-xs font-medium text-gray-500">Name</span>
              <input required value={editing.name} onChange={set('name')} aria-label="Locality name" /></label>
            <label className="block"><span className="text-xs font-medium text-gray-500">State</span>
              <select required value={editing.state} onChange={set('state')} aria-label="Locality state">
                <option value="">Choose</option>
                {US_STATES.map((s) => <option key={s} value={s}>{s}</option>)}
              </select></label>
            <label className="block" style={{ gridColumn: '1 / -1' }}><span className="text-xs font-medium text-gray-500">How it is worked out</span>
              <select value={editing.method} disabled={editing.method === 'FORMULA'} onChange={set('method')} aria-label="Locality method">
                {Object.entries(METHOD_LABEL).filter(([k]) => k !== 'FORMULA' || editing.method === 'FORMULA')
                  .map(([k, v]) => <option key={k} value={k}>{v}</option>)}
              </select></label>
            {editing.method === 'PERCENT' && (
              <>
                {field('Resident rate %', 'residentRate')}
                {field('Nonresident rate %', 'nonresidentRate')}
                {field('Employer rate % (employer-paid taxes)', 'employerRate')}
                {field('Exemption $ a year per local allowance', 'exemption')}
                {field('Exemption $ a year per second allowance', 'secondExemption')}
                <label className="block"><span className="text-xs font-medium text-gray-500">Group (withhold only the higher, e.g. PA-EIT)</span>
                  <input value={editing.group} onChange={set('group')} aria-label="Locality group" /></label>
              </>
            )}
            {editing.method === 'MONTHLY_FLAT' && (
              <>
                {field('Employee $ a month', 'flatEmployee')}
                {field('Employer $ a month', 'flatEmployer')}
                {field('Due when monthly wages reach $', 'monthlyThreshold')}
              </>
            )}
            {editing.method === 'WEEKLY_FLAT' && (
              <>
                {field('Employee $ a week', 'flatEmployee')}
                {field('Employer $ a week', 'flatEmployer')}
              </>
            )}
            {editing.method === 'PERIOD_FLAT' && (
              <>
                {field('Employee $ a year', 'flatEmployee')}
                {field('Employer $ a year', 'flatEmployer')}
              </>
            )}
            {editing.method !== 'FORMULA' && field('Yearly maximum per employee $ (blank = none)', 'employeeYearCap')}
            <label className="block"><span className="text-xs font-medium text-gray-500">Owed to (optional)</span>
              <select value={editing.accountId} onChange={set('accountId')} aria-label="Locality account">
                <option value="">Local Income Tax Withheld (2390)</option>
                {liabilities.map((a) => <option key={a.id} value={a.id}>{a.code} {a.name}</option>)}
              </select></label>
          </div>
          {editing.method !== 'FORMULA' && (
            <div className="flex flex-wrap gap-4 text-sm mb-3">
              <label className="flex items-center gap-2"><input type="checkbox" checked={editing.includes401k} onChange={set('includes401k')} />401(k) deferrals are taxed</label>
              <label className="flex items-center gap-2"><input type="checkbox" checked={editing.includes125} onChange={set('includes125')} />Section 125 amounts are taxed</label>
            </div>
          )}
          {editing.source && <p className="text-xs text-gray-500 mb-2">Source: {editing.source}{!editing.verified && <span className="text-amber-700"> — check this with the city before using it.</span>}</p>}
          {saveError && <p className="text-sm text-red-600 mb-2">{saveError}</p>}
          <div className="flex gap-2">
            <button type="submit" className="btn btn-primary">Save local tax</button>
            <button type="button" className="btn btn-secondary" onClick={() => setEditing(null)}>Cancel</button>
          </div>
        </form>
      )}
    </div>
  );
}
