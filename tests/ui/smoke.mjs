// UI smoke test: drives the real React frontend in headless Microsoft Edge (Chrome DevTools
// Protocol, no npm dependencies) against a running accounting-server.
//
//   node tests/ui/smoke.mjs http://127.0.0.1:47800 <screenshot-dir>
//
// The server must have a fresh database with ADMIN_USERNAME=admin / ADMIN_PASSWORD=admin12345.

import { spawn } from 'node:child_process';
import { createHmac } from 'node:crypto';
import { mkdirSync, writeFileSync, mkdtempSync, readdirSync, statSync, readFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const BASE = process.argv[2] ?? 'http://127.0.0.1:47800';
const SHOTS = process.argv[3] ?? join(tmpdir(), 'accounting-ui-smoke');
mkdirSync(SHOTS, { recursive: true });
const EDGE = 'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe';
const PORT = 9333;
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

function totp(secret) {
  const alphabet = 'ABCDEFGHIJKLMNOPQRSTUVWXYZ234567';
  let bits = '';
  for (const c of secret.toUpperCase()) bits += alphabet.indexOf(c).toString(2).padStart(5, '0');
  const key = [];
  for (let i = 0; i + 8 <= bits.length; i += 8) key.push(parseInt(bits.slice(i, i + 8), 2));
  const buf = Buffer.alloc(8);
  buf.writeBigUInt64BE(BigInt(Math.floor(Date.now() / 30000)));
  const h = createHmac('sha1', Buffer.from(key)).update(buf).digest();
  const o = h[h.length - 1] & 0xf;
  return String((h.readUInt32BE(o) & 0x7fffffff) % 1_000_000).padStart(6, '0');
}

const profile = mkdtempSync(join(tmpdir(), 'edge-smoke-'));
const edge = spawn(EDGE, [
  '--headless=new', `--remote-debugging-port=${PORT}`, `--user-data-dir=${profile}`,
  '--no-first-run', '--disable-extensions', '--window-size=1366,900', 'about:blank',
], { stdio: 'ignore' });

let ws;
let nextId = 1;
const pending = new Map();
const failures = [];
const httpErrors = [];
const consoleErrors = [];
const results = [];

async function connect() {
  for (let i = 0; i < 50; i++) {
    try {
      const targets = await (await fetch(`http://127.0.0.1:${PORT}/json`)).json();
      const page = targets.find((t) => t.type === 'page');
      if (page) {
        ws = new WebSocket(page.webSocketDebuggerUrl);
        await new Promise((res, rej) => { ws.onopen = res; ws.onerror = rej; });
        ws.onmessage = (ev) => {
          const msg = JSON.parse(ev.data);
          if (msg.id && pending.has(msg.id)) {
            const { resolve, reject } = pending.get(msg.id);
            pending.delete(msg.id);
            msg.error ? reject(new Error(msg.error.message)) : resolve(msg.result);
          } else if (msg.method === 'Network.responseReceived') {
            const r = msg.params.response;
            if (r.status >= 400 && r.url.includes('/api/')) httpErrors.push(`${r.status} ${new URL(r.url).pathname}`);
          } else if (msg.method === 'Runtime.consoleAPICalled' && msg.params.type === 'error') {
            consoleErrors.push(msg.params.args.map((a) => a.value ?? a.description ?? '').join(' ').slice(0, 200));
          }
        };
        return;
      }
    } catch { /* Edge still starting */ }
    await sleep(200);
  }
  throw new Error('could not connect to Edge');
}

const send = (method, params = {}) => new Promise((resolve, reject) => {
  const id = nextId++;
  pending.set(id, { resolve, reject });
  ws.send(JSON.stringify({ id, method, params }));
});
const evaluate = async (expr) => {
  const r = await send('Runtime.evaluate', { expression: expr, awaitPromise: true, returnByValue: true });
  if (r.exceptionDetails) throw new Error(r.exceptionDetails.text + ' ' + (r.exceptionDetails.exception?.description ?? ''));
  return r.result.value;
};
async function waitFor(expr, label, timeout = 10000) {
  const end = Date.now() + timeout;
  while (Date.now() < end) {
    if (await evaluate(expr).catch(() => false)) return;
    await sleep(150);
  }
  throw new Error(`timed out waiting for ${label}`);
}
async function type(selector, text) {
  await evaluate(`(() => { const el = document.querySelector(${JSON.stringify(selector)}); el.focus(); el.select?.(); return true; })()`);
  await send('Input.insertText', { text });
}
const clickText = (text, tag = 'button') => evaluate(`(() => {
  const el = [...document.querySelectorAll(${JSON.stringify(tag)})].find((b) => b.textContent.trim() === ${JSON.stringify(text)});
  if (!el) throw new Error('no ${tag} with text ' + ${JSON.stringify(text)});
  el.click(); return true; })()`);
const pageText = () => evaluate('document.body.innerText');
async function shot(name) {
  const { data } = await send('Page.captureScreenshot', { format: 'png' });
  writeFileSync(join(SHOTS, `${name}.png`), Buffer.from(data, 'base64'));
}
async function go(path) {
  await send('Page.navigate', { url: BASE + path });
  await waitFor(`document.readyState === 'complete' && location.pathname === ${JSON.stringify(path)}`, `load ${path}`);
  await sleep(600);
}
async function check(name, fn) {
  try {
    await fn();
    results.push(`PASS  ${name}`);
  } catch (e) {
    failures.push(name);
    results.push(`FAIL  ${name}: ${e.message}`);
    await shot(`FAIL-${name.replace(/\W+/g, '_')}`).catch(() => {});
  }
}

try {
  await connect();
  await send('Page.enable');
  await send('Runtime.enable');
  await send('Network.enable');
  let secret = '';

  await check('login page loads from the Rust server', async () => {
    await go('/login');
    await waitFor(`!!document.querySelector('input[type=password]')`, 'login form');
    await shot('01-login');
  });

  await check('password login leads to 2FA setup with QR and secret', async () => {
    await type('input[type=text]', 'admin');
    await type('input[type=password]', 'admin12345');
    await clickText('Sign In').catch(() => evaluate(`document.querySelector('button[type=submit]').click()`));
    await waitFor(`location.pathname === '/2fa/setup'`, '2FA setup page');
    await waitFor(`!!document.querySelector('p.font-mono')?.textContent.trim()`, 'secret text');
    await waitFor(`!!document.querySelector('img[src^="data:image/png"]')`, 'QR image');
    secret = await evaluate(`document.querySelector('p.font-mono').textContent.trim()`);
    await shot('02-2fa-setup');
  });

  await check('TOTP code enables 2FA and opens the app', async () => {
    await type('input[inputmode=numeric]', totp(secret));
    await evaluate(`document.querySelector('button[type=submit]').click()`);
    await waitFor(`location.pathname === '/' && !!localStorage.getItem('accessToken')`, 'dashboard');
    await sleep(1200);
    await shot('03-dashboard');
  });

  await check('Users page lists admin and creates a user through the form', async () => {
    await go('/users');
    await waitFor(`document.body.innerText.includes('admin')`, 'admin row');
    await clickText('+ New User');
    await waitFor(`!!document.querySelector('input[placeholder=Username]')`, 'new user form');
    await type('input[placeholder=Username]', 'clerk.one');
    await type('input[placeholder="Password (min 8 chars)"]', 'clerkpass1');
    await evaluate(`(() => { const sel = document.querySelector('form select'); if (sel) { const o = [...sel.options].find((x) => x.textContent.includes('Viewer')); if (o) { sel.value = o.value; sel.dispatchEvent(new Event('change', { bubbles: true })); } } return true; })()`);
    await clickText('Save');
    await waitFor(`document.body.innerText.includes('clerk.one')`, 'new user row');
    await shot('04-users');
  });

  await check('Roles page shows the seeded roles', async () => {
    await go('/roles');
    await waitFor(`['Admin','Accountant','Viewer'].every((r) => document.body.innerText.includes(r))`, 'roles');
    await shot('05-roles');
  });

  await check('Settings page saves the signatory', async () => {
    await go('/settings');
    await waitFor(`!!document.querySelector('input[placeholder="e.g. Samir Husain"]')`, 'settings form');
    await type('input[placeholder="e.g. Samir Husain"]', 'Jane Doe');
    await evaluate(`document.querySelector('button[type=submit]').click()`);
    await sleep(800);
    const saved = await evaluate(`fetch('/api/settings', { headers: { authorization: 'Bearer ' + localStorage.getItem('accessToken') } }).then((r) => r.json())`);
    if (saved.signatoryName !== 'Jane Doe') throw new Error(`signatoryName is ${saved.signatoryName}`);
    await shot('06-settings');
  });

  await check('Activity log shows the audited actions', async () => {
    await go('/audit');
    await waitFor(`['Created an user', 'Updated company settings', 'Logged in'].every((a) => document.body.innerText.includes(a))`, 'audit rows');
    await shot('07-audit');
  });

  // ── W1 screens ───────────────────────────────────────────────────────────
  const setSelect = (text) => evaluate(`(() => { const sel = document.querySelector('form select');
    sel.value = ${JSON.stringify(text)}; sel.dispatchEvent(new Event('change', { bubbles: true })); return true; })()`);
  async function newAccount(code, name, accountType) {
    await clickText('+ New Account');
    await waitFor(`!!document.querySelector('input[placeholder="Code (e.g. 1000)"]')`, 'account form');
    await type('input[placeholder="Code (e.g. 1000)"]', code);
    await type('input[placeholder="Account Name"]', name);
    await setSelect(accountType);
    await clickText('Save Account');
    await waitFor(`document.body.innerText.includes(${JSON.stringify(name)})`, `account ${name} listed`);
  }

  await check('Accounts page creates accounts through the form', async () => {
    await go('/accounts');
    await waitFor(`document.body.innerText.includes('Salaries Payable')`, 'seeded payroll accounts');
    await newAccount('1000', 'Cash', 'ASSET');
    await newAccount('3000', 'Owners Capital', 'EQUITY');
    await newAccount('9999', 'Temporary', 'EXPENSE');
    await shot('09-accounts');
  });

  await check('Accounts page deletes an unused account', async () => {
    await evaluate(`window.confirm = () => true`);
    await evaluate(`(() => {
      const row = [...document.querySelectorAll('tr')].find((tr) => tr.innerText.includes('Temporary'));
      [...row.querySelectorAll('button')].find((b) => b.textContent.trim() === 'Delete').click(); return true; })()`);
    await waitFor(`!document.body.innerText.includes('Temporary')`, 'account removed');
  });

  await check('Ledger page lists a posted journal entry with its lines', async () => {
    const posted = await evaluate(`(async () => {
      const h = { 'content-type': 'application/json', authorization: 'Bearer ' + localStorage.getItem('accessToken') };
      const accounts = await (await fetch('/api/accounts', { headers: h })).json();
      const id = (code) => accounts.find((a) => a.code === code).id;
      const r = await fetch('/api/ledger/entries', { method: 'POST', headers: h, body: JSON.stringify({
        date: '2026-04-01', description: 'Opening capital', reference: 'OPEN-1',
        lines: [{ accountId: id('1000'), debit: 1000.5, credit: 0 }, { accountId: id('3000'), debit: 0, credit: 1000.5 }] }) });
      return r.status;
    })()`);
    if (posted !== 201) throw new Error(`posting returned ${posted}`);
    await go('/ledger');
    await waitFor(`document.body.innerText.includes('JE-000001') && document.body.innerText.includes('Opening capital')`, 'entry row');
    await shot('10-ledger');
  });

  await check('Trial Balance page shows balanced totals from the ledger', async () => {
    await go('/reports/trial-balance');
    await waitFor(`['Cash', 'Owners Capital'].every((t) => document.body.innerText.includes(t))`, 'trial balance rows');
    const text = await pageText();
    if (!/1,000\.50/.test(text)) throw new Error('amount 1,000.50 not shown');
    await shot('11-trial-balance');
  });

  // ── W2 screens ───────────────────────────────────────────────────────────
  // Row actions report failures with alert() and ask with confirm(); both would block CDP.
  const quietDialogs = () => evaluate(`window.confirm = () => true; window.__alerts = []; window.alert = (m) => window.__alerts.push(String(m)); true`);
  /** Picks, in the nth <select> of the open form, the option whose text contains `text`. */
  const pick = (n, text) => evaluate(`(() => {
    const sel = document.querySelectorAll('form select')[${n}];
    const o = [...sel.options].find((x) => x.textContent.includes(${JSON.stringify(text)}));
    if (!o) throw new Error('no option ' + ${JSON.stringify(text)});
    sel.value = o.value; sel.dispatchEvent(new Event('change', { bubbles: true })); return true; })()`);
  const api = (method, path, body) => evaluate(`(async () => {
    const r = await fetch(${JSON.stringify(path)}, { method: ${JSON.stringify(method)}, headers: { 'content-type': 'application/json',
      authorization: 'Bearer ' + localStorage.getItem('accessToken') }, body: ${body ? JSON.stringify(JSON.stringify(body)) : 'undefined'} });
    const t = await r.text(); return { status: r.status, body: t ? JSON.parse(t) : null };
  })()`);
  /** Sets every empty required date input of the open form (React needs the native setter). */
  const fillDates = (value, all = false) => evaluate(`(() => {
    const set = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set;
    for (const el of document.querySelectorAll('form input[type=date]')) {
      if ((${all} || el.required) && !el.value) { set.call(el, ${JSON.stringify(value)}); el.dispatchEvent(new Event('input', { bubbles: true })); }
    }
    return true; })()`);
  const rowAction = (rowText, label) => evaluate(`(() => {
    const row = [...document.querySelectorAll('tr')].find((tr) => tr.innerText.includes(${JSON.stringify(rowText)}));
    if (!row) throw new Error('no row ' + ${JSON.stringify(rowText)});
    const b = [...row.querySelectorAll('button')].find((x) => x.textContent.trim() === ${JSON.stringify(label)});
    if (!b) throw new Error('no button ' + ${JSON.stringify(label)}); b.click(); return true; })()`);

  await check('Seeding adds the missing default accounts (D10)', async () => {
    const r = await api('POST', '/api/accounts/seed');
    if (r.status !== 204) throw new Error('seed returned ' + r.status);
    const list = await api('GET', '/api/accounts');
    const codes = list.body.map((a) => a.code);
    for (const c of ['1100', '2000', '4000', '6000']) if (!codes.includes(c)) throw new Error('missing ' + c);
    if (list.body.filter((a) => a.code === '1000').length !== 1) throw new Error('existing 1000 must not be duplicated');
  });

  await check('Customers and Vendors pages create records through their forms', async () => {
    await go('/customers');
    await clickText('+ New Customer');
    await type('input[placeholder="Full Name"]', 'Acme Ltd');
    await type('input[placeholder="Email"]', 'ap@acme.lk');
    await clickText('Save');
    await waitFor(`document.body.innerText.includes('Acme Ltd')`, 'customer row');
    await go('/vendors');
    await clickText('+ New Vendor');
    await type('input[placeholder="Vendor Name"]', 'Paper Co');
    // The reference rejects an empty email (IsEmail on ""), so the form needs one.
    await type('input[placeholder="Email"]', 'billing@paper.lk');
    await clickText('Save');
    await waitFor(`document.body.innerText.includes('Paper Co')`, 'vendor row');
    await shot('12-vendors');
  });

  await check('Invoices page posts an invoice and downloads its PDF', async () => {
    const accounts = (await api('GET', '/api/accounts')).body;
    const rev = accounts.find((a) => a.code === '4000').id;
    const cust = (await api('GET', '/api/customers')).body.find((c) => c.name === 'Acme Ltd').id;
    const r = await api('POST', '/api/invoices', { customerId: cust, date: '2026-04-01', paymentTerms: 'NET_30',
      lines: [{ description: 'Design work', quantity: 2.5, unitPrice: 1000.25, accountId: rev }] });
    if (r.status !== 201) throw new Error('create returned ' + r.status);
    await go('/invoices');
    await quietDialogs();
    await waitFor(`document.body.innerText.includes('INV-9000')`, 'invoice row');
    await evaluate(`(() => {
      const sel = [...document.querySelectorAll('select')].find((x) => x.title.startsWith('Change the status'));
      sel.value = 'POSTED'; sel.dispatchEvent(new Event('change', { bubbles: true })); return true; })()`);
    await waitFor(`(async () => (await fetch('/api/invoices', { headers: { authorization: 'Bearer ' + localStorage.getItem('accessToken') } }).then((x) => x.json())).data[0].status === 'POSTED')()`, 'invoice posted');
    const dl = join(SHOTS, 'downloads');
    mkdirSync(dl, { recursive: true });
    await send('Page.setDownloadBehavior', { behavior: 'allow', downloadPath: dl });
    await go('/invoices');
    await evaluate(`(() => { const row = [...document.querySelectorAll('tr')].find((tr) => tr.innerText.includes('INV-9000'));
      const b = [...row.querySelectorAll('button')].find((x) => x.textContent.includes('⋮') || x.getAttribute('aria-label'));
      b.click(); return true; })()`);
    await waitFor(`[...document.querySelectorAll('button,[role=menuitem]')].some((x) => x.textContent.trim() === 'Download')`, 'action menu');
    await evaluate(`[...document.querySelectorAll('button,[role=menuitem]')].find((x) => x.textContent.trim() === 'Download').click()`);
    const end = Date.now() + 20000;
    let pdf;
    while (Date.now() < end && !pdf) {
      pdf = readdirSync(dl).find((f) => f.endsWith('.pdf'));
      await sleep(250);
    }
    if (!pdf) throw new Error('no PDF downloaded');
    await sleep(500);
    const bytes = readFileSync(join(dl, pdf));
    if (statSync(join(dl, pdf)).size < 2000 || bytes.subarray(0, 5).toString() !== '%PDF-') throw new Error('PDF looks empty');
    await shot('13-invoices');
  });

  await check('Payments page records a customer payment and the invoice becomes PAID', async () => {
    await go('/payments');
    await quietDialogs();
    await clickText('+ Record Payment');
    await waitFor(`[...(document.querySelectorAll('form select')[1]?.options ?? [])].some((o) => o.textContent.includes('INV-9000'))`, 'invoice option');
    await pick(1, 'INV-9000');
    await type('input[placeholder="Amount"]', '2500.63');
    await pick(3, '1000');
    await fillDates('2026-04-25');
    await clickText('Record Payment');
    await waitFor(`document.body.innerText.includes('PAY-000001')`, 'payment row');
    const inv = (await api('GET', '/api/invoices')).body.data[0];
    if (inv.status !== 'PAID' || inv.amountPaid !== '2500.63') throw new Error('invoice is ' + inv.status + ' / ' + inv.amountPaid);
    await shot('14-payments');
  });

  await check('Bills page enters and posts a vendor bill', async () => {
    await go('/bills');
    await quietDialogs();
    await clickText('+ New Bill');
    await waitFor(`[...(document.querySelectorAll('form select')[0]?.options ?? [])].some((o) => o.textContent.includes('Paper Co'))`, 'vendor option');
    await pick(0, 'Paper Co');
    await type('input[placeholder="Item description"]', 'Printer paper');
    await evaluate(`(() => { const nums = document.querySelectorAll('form input[type=number]'); nums[0].focus(); nums[0].select(); return true; })()`);
    await send('Input.insertText', { text: '3' });
    await evaluate(`(() => { const nums = document.querySelectorAll('form input[type=number]'); nums[1].focus(); return true; })()`);
    await send('Input.insertText', { text: '12.5' });
    await pick(1, '6000');
    // The reference rejects an empty due date (IsDateFormat on ""), so the form needs one too.
    await fillDates('2026-04-03', true);
    await clickText('Save Bill');
    await waitFor(`document.body.innerText.includes('BILL-000001')`, 'bill row');
    await rowAction('BILL-000001', 'Post');
    await waitFor(`[...document.querySelectorAll('tr')].some((tr) => tr.innerText.includes('BILL-000001') && tr.innerText.includes('POSTED'))`, 'bill posted');
    await shot('15-bills');
  });

  await check('Expenses page records and posts an expense', async () => {
    await go('/expenses');
    await quietDialogs();
    await clickText('+ New Expense');
    await type('input[placeholder="Description"]', 'Taxi');
    await type('input[placeholder="Amount"]', '1500.75');
    await pick(0, '6000');
    await pick(1, '1000');
    await fillDates('2026-04-08');
    await clickText('Save');
    await waitFor(`document.body.innerText.includes('EXP-000001')`, 'expense row');
    await rowAction('EXP-000001', 'Post');
    await waitFor(`[...document.querySelectorAll('tr')].some((tr) => tr.innerText.includes('EXP-000001') && tr.innerText.includes('POSTED'))`, 'expense posted');
  });

  await check('Trial balance still balances after the day\'s work', async () => {
    const tb = (await api('GET', '/api/ledger/trial-balance')).body;
    const cents = (k) => tb.reduce((sum, r) => sum + Math.round(r[k] * 100), 0);
    if (cents('debit') !== cents('credit')) throw new Error(`debits ${cents('debit')} != credits ${cents('credit')}`);
    const cash = tb.find((r) => r.account.code === '1000');
    // 1000.50 opening + 2500.63 received - 1500.75 taxi
    if (cash.balance !== 2000.38) throw new Error('cash balance ' + cash.balance);
    await go('/reports/trial-balance');
    await shot('16-trial-balance-w2');
  });

  // ── W3 screens ───────────────────────────────────────────────────────────
  await check('Dashboard shows the year-to-date figures from the reports', async () => {
    const before = httpErrors.length;
    await go('/');
    await waitFor(`document.body.innerText.toLowerCase().includes('net income (ytd)') && document.body.innerText.toLowerCase().includes('ap outstanding')`, 'KPI cards');
    const text = await pageText();
    // Cards are CSS-uppercased, so innerText is too. Revenue 2,500.63 (the paid invoice);
    // AP outstanding 37.50 (the posted bill); net income 962.38 = 2,500.63 - 37.50 - 1,500.75.
    for (const v of ['2,500.63', '37.50', '962.38']) if (!text.includes(v)) throw new Error('dashboard is missing ' + v);
    const newErrors = httpErrors.slice(before).filter((e) => e.includes('/api/reports'));
    if (newErrors.length) throw new Error('report calls failed: ' + newErrors.join(', '));
    await shot('17-dashboard');
  });

  await check('Profit & Loss and Balance Sheet pages render and balance', async () => {
    await go('/reports/profit-loss');
    await waitFor(`document.body.innerText.includes('Total Revenue') && document.body.innerText.includes('Net Income')`, 'P&L sections');
    await shot('18-profit-loss');
    await go('/reports/balance-sheet');
    await waitFor(`document.body.innerText.includes('Balanced')`, 'balance indicator');
    const text = await pageText();
    if (!text.includes('✓ Balanced')) throw new Error('balance sheet is out of balance');
    await shot('19-balance-sheet');
  });

  await check('AR and AP ageing pages list what is outstanding', async () => {
    await go('/reports/ap-aging');
    await waitFor(`document.body.innerText.includes('Grand Total Outstanding')`, 'AP ageing');
    if (!(await pageText()).includes('BILL-000001')) throw new Error('posted bill missing from AP ageing');
    await shot('20-ap-aging');
    await go('/reports/ar-aging');
    await waitFor(`document.body.innerText.includes('Grand Total Outstanding')`, 'AR ageing');
  });

  await check('second login goes through 2FA verify', async () => {
    await evaluate(`localStorage.removeItem('accessToken')`);
    await go('/login');
    await type('input[type=text]', 'admin');
    await type('input[type=password]', 'admin12345');
    await evaluate(`document.querySelector('button[type=submit]').click()`);
    await waitFor(`location.pathname === '/2fa/verify'`, 'verify page');
    await type('input[inputmode=numeric]', totp(secret));
    await evaluate(`document.querySelector('button[type=submit]').click()`);
    await waitFor(`location.pathname === '/' && !!localStorage.getItem('accessToken')`, 'dashboard after verify');
  });

  await check('wrong password shows the login error', async () => {
    await evaluate(`localStorage.removeItem('accessToken')`);
    await go('/login');
    await type('input[type=text]', 'admin');
    await type('input[type=password]', 'not-the-password');
    await evaluate(`document.querySelector('button[type=submit]').click()`);
    await waitFor(`document.body.innerText.includes('Invalid username or password')`, 'error message');
    await shot('08-login-error');
  });
} catch (e) {
  failures.push('harness');
  results.push(`FAIL  harness: ${e.message}`);
} finally {
  console.log(results.join('\n'));
  console.log(`\nAPI responses >= 400 seen by the browser:\n  ${[...new Set(httpErrors)].join('\n  ') || '(none)'}`);
  console.log(`Console errors:\n  ${[...new Set(consoleErrors)].slice(0, 15).join('\n  ') || '(none)'}`);
  console.log(`Screenshots: ${SHOTS}`);
  try { ws?.close(); } catch { /* ignore */ }
  edge.kill();
  process.exit(failures.length ? 1 : 0);
}
