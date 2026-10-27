import { useCallback, useEffect, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import api, { errMsg } from '../api/client';
import PageHeader from '../components/PageHeader';
import { usd } from '../lib/usPayroll';

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
}
interface W2 { employees: W2Row[]; totals: Record<string, Money> }

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
              <span className="text-xs font-medium text-gray-500">Quarter (941)</span>
              <select value={quarter} onChange={(e) => setQuarter(Number(e.target.value))}>
                {[1, 2, 3, 4].map((q) => <option key={q} value={q}>Q{q}</option>)}
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
          <p className="text-xs text-gray-500 mt-2">Box 15 is the employee's work state (add your state ID number on the form). Section 125 amounts are not wages on the W-2. Local tax boxes (18–20) are not covered.</p>
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
