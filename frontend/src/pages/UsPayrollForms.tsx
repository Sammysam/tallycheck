import { useCallback, useEffect, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import api, { errMsg } from '../api/client';
import PageHeader from '../components/PageHeader';
import { US_STATES, usd } from '../lib/usPayroll';

type Money = number;
interface Employer { name: string | null; ein: string | null; state: string | null }
interface F941 {
  employer: Employer; payRuns: number; line1: number; line2: Money; line3: Money;
  line5a: { wages: Money; tax: Money }; line5c: { wages: Money; tax: Money }; line5d: { wages: Money; tax: Money };
  line5e: Money; line6: Money; line7: Money; line10: Money; line12: Money; line13: Money; line14: Money; line15: Money;
  line16: { month: string; liability: Money }[]; line16Total: Money;
}
interface F940 {
  line3: Money; line4: Money; line5: Money; line6: Money; line7: Money; line8: Money; line11: Money; line12: Money;
  line13: Money; line14: Money; line15: Money; line16: { quarter: number; futaWages: Money; liability: Money }[];
}
interface W2Row {
  employeeId: string | null; employeeCode: string; name: string; workState: string; paychecks: number;
  box1: Money; box2: Money; box3: Money; box4: Money; box5: Money; box6: Money; box12D: Money;
  box15: string; box16: Money; box17: Money;
  box14?: { label: string; amount: Money }[];
  locals?: { code: string; state: string; box18: Money; box19: Money; box20: string }[];
}
interface W2 { employees: W2Row[]; totals: Record<string, Money> }
/** D70: one state's quarter and year. */
interface Amounts { id?: string; code?: string; name: string; wages: Money; employee: Money; employer: Money; deposits: Money }
interface StateQuarter {
  state: string;
  accountNumbers: { withholdingAccountNo: string | null; unemploymentAccountNo: string | null; registered: boolean };
  withholding: { months: { month: string; wages: Money; withheld: Money }[]; wages: Money; withheld: Money; employees: number; deposits: Money };
  unemployment: {
    employees: { employeeCode: string; name: string; grossWages: Money; excessWages: Money; taxableWages: Money; employerContributions: Money; employeeContributions: Money }[];
    grossWages: Money; excessWages: Money; taxableWages: Money; employerContributions: Money; employeeContributions: Money;
    ratesUsed: { rate: number; surcharges: number }[]; employeesOnThe12th: { date: string; employees: number }[]; deposits: Money;
  };
  leave: Amounts[]; local: Amounts[];
}
interface StateAnnual {
  quarters: { quarter: number; wages: Money; withheld: Money; deposits: Money; unemploymentTaxableWages: Money; unemploymentContributions: Money }[];
  totals: { wages: Money; withheld: Money; deposits: Money; unemploymentTaxableWages: Money; unemploymentContributions: Money };
  w2: { forms: number; stateWages: Money; stateTax: Money }; balanced: boolean;
}

const thisYear = new Date().getFullYear();
const thisQuarter = Math.floor(new Date().getMonth() / 3) + 1;
const monthName = (ym: string) => new Date(`${ym}-01T00:00:00Z`).toLocaleDateString(undefined, { month: 'long', timeZone: 'UTC' });

/**
 * The figures for Form 941, Form 940 and each W-2, from posted US pay runs (by pay date).
 * Nothing is filed from here; copy the numbers onto the IRS forms or into your filing service.
 */
export default function UsPayrollForms() {
  const navigate = useNavigate();
  const [year, setYear] = useState(thisYear);
  const [quarter, setQuarter] = useState(thisQuarter);
  const [f941, setF941] = useState<F941 | null>(null);
  const [f940, setF940] = useState<F940 | null>(null);
  const [w2, setW2] = useState<W2 | null>(null);
  const [state, setState] = useState('');
  const [registered, setRegistered] = useState<string[]>([]);
  const [sq, setSq] = useState<StateQuarter | null>(null);
  const [sa, setSa] = useState<StateAnnual | null>(null);
  const [error, setError] = useState('');

  const load = useCallback(
    () =>
      Promise.all([
        api.get<F941>(`/payroll/us/reports/941?year=${year}&quarter=${quarter}`),
        api.get<F940>(`/payroll/us/reports/940?year=${year}`),
        api.get<W2>(`/payroll/us/reports/w2?year=${year}`),
      ])
        .then(([a, b, c]) => { setF941(a.data); setF940(b.data); setW2(c.data); setError(''); })
        .catch((err) => setError(errMsg(err, 'Could not load the form figures'))),
    [year, quarter],
  );
  useEffect(() => { load(); }, [load]);
  // D70: the state forms, for a registered state (or the company's state).
  useEffect(() => {
    api.get<{ registrations: { state: string }[] }>('/payroll/us/states').then((r) => {
      const list = r.data.registrations.map((x) => x.state);
      setRegistered(list);
      setState((cur) => cur || list[0] || '');
    }).catch(() => setRegistered([]));
  }, []);
  useEffect(() => {
    if (!state) return;
    Promise.all([
      api.get<StateQuarter>(`/payroll/us/reports/state?state=${state}&year=${year}&quarter=${quarter}`),
      api.get<StateAnnual>(`/payroll/us/reports/state-annual?state=${state}&year=${year}`),
    ]).then(([a, b]) => { setSq(a.data); setSa(b.data); }).catch((err) => setError(errMsg(err, 'Could not load the state figures')));
  }, [state, year, quarter]);

  return (
    <div>
      <div className="mb-3"><button onClick={() => navigate('/payroll/us')} className="btn-link text-xs">← US payroll</button></div>
      <PageHeader
        title="US payroll tax forms"
        action={
          <div className="flex items-end gap-2">
            <label className="block">
              <span className="text-xs font-medium text-gray-500">Year</span>
              <input type="number" min="2000" max="2100" value={year} onChange={(e) => setYear(Number(e.target.value))} style={{ width: 100 }} />
            </label>
            <label className="block">
              <span className="text-xs font-medium text-gray-500">Quarter</span>
              <select value={quarter} onChange={(e) => setQuarter(Number(e.target.value))}>
                {[1, 2, 3, 4].map((q) => <option key={q} value={q}>Q{q}</option>)}
              </select>
            </label>
            <label className="block">
              <span className="text-xs font-medium text-gray-500">State forms</span>
              <select value={state} onChange={(e) => setState(e.target.value)} aria-label="State forms">
                <option value="">Choose a state</option>
                {registered.map((st) => <option key={`r-${st}`} value={st}>{st}</option>)}
                {US_STATES.filter((st) => !registered.includes(st)).map((st) => <option key={st} value={st}>{st}</option>)}
              </select>
            </label>
          </div>
        }
      />
      <p className="text-xs text-gray-500 mb-4">
        From posted and paid US pay runs, counted by pay date. Nothing is filed from here — copy the figures onto the IRS forms or into your e-file service.
        {f941?.employer.ein ? ` EIN ${f941.employer.ein}.` : ' Add your EIN under US payroll settings.'}
      </p>
      {error && <div className="card mb-4 text-sm text-red-600">{error}</div>}

      {f941 && (
        <div className="card mb-5">
          <h2 className="text-sm font-semibold text-gray-700 mb-1">Form 941 — Q{quarter} {year}</h2>
          <p className="text-xs text-gray-500 mb-3">{f941.payRuns} pay run{f941.payRuns === 1 ? '' : 's'} paid in the quarter.</p>
          <table className="text-sm w-full">
            <tbody>
              <Line n="1" label="Employees paid for the pay period including the 12th of the last month" v={String(f941.line1)} />
              <Line n="2" label="Wages, tips and other compensation" v={usd(f941.line2)} />
              <Line n="3" label="Federal income tax withheld" v={usd(f941.line3)} />
              <Line n="5a" label={`Taxable social security wages ${usd(f941.line5a.wages)} × 0.124`} v={usd(f941.line5a.tax)} />
              <Line n="5c" label={`Taxable Medicare wages ${usd(f941.line5c.wages)} × 0.029`} v={usd(f941.line5c.tax)} />
              <Line n="5d" label={`Wages subject to Additional Medicare ${usd(f941.line5d.wages)} × 0.009`} v={usd(f941.line5d.tax)} />
              <Line n="5e" label="Total social security and Medicare taxes" v={usd(f941.line5e)} />
              <Line n="6" label="Total taxes before adjustments" v={usd(f941.line6)} />
              <Line n="7" label="Adjustment for fractions of cents" v={usd(f941.line7)} />
              <Line n="10" label="Total taxes after adjustments" v={usd(f941.line10)} strong />
              <Line n="12" label="Total taxes after credits" v={usd(f941.line12)} strong />
              <Line n="13" label="Total deposits for the quarter (recorded here)" v={usd(f941.line13)} />
              {f941.line14 > 0 && <Line n="14" label="Balance due" v={usd(f941.line14)} strong />}
              {f941.line15 > 0 && <Line n="15" label="Overpayment" v={usd(f941.line15)} />}
              {f941.line16.map((m, i) => <Line key={m.month} n={i === 0 ? '16' : ''} label={`Tax liability — ${monthName(m.month)}`} v={usd(m.liability)} />)}
              <Line n="" label="Total liability for the quarter" v={usd(f941.line16Total)} />
            </tbody>
          </table>
          <p className="text-xs text-gray-500 mt-2">Line 16 is for monthly schedule depositors; semiweekly depositors use Schedule B with the same liabilities by pay date.</p>
        </div>
      )}

      {f940 && (
        <div className="card mb-5">
          <h2 className="text-sm font-semibold text-gray-700 mb-3">Form 940 (FUTA) — {year}</h2>
          <table className="text-sm w-full">
            <tbody>
              <Line n="3" label="Total payments to all employees" v={usd(f940.line3)} />
              <Line n="4" label="Payments exempt from FUTA tax (section 125)" v={usd(f940.line4)} />
              <Line n="5" label="Payments over $7,000 to each employee" v={usd(f940.line5)} />
              <Line n="6" label="Subtotal (4 + 5)" v={usd(f940.line6)} />
              <Line n="7" label="Total taxable FUTA wages" v={usd(f940.line7)} />
              <Line n="8" label="FUTA tax before adjustments (× 0.006)" v={usd(f940.line8)} />
              <Line n="11" label="Credit reduction" v={usd(f940.line11)} />
              <Line n="12" label="Total FUTA tax after adjustments" v={usd(f940.line12)} strong />
              <Line n="13" label="FUTA tax deposited (recorded here)" v={usd(f940.line13)} />
              {f940.line14 > 0 && <Line n="14" label="Balance due" v={usd(f940.line14)} strong />}
              {f940.line15 > 0 && <Line n="15" label="Overpayment" v={usd(f940.line15)} />}
              {f940.line16.map((q) => <Line key={q.quarter} n={`16${'abcd'[q.quarter - 1]}`} label={`Liability, quarter ${q.quarter}`} v={usd(q.liability)} />)}
            </tbody>
          </table>
        </div>
      )}

      {sq && state && (
        <div className="card mb-5" aria-label="State quarter">
          <h2 className="text-sm font-semibold text-gray-700 mb-1">{state} — Q{quarter} {year}</h2>
          <p className="text-xs text-gray-500 mb-3">
            {sq.accountNumbers.withholdingAccountNo ? `Withholding no. ${sq.accountNumbers.withholdingAccountNo}. ` : ''}
            {sq.accountNumbers.unemploymentAccountNo ? `Unemployment no. ${sq.accountNumbers.unemploymentAccountNo}. ` : ''}
            {!sq.accountNumbers.registered && 'Add your account numbers under US payroll settings → States you are registered in. '}
            The figures for the state's quarterly returns; nothing is filed from here.
          </p>
          <h3 className="text-xs font-semibold text-gray-700 mb-1">Income tax withholding return</h3>
          <table className="text-sm w-full mb-4"><tbody>
            {sq.withholding.months.map((mo) => <Line key={mo.month} n="" label={`${monthName(mo.month)} — withheld (on wages ${usd(mo.wages)})`} v={usd(mo.withheld)} />)}
            <Line n="" label={`Total withheld for the quarter (${sq.withholding.employees} employee${sq.withholding.employees === 1 ? '' : 's'}, wages ${usd(sq.withholding.wages)})`} v={usd(sq.withholding.withheld)} strong />
            <Line n="" label="Paid to the state (recorded here)" v={usd(sq.withholding.deposits)} />
          </tbody></table>
          <h3 className="text-xs font-semibold text-gray-700 mb-1">Unemployment wage report</h3>
          <div className="overflow-x-auto mb-2"><table className="min-w-full text-xs">
            <thead><tr className="text-gray-500 uppercase tracking-wide border-b border-gray-200">
              <th className="text-left px-2 py-2">Employee</th><th className="text-right px-2 py-2">Gross wages</th><th className="text-right px-2 py-2">Excess wages</th>
              <th className="text-right px-2 py-2">Taxable wages</th><th className="text-right px-2 py-2">Employer contributions</th><th className="text-right px-2 py-2">Employee contributions</th>
            </tr></thead>
            <tbody>
              {sq.unemployment.employees.length === 0 && <tr><td colSpan={6} className="px-2 py-4 text-center text-gray-400">No unemployment wages for {state} in the quarter.</td></tr>}
              {sq.unemployment.employees.map((e) => (
                <tr key={e.employeeCode} className="border-b border-gray-100">
                  <td className="px-2 py-2">{e.name} <span className="text-gray-400 font-mono">{e.employeeCode}</span></td>
                  {[e.grossWages, e.excessWages, e.taxableWages, e.employerContributions, e.employeeContributions].map((v, i) => <td key={i} className="px-2 py-2 text-right tabular-nums">{usd(v)}</td>)}
                </tr>
              ))}
            </tbody>
            <tfoot><tr className="font-semibold text-gray-800"><td className="px-2 py-2">Totals</td>
              {[sq.unemployment.grossWages, sq.unemployment.excessWages, sq.unemployment.taxableWages, sq.unemployment.employerContributions, sq.unemployment.employeeContributions].map((v, i) => <td key={i} className="px-2 py-2 text-right tabular-nums">{usd(v)}</td>)}
            </tr></tfoot>
          </table></div>
          <p className="text-xs text-gray-500 mb-4">
            Employees on the payroll for the pay period including the 12th: {sq.unemployment.employeesOnThe12th.map((x) => `${monthName(x.date.slice(0, 7))} ${x.employees}`).join(' · ')}.
            {sq.unemployment.ratesUsed.length > 0 && ` Rate used: ${sq.unemployment.ratesUsed.map((r) => `${r.rate}%${r.surcharges ? ` + ${r.surcharges}% charges` : ''}`).join(', ')}.`}
            {` Paid to the state (recorded here): ${usd(sq.unemployment.deposits)}.`}
          </p>
          {(sq.leave.length > 0 || sq.local.length > 0) && (
            <>
              <h3 className="text-xs font-semibold text-gray-700 mb-1">Disability / paid leave and local taxes</h3>
              <table className="text-sm w-full mb-2"><tbody>
                {[...sq.leave, ...sq.local].map((x) => (
                  <Line key={x.id ?? x.code} n="" label={`${x.name} — wages ${usd(x.wages)}, employee ${usd(x.employee)}${x.employer ? `, employer ${usd(x.employer)}` : ''}; paid ${usd(x.deposits)}`} v={usd(x.employee + x.employer)} />
                ))}
              </tbody></table>
            </>
          )}
          {sa && (
            <>
              <h3 className="text-xs font-semibold text-gray-700 mb-1 mt-3">Annual reconciliation — {year}</h3>
              <table className="text-sm w-full"><tbody>
                {sa.quarters.map((q) => <Line key={q.quarter} n={`Q${q.quarter}`} label={`Withheld (paid ${usd(q.deposits)}); unemployment ${usd(q.unemploymentContributions)} on ${usd(q.unemploymentTaxableWages)}`} v={usd(q.withheld)} />)}
                <Line n="" label="Total withheld for the year" v={usd(sa.totals.withheld)} strong />
                <Line n="" label={`W-2s with ${state} boxes 15–17: ${sa.w2.forms}, state wages ${usd(sa.w2.stateWages)}`} v={usd(sa.w2.stateTax)} />
              </tbody></table>
              <p className={`text-xs mt-1 ${sa.balanced ? 'text-gray-500' : 'text-amber-700'}`}>{sa.balanced ? 'The W-2 state tax agrees with the tax withheld.' : 'The W-2 state tax differs from the tax withheld — check the runs.'}</p>
            </>
          )}
        </div>
      )}

      {w2 && (
        <div className="card">
          <h2 className="text-sm font-semibold text-gray-700 mb-3">W-2 figures — {year}</h2>
          <div className="overflow-x-auto">
            <table className="min-w-full text-xs">
              <thead>
                <tr className="text-gray-500 uppercase tracking-wide border-b border-gray-200">
                  <th className="text-left px-2 py-2">Employee</th>
                  <th className="text-right px-2 py-2">1 Wages</th><th className="text-right px-2 py-2">2 Fed. tax</th>
                  <th className="text-right px-2 py-2">3 SS wages</th><th className="text-right px-2 py-2">4 SS tax</th>
                  <th className="text-right px-2 py-2">5 Medicare wages</th><th className="text-right px-2 py-2">6 Medicare tax</th>
                  <th className="text-right px-2 py-2">12 D 401(k)</th>
                  <th className="text-right px-2 py-2">15 State</th><th className="text-right px-2 py-2">16 State wages</th>
                  <th className="text-right px-2 py-2">17 State tax</th>
                </tr>
              </thead>
              <tbody>
                {w2.employees.length === 0 && <tr><td colSpan={11} className="px-2 py-6 text-center text-gray-400">No US wages paid in {year}.</td></tr>}
                {w2.employees.map((e) => (
                  <tr key={e.employeeCode} className="border-b border-gray-100">
                    <td className="px-2 py-2">
                      <div className="font-medium text-gray-900">{e.name}</div>
                      <div className="text-gray-400 font-mono">{e.employeeCode} · {e.workState} · {e.paychecks} paycheck{e.paychecks === 1 ? '' : 's'}</div>
                      {(e.box14 ?? []).length > 0 && <div className="text-gray-500">14: {(e.box14 ?? []).map((x) => `${x.label} ${usd(x.amount)}`).join(' · ')}</div>}
                      {(e.locals ?? []).map((x) => <div key={x.code} className="text-gray-500">18–20: {usd(x.box18)} / {usd(x.box19)} / {x.box20}</div>)}
                    </td>
                    {[e.box1, e.box2, e.box3, e.box4, e.box5, e.box6, e.box12D].map((v, i) => (
                      <td key={i} className="px-2 py-2 text-right tabular-nums">{usd(v)}</td>
                    ))}
                    <td className="px-2 py-2 text-right">{e.box15}</td>
                    <td className="px-2 py-2 text-right tabular-nums">{usd(e.box16)}</td>
                    <td className="px-2 py-2 text-right tabular-nums">{usd(e.box17)}</td>
                  </tr>
                ))}
              </tbody>
              {w2.employees.length > 0 && (
                <tfoot>
                  <tr className="font-semibold text-gray-800">
                    <td className="px-2 py-2">Totals (W-3)</td>
                    {['box1', 'box2', 'box3', 'box4', 'box5', 'box6', 'box12D'].map((k) => (
                      <td key={k} className="px-2 py-2 text-right tabular-nums">{usd(w2.totals[k])}</td>
                    ))}
                    <td />
                    <td className="px-2 py-2 text-right tabular-nums">{usd(w2.totals.box16)}</td>
                    <td className="px-2 py-2 text-right tabular-nums">{usd(w2.totals.box17)}</td>
                  </tr>
                </tfoot>
              )}
            </table>
          </div>
          <p className="text-xs text-gray-500 mt-2">Box 15 is the employee's work state (add your state ID number on the form). Section 125 amounts are not wages on the W-2. Box 14 lists state disability and paid leave the employee paid; boxes 18–20 (local wages, local tax, locality) are shown under each employee.</p>
        </div>
      )}
    </div>
  );
}

function Line({ n, label, v, strong = false }: { n: string; label: string; v: string; strong?: boolean }) {
  return (
    <tr className="border-t border-gray-100">
      <td className="py-1.5 pr-3 text-xs text-gray-400 w-10">{n}</td>
      <td className={`py-1.5 ${strong ? 'font-semibold text-gray-900' : 'text-gray-700'}`}>{label}</td>
      <td className={`py-1.5 text-right tabular-nums ${strong ? 'font-semibold text-gray-900' : 'text-gray-800'}`}>{v}</td>
    </tr>
  );
}
