import { useCallback, useEffect, useState } from 'react';
import api, { errMsg } from '../api/client';
import { US_STATES, usd, type StateTaxRule } from '../lib/usPayroll';

type Draft = {
  method: 'FLAT' | 'BRACKETS';
  flatRate: string;
  brackets: { upTo: string; rate: string }[];
  married: boolean;
  bracketsMarried: { upTo: string; rate: string }[];
  standardDeductionSingle: string;
  standardDeductionMarried: string;
  allowanceDeduction: string;
  allowanceCredit: string;
  excludes401k: boolean;
  notes: string;
};

const s = (v: number | null | undefined) => (v ? String(v) : '');
const toRows = (b: [number | null, number][] | null | undefined) =>
  (b && b.length ? b : [[null, 0] as [number | null, number]]).map(([u, r]) => ({ upTo: u === null ? '' : String(u), rate: String(r) }));

function draftOf(r?: StateTaxRule): Draft {
  return {
    method: r?.method === 'BRACKETS' ? 'BRACKETS' : 'FLAT',
    flatRate: s(r?.flatRate),
    brackets: toRows(r?.brackets),
    married: !!r?.bracketsMarried,
    bracketsMarried: toRows(r?.bracketsMarried),
    standardDeductionSingle: s(r?.standardDeductionSingle),
    standardDeductionMarried: s(r?.standardDeductionMarried),
    allowanceDeduction: s(r?.allowanceDeduction),
    allowanceCredit: s(r?.allowanceCredit),
    excludes401k: r?.excludes401k ?? true,
    notes: r?.notes ?? '',
  };
}

function summary(r: StateTaxRule) {
  if (r.method === 'NONE') return 'No state income tax on wages';
  if (r.method === null) return 'Not set up — pay runs with employees here can’t be posted';
  const base = r.method === 'FLAT' ? `Flat ${r.flatRate}%` : `${r.brackets?.length ?? 0} brackets`;
  const extras = [
    r.standardDeductionSingle ? `std. deduction ${usd(r.standardDeductionSingle)}` : '',
    r.allowanceDeduction ? `${usd(r.allowanceDeduction)} per allowance` : '',
    r.allowanceCredit ? `${usd(r.allowanceCredit)} credit per allowance` : '',
    r.excludes401k ? '' : '401(k) taxed',
  ].filter(Boolean);
  return [base, ...extras].join(' · ');
}

/**
 * The company's state income tax rules, one per state, copied from each state's withholding
 * guide. States without an income tax on wages are built in.
 */
