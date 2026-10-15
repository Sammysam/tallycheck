// Payroll arithmetic fixture produced by the reference's own
// backend/src/modules/payroll/payroll-rates.ts (computeLine, calcPaye, isValidSlabs).
//
//   node tools/gen_payroll_fixture.mjs <reference backend dir> <output json>
//
// The cases are deterministic (seeded generator), so re-running gives the same file.
import { readFileSync, writeFileSync } from 'node:fs';
import { stripTypeScriptTypes } from 'node:module';
import { join } from 'node:path';

const [backend, out] = process.argv.slice(2);
if (!backend || !out) throw new Error('usage: gen_payroll_fixture.mjs <backend dir> <output json>');
const src = readFileSync(join(backend, 'src', 'modules', 'payroll', 'payroll-rates.ts'), 'utf8');
const ref = await import('data:text/javascript;base64,' + Buffer.from(stripTypeScriptTypes(src)).toString('base64'));

let seed = 20260401;
const rnd = () => ((seed = (seed * 1103515245 + 12345) % 2147483648) / 2147483648);
const money = (max) => Math.round(rnd() * max * 100) / 100;
const pick = (xs) => xs[Math.floor(rnd() * xs.length)];

const RATE_SETS = [
  ref.DEFAULT_RATES,
  { ...ref.DEFAULT_RATES, stampDuty: 0, employerWelfare: 0 },
  { ...ref.DEFAULT_RATES, epfEmployeeRate: 0.085, epfEmployerRate: 0.125, etfRate: 0.035, stampDuty: 25.5 },
  { ...ref.DEFAULT_RATES, payeSlabs: [[100000, 0], [141667, 0.06], [183333, 0.12], [225000, 0.18], [266667, 0.24], [308333, 0.3], [null, 0.36]] },
  { ...ref.DEFAULT_RATES, daysInMonth: 31, payeSlabs: [[150000.5, 0.01], [null, 0.2]] },
];

const lines = [];
// The April 2026 regression row (CLAUDE.md §5): basic + BRA = 44,000, allowances 39,850.
lines.push({
  rates: 0,
  input: { epfApplicable: true, daysInMonth: 30, daysWorked: 30, basic: 40500, bra2005: 1000, bra2016: 2500,
    vehicleAllowance: 15000, otherAllowance: 24850, projectBonus: 0, arrears: 0, medical: 0, otherEarnings: 0,
    noPayAmount: 0, payeOverride: null, welfare: 300, loanRecovery: 0, salaryAdvance: 0, phoneBill: 0, otherDeductions: 0 },
});
for (let i = 0; i < 400; i++) {
  const rates = Math.floor(rnd() * RATE_SETS.length);
  const daysInMonth = RATE_SETS[rates].daysInMonth;
  const big = rnd() < 0.3;
  lines.push({
    rates,
    input: {
      epfApplicable: rnd() < 0.8,
      daysInMonth,
      daysWorked: pick([daysInMonth, daysInMonth, 0, 1, 15, 17.5, 22.25, daysInMonth - 0.5, 29.99]),
      basic: money(big ? 900000 : 120000),
      bra2005: pick([0, 1000, 1000, money(5000)]),
      bra2016: pick([0, 2500, 2500, money(5000)]),
      vehicleAllowance: pick([0, 15000, money(60000)]),
      otherAllowance: pick([0, money(80000)]),
      projectBonus: pick([0, 0, money(200000)]),
      arrears: pick([0, 0, money(20000)]),
      medical: pick([0, 0, money(10000)]),
      otherEarnings: pick([0, 0, money(5000)]),
      noPayAmount: pick([0, 0, 0, money(30000), 999999]),
      payeOverride: pick([null, null, null, 0, money(20000)]),
      welfare: pick([300, 300, 0, money(1000)]),
      loanRecovery: pick([0, 0, money(10000)]),
      salaryAdvance: pick([0, 0, money(10000)]),
      phoneBill: pick([0, money(3000)]),
      otherDeductions: pick([0, money(3000)]),
    },
  });
}
for (const l of lines) l.output = ref.computeLine(l.input, RATE_SETS[l.rates]);

const paye = [];
for (const taxable of [0, 1, 249999.99, 250000, 250000.01, 300000, 500000, 750000, 750000.01, 1234567.89, 83850]) {
  for (let r = 0; r < RATE_SETS.length; r++) paye.push({ rates: r, taxable, paye: ref.calcPaye(taxable, RATE_SETS[r].payeSlabs) });
}
for (let i = 0; i < 200; i++) {
  const r = Math.floor(rnd() * RATE_SETS.length);
  const taxable = money(2000000);
  paye.push({ rates: r, taxable, paye: ref.calcPaye(taxable, RATE_SETS[r].payeSlabs) });
}

const slabCases = [
  [], [[null, 0]], [[100, 0.1]], [[100, 0.1], [null, 0.2]], [[100, 0.1], [50, 0.2]], [[100, 1.5]],
  [[100, -0.1]], [[null, 0.1], [null, 0.2]], [[100, 0.1], [100, 0.2]], [[100, '0.1']], [[100]], 'x', [['100', 0.1]],
  [[0, 0.1]], [[100, 0.1], [200, 0.2], [150, 0.3]], [[100, 0.1], [200, 0.2], [250, 0.3]],
].map((slabs) => ({ slabs, valid: ref.isValidSlabs(slabs) }));

writeFileSync(out, JSON.stringify({ rateSets: RATE_SETS, lines, paye, slabs: slabCases }, null, 1));
console.log(`${lines.length} lines, ${paye.length} PAYE cases, ${slabCases.length} slab checks`);
