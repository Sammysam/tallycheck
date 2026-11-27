import { useCallback, useEffect, useState } from 'react';
import api, { errMsg } from '../api/client';
import { askConfirm } from '../lib/confirm';
import { LEAVE_CHOICES, US_STATES, usd, type LeaveOption, type LeaveProgram, type UnemploymentDefault, type UsStateRegistration, type UsStatesOverview } from '../lib/usPayroll';

/**
 * D63: the states the company is registered in for payroll (account numbers, the unemployment
 * rate each state assigned, optional payable accounts per state) and the reciprocity agreements
 * between states. Employees' work and home states are set on each employee's US tax section.
 */

interface LedgerAccount { id: string; code: string; name: string; type: string; currency: string | null }

type Draft = {
  withholdingAccountNo: string; unemploymentAccountNo: string; sutaRate: string; sutaWageBase: string; stateTaxAccountId: string; sutaAccountId: string;
  employees: string; leaveAccountId: string; leaveOptions: Record<string, LeaveOption>;
  surcharges: { name: string; rate: string }[];
};

const draftOf = (r?: UsStateRegistration): Draft => ({
  withholdingAccountNo: r?.withholdingAccountNo ?? '',
  unemploymentAccountNo: r?.unemploymentAccountNo ?? '',
  sutaRate: r ? String(r.sutaRate) : '',
  sutaWageBase: r?.sutaWageBase == null ? '' : String(r.sutaWageBase),
  stateTaxAccountId: r?.stateTaxAccountId ?? '',
  sutaAccountId: r?.sutaAccountId ?? '',
  employees: r?.employees == null ? '' : String(r.employees),
  leaveAccountId: r?.leaveAccountId ?? '',
  leaveOptions: r?.leaveOptions ?? {},
  surcharges: (r?.sutaSurcharges ?? []).map((c) => ({ name: c.name, rate: String(c.rate) })),
});

/** D68: a new registration starts from the state's 2026 defaults. */
const draftFromDefaults = (d?: UnemploymentDefault): Draft => ({
  ...draftOf(),
  sutaRate: d?.newEmployerRate == null ? '' : String(d.newEmployerRate),
  sutaWageBase: d ? String(d.wageBase) : '',
  surcharges: (d?.surcharges ?? []).map((c) => ({ name: c.name, rate: String(c.rate) })),
});