export default function StateTaxRules() {
  const [rules, setRules] = useState<StateTaxRule[]>([]);
  const [error, setError] = useState('');
  const [editing, setEditing] = useState<string | null>(null);
  const [draft, setDraft] = useState<Draft>(draftOf());
  const [saveError, setSaveError] = useState('');
  const [addState, setAddState] = useState('');

  const load = useCallback(
    () => api.get<StateTaxRule[]>('/payroll/us/state-taxes').then((r) => { setRules(r.data); setError(''); }).catch((err) => setError(errMsg(err, 'Could not load state rules'))),
    [],
  );
  useEffect(() => { load(); }, [load]);

  const open = (state: string) => {
    setEditing(state);
    setSaveError('');
    setDraft(draftOf(rules.find((r) => r.state === state && r.method !== null)));
  };

  const save = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!editing) return;
    setSaveError('');
    const num = (v: string) => (v.trim() === '' ? 0 : Number(v));
    const rows = (b: Draft['brackets']) => b.map((x, i) => [x.upTo.trim() === '' && i === b.length - 1 ? null : Number(x.upTo), Number(x.rate || 0)]);
    try {
      await api.put(`/payroll/us/state-taxes/${editing}`, {
        method: draft.method,
        ...(draft.method === 'FLAT' ? { flatRate: num(draft.flatRate) } : { brackets: rows(draft.brackets), bracketsMarried: draft.married ? rows(draft.bracketsMarried) : null }),
        standardDeductionSingle: num(draft.standardDeductionSingle),
        standardDeductionMarried: num(draft.standardDeductionMarried),
        allowanceDeduction: num(draft.allowanceDeduction),
        allowanceCredit: num(draft.allowanceCredit),
        excludes401k: draft.excludes401k,
        notes: draft.notes.trim() || null,
      });
      setEditing(null);
      await load();
    } catch (err) {
      setSaveError(errMsg(err, 'Could not save the rule'));
    }
  };

  const remove = async (state: string) => {
    if (!window.confirm(`Delete the ${state} income tax rule? Pay runs for employees in ${state} can't be posted until it is added again.`)) return;
    try {
      await api.delete(`/payroll/us/state-taxes/${state}`);
      await load();
    } catch (err) {
      setError(errMsg(err, 'Could not delete the rule'));
    }
  };

  const shown = rules.filter((r) => !r.builtIn);
  const addable = US_STATES.filter((st) => !rules.some((r) => r.state === st && (r.builtIn || r.method !== null)));
  const set = (k: keyof Draft) => (e: React.ChangeEvent<HTMLInputElement | HTMLSelectElement>) => setDraft({ ...draft, [k]: e.target.value });

  return (
    <div className="card mb-5">
      <h3 className="text-sm font-semibold text-gray-700 mb-1">State income tax</h3>
      <p className="text-xs text-gray-500 mb-3">
        Copy each state's figures from its current employer withholding guide; update them when the state publishes new ones.
        No state income tax on wages: {rules.filter((r) => r.builtIn).map((r) => r.state).join(', ')}.
      </p>
      {error && <p className="text-sm text-red-600 mb-2">{error}</p>}

      <table className="text-sm w-full mb-3">
        <tbody>
          {shown.length === 0 && <tr><td className="py-2 text-gray-400">No state rules yet. Add the states your employees work in.</td></tr>}
          {shown.map((r) => (
            <tr key={r.state} className="border-t border-gray-100">
              <td className="py-2 pr-3 font-medium w-12">{r.state}</td>
              <td className={`py-2 ${r.method === null ? 'text-amber-700' : 'text-gray-700'}`}>
                {summary(r)}
                {r.notes && <div className="text-xs text-gray-400">{r.notes}</div>}
              </td>
              <td className="py-2 text-right whitespace-nowrap">
                <button className="btn-link text-xs" onClick={() => open(r.state)}>{r.method === null ? 'Set up' : 'Edit'}</button>
                {r.method !== null && <button className="btn-link text-xs ml-2" style={{ color: '#dc2626' }} onClick={() => remove(r.state)}>Delete</button>}
              </td>
            </tr>
          ))}
        </tbody>
      </table>

      {!editing && (
        <div className="flex items-center gap-2">
          <select value={addState} onChange={(e) => setAddState(e.target.value)}>
            <option value="">Add a state…</option>
            {addable.map((st) => <option key={st} value={st}>{st}</option>)}
          </select>
          <button className="btn btn-secondary" disabled={!addState} onClick={() => { open(addState); setAddState(''); }}>Add</button>
        </div>
      )}

      {editing && (
        <form onSubmit={save} className="border-t border-gray-200 pt-4 mt-2">
          <p className="text-sm font-semibold text-gray-800 mb-3">{editing} withholding rule</p>
          <div className="form-grid mb-3" style={{ gridTemplateColumns: 'repeat(4, 1fr)' }}>
            <label className="block">
              <span className="text-xs font-medium text-gray-500">Method</span>
              <select value={draft.method} onChange={set('method')}>
                <option value="FLAT">Flat rate</option>
                <option value="BRACKETS">Brackets (progressive)</option>
              </select>
            </label>
            {draft.method === 'FLAT' && (
              <label className="block">
                <span className="text-xs font-medium text-gray-500">Rate %</span>
                <input type="number" min="0" max="100" step="0.0001" required value={draft.flatRate} onChange={set('flatRate')} />
              </label>
            )}
            <Amt label="Standard deduction, single $ (a year)" v={draft.standardDeductionSingle} on={set('standardDeductionSingle')} />
            <Amt label="Standard deduction, married $ (a year)" v={draft.standardDeductionMarried} on={set('standardDeductionMarried')} />
            <Amt label="Deduction per allowance $ (a year)" v={draft.allowanceDeduction} on={set('allowanceDeduction')} />
            <Amt label="Credit per allowance $ (a year)" v={draft.allowanceCredit} on={set('allowanceCredit')} />
            <label className="flex items-center gap-2 text-sm mt-5" style={{ gridColumn: 'span 2' }}>
              <input type="checkbox" checked={draft.excludes401k} onChange={(e) => setDraft({ ...draft, excludes401k: e.target.checked })} />
              401(k) deferrals are not state wages
            </label>
          </div>

          {draft.method === 'BRACKETS' && (
            <>
              <Brackets title={draft.married ? 'Brackets — single (annual taxable income)' : 'Brackets (annual taxable income)'} rows={draft.brackets} set={(b) => setDraft({ ...draft, brackets: b })} />
              <label className="flex items-center gap-2 text-sm mb-2">
                <input type="checkbox" checked={draft.married} onChange={(e) => setDraft({ ...draft, married: e.target.checked })} />
                Married employees use different brackets
              </label>
              {draft.married && <Brackets title="Brackets — married" rows={draft.bracketsMarried} set={(b) => setDraft({ ...draft, bracketsMarried: b })} />}
            </>
          )}

          <label className="block mb-3">
            <span className="text-xs font-medium text-gray-500">Notes (e.g. which guide and year these figures come from)</span>
            <input value={draft.notes} onChange={set('notes')} />
          </label>
          {saveError && <p className="text-sm text-red-600 mb-2">{saveError}</p>}
          <div className="flex gap-2">
            <button type="submit" className="btn btn-primary">Save {editing} rule</button>
            <button type="button" className="btn btn-secondary" onClick={() => setEditing(null)}>Cancel</button>
          </div>
        </form>
      )}
    </div>
  );
}

