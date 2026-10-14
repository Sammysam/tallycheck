import { readFileSync, writeFileSync } from 'node:fs';
import { stripTypeScriptTypes } from 'node:module';
const src = readFileSync(process.argv[2], 'utf8');
const start = src.indexOf('const VERB');
const end = src.indexOf('@Injectable()');
const js = stripTypeScriptTypes(src.slice(start, end));
const { summarize, detailFrom } = new Function(js + '\nreturn { summarize, detailFrom };')();
const methods = ['POST', 'PATCH', 'PUT', 'DELETE'];
const paths = [
  '/api/users', '/api/users/abc', '/api/users/abc/reset-password', '/api/users/abc/reset-2fa',
  '/api/roles', '/api/roles/abc', '/api/settings', '/api/settings?x=1',
  '/api/accounts', '/api/accounts/seed', '/api/accounts/abc', '/api/ledger/entries', '/api/ledger/entries/abc/reverse',
  '/api/invoices', '/api/invoices/abc', '/api/invoices/abc/post', '/api/invoices/abc/void', '/api/invoices/abc/unpost',
  '/api/invoices/abc/duplicate', '/api/invoices/abc/status', '/api/bills', '/api/bills/abc/post', '/api/expenses/abc/void',
  '/api/payments', '/api/banking', '/api/banking/abc', '/api/customers', '/api/vendors/abc', '/api/employees',
  '/api/employees/abc', '/api/employees/abc/photo', '/api/employees/abc/documents', '/api/employees/abc/documents/d1',
  '/api/employees/abc/documents/d1/replace', '/api/payroll/runs', '/api/payroll/runs/r1', '/api/payroll/runs/r1/lines',
  '/api/payroll/runs/r1/lines/l1', '/api/payroll/runs/r1/post', '/api/payroll/runs/r1/pay', '/api/payroll/runs/r1/remit',
  '/api/payroll/runs/r1/void', '/api/payroll/runs/r1/slips-filed', '/api/payroll/rates', '/api/payroll/company',
  '/api/payroll/employees/e1/bank', '/api/payroll/employees/e1/salaries', '/api/payroll/employees/e1/salaries/s1',
  '/api/unknown-thing', '/api/', '/api/users/', 'api/users',
];
const summaries = [];
for (const m of methods) for (const p of paths) summaries.push([m, p, summarize(m, p)]);
const bodies = [
  { invoiceNumber: 'INV-9000', name: 'x' }, { billNumber: 'BILL-1' }, { title: 'Doc' }, { name: 'Clerk' }, { code: '1000' },
  { invoiceNumber: '', name: 'n' }, { name: 0, code: 'c' }, {}, null, 'text', [ 'a' ], { code: 5 },
];
const details = bodies.map((b) => [b, detailFrom(b) ?? null]);
writeFileSync(process.argv[3], JSON.stringify({ summaries, details }, null, 1));
console.log(summaries.length, 'summaries,', details.length, 'details');