export default function UsStateRegistrations({ accounts }: { accounts: LedgerAccount[] }) {
  const [data, setData] = useState<UsStatesOverview | null>(null);
  const [error, setError] = useState('');
  const [editing, setEditing] = useState<string | null>(null);
  const [draft, setDraft] = useState<Draft>(draftOf());
  const [saveError, setSaveError] = useState('');
  const [addState, setAddState] = useState('');
  const [pair, setPair] = useState({ workState: '', residentState: '' });
  const [programs, setPrograms] = useState<LeaveProgram[]>([]);
  const [defaults, setDefaults] = useState<UnemploymentDefault[]>([]);

  const load = useCallback(
    () => api.get<UsStatesOverview>('/payroll/us/states').then((r) => { setData(r.data); setError(''); })
      .catch((err) => setError(errMsg(err, 'Could not load the states'))),
    [],
  );
  useEffect(() => { load(); }, [load]);
  useEffect(() => { api.get<LeaveProgram[]>('/payroll/us/leave-programs').then((r) => setPrograms(r.data)).catch(() => setPrograms([])); }, []);
  useEffect(() => { api.get<UnemploymentDefault[]>('/payroll/us/unemployment-defaults').then((r) => setDefaults(r.data)).catch(() => setDefaults([])); }, []);

  const open = (state: string) => {
    setEditing(state);
    setSaveError('');
    const existing = data?.registrations.find((r) => r.state === state);
    setDraft(existing ? draftOf(existing) : draftFromDefaults(defaults.find((d) => d.state === state)));
  };

  const save = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!editing) return;
    setSaveError('');
    try {
      await api.put(`/payroll/us/states/${editing}`, {
        withholdingAccountNo: draft.withholdingAccountNo.trim() || null,
        unemploymentAccountNo: draft.unemploymentAccountNo.trim() || null,
        sutaRate: Number(draft.sutaRate || 0),
        sutaWageBase: draft.sutaWageBase.trim() === '' ? null : Number(draft.sutaWageBase),
        stateTaxAccountId: draft.stateTaxAccountId || null,
        sutaAccountId: draft.sutaAccountId || null,
        employees: draft.employees.trim() === '' ? null : Number(draft.employees),
        leaveAccountId: draft.leaveAccountId || null,
        leaveOptions: draft.leaveOptions,
        sutaSurcharges: draft.surcharges.filter((c) => c.name.trim() !== '').map((c) => ({ name: c.name.trim(), rate: Number(c.rate || 0) })),
      });
      setEditing(null);
      await load();
    } catch (err) {
      setSaveError(errMsg(err, 'Could not save the state'));
    }
  };

  const remove = async (state: string) => {
    if (!(await askConfirm(`Remove the ${state} registration? Employees working mainly in ${state} then use the company-wide unemployment settings.`))) return;
    try {
      await api.delete(`/payroll/us/states/${state}`);
      await load();
    } catch (err) {
      setError(errMsg(err, 'Could not remove the state'));
    }
  };

  const addPair = async () => {
    try {
      await api.post('/payroll/us/reciprocity', pair);
      setPair({ workState: '', residentState: '' });
      await load();
    } catch (err) {
      setError(errMsg(err, 'Could not add the agreement'));
    }
  };

  const removePair = async (w: string, r: string) => {
    try {
      await api.delete(`/payroll/us/reciprocity/${w}/${r}`);
      await load();
    } catch (err) {
      setError(errMsg(err, 'Could not remove the agreement'));
    }
  };

  if (!data) return error ? <div className="card mb-5 text-sm text-red-600">{error}</div> : null;
  const registered = data.registrations.map((r) => r.state);
  const unregistered = data.statesInUse.filter((s) => !registered.includes(s));
  const liabilities = accounts.filter((a) => a.type === 'LIABILITY' && !a.currency);
  const accountName = (id: string | null) => {
    const a = accounts.find((x) => x.id === id);
    return a ? `${a.code} ${a.name}` : null;
  };
  const set = (k: keyof Draft) => (e: React.ChangeEvent<HTMLInputElement | HTMLSelectElement>) => setDraft({ ...draft, [k]: e.target.value });
  const setOption = (id: string, change: LeaveOption) => {
    const next = { ...(draft.leaveOptions[id] ?? {}), ...change };
    setDraft({ ...draft, leaveOptions: { ...draft.leaveOptions, [id]: next } });
  };
  const statePrograms = programs.filter((p) => p.state === editing);
  const stateDefault = defaults.find((d) => d.state === editing);
  const setCharge = (i: number, change: Partial<{ name: string; rate: string }>) =>
    setDraft({ ...draft, surcharges: draft.surcharges.map((c, j) => (j === i ? { ...c, ...change } : c)) });

  return (
    <div className="card mb-5" aria-label="States you are registered in">
      <h3 className="text-sm font-semibold text-gray-700 mb-1">States you are registered in</h3>
      <p className="text-xs text-gray-500 mb-3">
        Add every state where you have employees: your account numbers there and the unemployment (SUTA) rate that state
        assigned you. Each employee's unemployment tax goes to the state they mainly work in. A state left out here uses the
        company-wide unemployment settings above.
      </p>
      {error && <p className="text-sm text-red-600 mb-2">{error}</p>}
      {unregistered.length > 0 && (
        <p className="text-xs text-amber-700 mb-2">Employees live or work in {unregistered.join(', ')}, which {unregistered.length === 1 ? 'is' : 'are'} not registered here yet.</p>
      )}

      <table className="text-sm w-full mb-3">
        <tbody>
          {data.registrations.length === 0 && <tr><td className="py-2 text-gray-400">No states yet.</td></tr>}
          {data.registrations.map((r) => (
            <tr key={r.state} className="border-t border-gray-100">
              <td className="py-2 pr-3 font-medium w-12">{r.state}</td>
              <td className="py-2 text-gray-700">
                SUTA {r.sutaRate}%{(r.sutaSurcharges ?? []).map((c) => ` + ${c.name} ${c.rate}%`).join('')}{r.sutaWageBase != null ? ` on the first ${usd(r.sutaWageBase)}` : ' (no wage limit)'}
                <div className="text-xs text-gray-400">
                  {[r.withholdingAccountNo && `Withholding no. ${r.withholdingAccountNo}`, r.unemploymentAccountNo && `Unemployment no. ${r.unemploymentAccountNo}`,
                    accountName(r.stateTaxAccountId) && `income tax to ${accountName(r.stateTaxAccountId)}`, accountName(r.sutaAccountId) && `unemployment to ${accountName(r.sutaAccountId)}`]
                    .filter(Boolean).join(' · ')}
                </div>
              </td>
              <td className="py-2 text-right whitespace-nowrap">
                <button className="btn-link text-xs" onClick={() => open(r.state)}>Edit</button>
                <button className="btn-link text-xs ml-2" style={{ color: '#dc2626' }} onClick={() => remove(r.state)}>Remove</button>
              </td>
            </tr>
          ))}
        </tbody>
      </table>

      {!editing && (
        <div className="flex items-center gap-2 mb-4">
          <select value={addState} onChange={(e) => setAddState(e.target.value)} aria-label="Register a state">
            <option value="">Register a state…</option>
            {unregistered.map((st) => <option key={`u-${st}`} value={st}>{st} (employees here)</option>)}
            {US_STATES.filter((st) => !registered.includes(st) && !unregistered.includes(st)).map((st) => <option key={st} value={st}>{st}</option>)}
          </select>
          <button className="btn btn-secondary" disabled={!addState} onClick={() => { open(addState); setAddState(''); }}>Add</button>
        </div>
      )}

      {editing && (
        <form onSubmit={save} className="border-t border-gray-200 pt-4 mt-2 mb-4" aria-label={`${editing} registration`}>
          <p className="text-sm font-semibold text-gray-800 mb-1">{editing}</p>
          {stateDefault && (
            <p className="text-xs text-gray-500 mb-3" aria-label="Unemployment defaults">
              2026 for {editing}: wage base {usd(stateDefault.wageBase)}
              {stateDefault.newEmployerRate != null ? `, new employers ${stateDefault.newEmployerRate}%` : ''}. {stateDefault.note} Source: {stateDefault.source}.
              {!stateDefault.verified && <span className="text-amber-700"> Check these figures with the state — its page could not be read directly.</span>}
              {' '}Replace them with the figures on your own rate notice.
            </p>
          )}
          <div className="form-grid mb-3" style={{ gridTemplateColumns: 'repeat(3, 1fr)' }}>
            <label className="block"><span className="text-xs font-medium text-gray-500">State withholding account no.</span>
              <input value={draft.withholdingAccountNo} onChange={set('withholdingAccountNo')} aria-label="Withholding account no." /></label>
            <label className="block"><span className="text-xs font-medium text-gray-500">Unemployment account no.</span>
              <input value={draft.unemploymentAccountNo} onChange={set('unemploymentAccountNo')} aria-label="Unemployment account no." /></label>
            <span />
            <label className="block"><span className="text-xs font-medium text-gray-500">SUTA rate % (from the state's notice)</span>
              <input type="number" min="0" max="100" step="0.0001" required value={draft.sutaRate} onChange={set('sutaRate')} aria-label="SUTA rate" /></label>
            <label className="block"><span className="text-xs font-medium text-gray-500">SUTA wage base $ (blank = no limit)</span>
              <input type="number" min="0" step="0.01" value={draft.sutaWageBase} onChange={set('sutaWageBase')} aria-label="SUTA wage base" /></label>
            <span />
            <label className="block"><span className="text-xs font-medium text-gray-500">{editing} income tax owed to (optional)</span>
              <select value={draft.stateTaxAccountId} onChange={set('stateTaxAccountId')}>
                <option value="">The state income tax account</option>
                {liabilities.map((a) => <option key={a.id} value={a.id}>{a.code} {a.name}</option>)}
              </select></label>
            <label className="block"><span className="text-xs font-medium text-gray-500">{editing} unemployment owed to (optional)</span>
              <select value={draft.sutaAccountId} onChange={set('sutaAccountId')}>
                <option value="">The state unemployment account</option>
                {liabilities.map((a) => <option key={a.id} value={a.id}>{a.code} {a.name}</option>)}
              </select></label>
          </div>
          <div className="mb-3" aria-label="Unemployment surcharges">
            <p className="text-xs font-semibold text-gray-700 mb-1">Other employer charges on the same unemployment wages (owed to {editing})</p>
            {draft.surcharges.length === 0 && <p className="text-xs text-gray-400 mb-1">None.</p>}
            {draft.surcharges.map((c, i) => (
              <div key={i} className="flex items-center gap-2 mb-1">
                <input value={c.name} onChange={(e) => setCharge(i, { name: e.target.value })} aria-label="Charge name" style={{ flex: 1 }} />
                <input type="number" min="0" max="20" step="0.0001" value={c.rate} onChange={(e) => setCharge(i, { rate: e.target.value })} aria-label="Charge rate" style={{ width: '7rem' }} />
                <span className="text-xs text-gray-500">%</span>
                <button type="button" className="btn-link text-xs" style={{ color: '#dc2626' }} onClick={() => setDraft({ ...draft, surcharges: draft.surcharges.filter((_, j) => j !== i) })}>Remove</button>
              </div>
            ))}
            <button type="button" className="btn-link text-xs" onClick={() => setDraft({ ...draft, surcharges: [...draft.surcharges, { name: '', rate: '' }] })}>+ Add a charge</button>
          </div>
          {statePrograms.length > 0 && (
            <div className="mb-3" aria-label="Disability and paid leave">
              <p className="text-xs font-semibold text-gray-700 mb-1">Disability and paid leave in {editing}</p>
              <div className="form-grid mb-2" style={{ gridTemplateColumns: 'repeat(3, 1fr)' }}>
                <label className="block"><span className="text-xs font-medium text-gray-500">Employees (for small-employer rules; blank = treated as large)</span>
                  <input type="number" min="0" step="1" value={draft.employees} onChange={set('employees')} aria-label="Employees" /></label>
                <label className="block"><span className="text-xs font-medium text-gray-500">{editing} contributions owed to (optional)</span>
                  <select value={draft.leaveAccountId} onChange={set('leaveAccountId')}>
                    <option value="">The disability and paid leave account</option>
                    {liabilities.map((a) => <option key={a.id} value={a.id}>{a.code} {a.name}</option>)}
                  </select></label>
                <span />
              </div>
              {statePrograms.map((p) => {
                const choice = LEAVE_CHOICES[p.id];
                const o = draft.leaveOptions[p.id] ?? {};
                return (
                  <div key={p.id} className="border-t border-gray-100 py-2">
                    <div className="text-sm text-gray-800">{p.name}</div>
                    <div className="text-xs text-gray-400 mb-1">{p.source}</div>
                    <div className="flex flex-wrap items-center gap-4 text-sm">
                      {choice?.rate && (
                        <label className="flex items-center gap-2"><span className="text-xs text-gray-500">{choice.rate}</span>
                          <input type="number" min="0" max="100" step="0.0001" style={{ width: '7rem' }} value={o.employerRate ?? ''}
                            onChange={(e) => setOption(p.id, { employerRate: e.target.value === '' ? undefined : Number(e.target.value) })} aria-label={`${p.id} rate`} /></label>
                      )}
                      {choice?.small && (
                        <label className="flex items-center gap-2"><input type="checkbox" checked={!!o.small} onChange={(e) => setOption(p.id, { small: e.target.checked })} />{choice.small}</label>
                      )}
                      <label className="flex items-center gap-2"><input type="checkbox" checked={!!o.off} onChange={(e) => setOption(p.id, { off: e.target.checked })} aria-label={`${p.id} off`} />{choice?.off ?? 'Not withheld'}</label>
                    </div>
                  </div>
                );
              })}
            </div>
          )}
          {saveError && <p className="text-sm text-red-600 mb-2">{saveError}</p>}
          <div className="flex gap-2">
            <button type="submit" className="btn btn-primary">Save {editing}</button>
            <button type="button" className="btn btn-secondary" onClick={() => setEditing(null)}>Cancel</button>
          </div>
        </form>
      )}

      <h4 className="text-xs font-semibold text-gray-700 mb-1">Reciprocity agreements</h4>
      <p className="text-xs text-gray-500 mb-2">
        When two states have an agreement, someone living in one and working in the other pays income tax only to their home
        state (the employee usually gives you the state's exemption form). Add the pairs that apply to your employees.
      </p>
      <table className="text-sm mb-2">
        <tbody>
          {data.reciprocity.length === 0 && <tr><td className="py-1 text-gray-400">None.</td></tr>}
          {data.reciprocity.map((p) => (
            <tr key={`${p.workState}-${p.residentState}`}>
              <td className="py-1 pr-4">Lives in <strong>{p.residentState}</strong>, works in <strong>{p.workState}</strong>: pays {p.residentState} only</td>
              <td className="py-1"><button className="btn-link text-xs" style={{ color: '#dc2626' }} onClick={() => removePair(p.workState, p.residentState)}>Remove</button></td>
            </tr>
          ))}
        </tbody>
      </table>
      <div className="flex items-center gap-2 text-sm">
        Lives in
        <select value={pair.residentState} onChange={(e) => setPair({ ...pair, residentState: e.target.value })} aria-label="Home state">
          <option value="">State</option>{US_STATES.map((s) => <option key={s} value={s}>{s}</option>)}
        </select>
        works in
        <select value={pair.workState} onChange={(e) => setPair({ ...pair, workState: e.target.value })} aria-label="Work state">
          <option value="">State</option>{US_STATES.map((s) => <option key={s} value={s}>{s}</option>)}
        </select>
        <button className="btn btn-secondary" disabled={!pair.workState || !pair.residentState || pair.workState === pair.residentState} onClick={addPair}>Add agreement</button>
      </div>
    </div>
  );
}