function Amt({ label, v, on }: { label: string; v: string; on: (e: React.ChangeEvent<HTMLInputElement>) => void }) {
  return (
    <label className="block">
      <span className="text-xs font-medium text-gray-500">{label}</span>
      <input type="number" min="0" step="0.01" placeholder="0" value={v} onChange={on} />
    </label>
  );
}

function Brackets({ title, rows, set }: { title: string; rows: { upTo: string; rate: string }[]; set: (b: { upTo: string; rate: string }[]) => void }) {
  return (
    <div className="mb-3">
      <p className="text-xs font-semibold text-gray-500 uppercase tracking-wide mb-1">{title}</p>
      <table className="text-sm">
        <thead><tr className="text-xs text-gray-500"><th className="text-left pr-3">From</th><th className="text-left pr-3">Up to $ (blank = no limit, last row)</th><th className="text-left pr-3">Rate %</th><th /></tr></thead>
        <tbody>
          {rows.map((r, i) => (
            <tr key={i}>
              <td className="pr-3 py-1 text-gray-500">{i === 0 ? '$0' : `$${rows[i - 1].upTo || '…'}`}</td>
              <td className="pr-3 py-1"><input type="number" min="0" step="0.01" value={r.upTo} style={{ width: 150 }} onChange={(e) => set(rows.map((x, j) => (j === i ? { ...x, upTo: e.target.value } : x)))} /></td>
              <td className="pr-3 py-1"><input type="number" min="0" max="100" step="0.0001" value={r.rate} style={{ width: 100 }} onChange={(e) => set(rows.map((x, j) => (j === i ? { ...x, rate: e.target.value } : x)))} /></td>
              <td className="py-1">{rows.length > 1 && <button type="button" className="btn-link text-xs" style={{ color: '#dc2626' }} onClick={() => set(rows.filter((_, j) => j !== i))}>Remove</button>}</td>
            </tr>
          ))}
        </tbody>
      </table>
      <button type="button" className="btn-link text-xs" onClick={() => set([...rows, { upTo: '', rate: '' }])}>+ Add bracket</button>
    </div>
  );
}
