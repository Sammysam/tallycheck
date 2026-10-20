// UI smoke test: drives the real React frontend in headless Microsoft Edge (Chrome DevTools
// Protocol, no npm dependencies) against a running accounting-server.
//
//   node tests/ui/smoke.mjs http://127.0.0.1:47800 <screenshot-dir>
//
// The server must have a fresh, empty database (no ADMIN_USERNAME / ADMIN_PASSWORD): the test
// creates admin / admin12345 through the first-run screen (D17).

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
const fs_size = (path) => statSync(path).size;
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
/** Presses a real key through DevTools. modifiers: 1 Alt, 2 Ctrl, 8 Shift. */
async function press(key, code, keyCode, modifiers = 0) {
  await send('Input.dispatchKeyEvent', { type: 'rawKeyDown', key, code, windowsVirtualKeyCode: keyCode, modifiers });
  await send('Input.dispatchKeyEvent', { type: 'keyUp', key, code, windowsVirtualKeyCode: keyCode, modifiers });
}
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
    // A step that hangs (e.g. a browser file dialog) fails after 90 s instead of stalling the run.
    await Promise.race([fn(), sleep(90000).then(() => { throw new Error('step timed out after 90 s'); })]);
    results.push(`PASS  ${name}`);
    console.log(results.at(-1));
  } catch (e) {
    failures.push(name);
    results.push(`FAIL  ${name}: ${e.message}`);
    console.log(results.at(-1));
    await shot(`FAIL-${name.replace(/\W+/g, '_')}`).catch(() => {});
  }
}

try {
  await connect();
  await send('Page.enable');
  await send('Runtime.enable');
  await send('Network.enable');
  let secret = '';

  await check('a fresh install opens the create-administrator screen (D17)', async () => {
    await send('Page.navigate', { url: BASE + '/login' });
    await waitFor(`location.pathname === '/setup' && document.querySelectorAll('input[type=password]').length === 2`, 'first-run form');
    await shot('00-first-run');
  });

  await check('first-run screen checks the passwords match, then creates the admin', async () => {
    await type('input[type=text]', 'admin');
    const pw = (i, v) => evaluate(`(() => { const el = document.querySelectorAll('input[type=password]')[${i}];
      Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set.call(el, ${JSON.stringify(v)});
      el.dispatchEvent(new Event('input', { bubbles: true })); return true; })()`);
    await pw(0, 'admin12345');
    await pw(1, 'admin12346');
    await evaluate(`document.querySelector('button[type=submit]').click()`);
    await waitFor(`document.body.innerText.includes('The two passwords do not match')`, 'mismatch error');
    await pw(1, 'admin12345');
    await evaluate(`document.querySelector('button[type=submit]').click()`);
    await waitFor(`location.pathname === '/' && !!localStorage.getItem('accessToken')`, 'dashboard after setup');
    await evaluate(`localStorage.clear()`);
  });

  await check('login page loads from the Rust server (no first-run screen once set up)', async () => {
    await go('/login');
    await waitFor(`!!document.querySelector('input[type=password]')`, 'login form');
    await sleep(800);
    if ((await evaluate('location.pathname')) !== '/login') throw new Error('left the login page');
    await shot('01-login');
  });

  await check('without 2FA the password alone signs in (D16)', async () => {
    await type('input[type=text]', 'admin');
    await type('input[type=password]', 'admin12345');
    await clickText('Sign In').catch(() => evaluate(`document.querySelector('button[type=submit]').click()`));
    await waitFor(`location.pathname === '/' && !!localStorage.getItem('accessToken')`, 'dashboard without 2FA');
    await waitFor(`[...document.querySelectorAll('button')].some((b) => b.textContent.trim() === 'Turn on 2FA')`, 'Turn on 2FA offer');
  });

  await check('a signed-in user can turn 2FA on from the top bar (QR and secret)', async () => {
    await clickText('Turn on 2FA');
    await waitFor(`location.pathname === '/2fa/setup'`, '2FA setup page');
    await waitFor(`!!document.querySelector('p.font-mono')?.textContent.trim()`, 'secret text');
    await waitFor(`!!document.querySelector('img[src^="data:image/png"]')`, 'QR image');
    secret = await evaluate(`document.querySelector('p.font-mono').textContent.trim()`);
    await shot('02-2fa-setup');
  });

  await check('TOTP code enables 2FA and returns to the app', async () => {
    await type('input[inputmode=numeric]', totp(secret));
    await evaluate(`document.querySelector('button[type=submit]').click()`);
    await waitFor(`location.pathname === '/' && !!localStorage.getItem('accessToken')`, 'dashboard');
    await waitFor(`![...document.querySelectorAll('button')].some((b) => b.textContent.trim() === 'Turn on 2FA')`, 'offer gone once 2FA is on');
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
    // D20: a fresh install already has the default chart as well as the payroll accounts.
    await waitFor(`['Salaries Payable', 'Accounts Receivable', 'Accounts Payable', 'Cash'].every((t) => document.body.innerText.includes(t))`, 'default chart on a fresh install');
    await newAccount('3100', 'Owners Capital', 'EQUITY');
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
        lines: [{ accountId: id('1000'), debit: 1000.5, credit: 0 }, { accountId: id('3100'), debit: 0, credit: 1000.5 }] }) });
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

  await check('Bank reconciliation ticks the statement lines and finishes at zero (D22)', async () => {
    await go('/reconcile');
    await waitFor(`document.body.innerText.includes('Start a reconciliation') && document.querySelector('select')?.selectedOptions[0]?.textContent.includes('1000')`, 'start form for Cash');
    await evaluate(`(() => {
      const set = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set;
      const d = document.querySelector('form input[type=date]'); set.call(d, '2026-04-30'); d.dispatchEvent(new Event('input', { bubbles: true }));
      return true; })()`);
    await type('input[placeholder="Closing balance"]', '1000.5');
    await clickText('Start');
    await waitFor(`!!document.querySelector('input[aria-label="Clear JE-000001"]')`, 'opening capital listed');
    await waitFor(`document.querySelector('[data-testid=rec-difference]').innerText.includes('1,000.50')`, 'difference before ticking');
    if (!(await evaluate(`[...document.querySelectorAll('button')].find((b) => b.textContent.trim() === 'Finish').disabled`)))
      throw new Error('Finish must wait for a zero difference');
    await evaluate(`document.querySelector('input[aria-label="Clear JE-000001"]').click()`);
    await waitFor(`document.querySelector('[data-testid=rec-difference]').innerText.includes('0.00') && !document.querySelector('[data-testid=rec-difference]').innerText.includes('1,000')`, 'zero difference');
    await shot('10b-reconcile');
    await clickText('Finish');
    await waitFor(`[...document.querySelectorAll('tr')].some((tr) => tr.innerText.includes('2026-04-30') && tr.innerText.includes('COMPLETED'))`, 'completed in history');
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

  await check('Seeding again adds nothing twice (D10, D20)', async () => {
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

  await check('Desktop window: menu bar, toolbar and keyboard shortcuts', async () => {
    await go('/bills');
    const formOpen = `!!document.querySelector('input[placeholder="Item description"]')`;
    await press('n', 'KeyN', 78, 2);
    await waitFor(formOpen, 'Ctrl+N opens a new bill');
    await type('input[placeholder="Item description"]', 'Unsaved line');
    await press('Escape', 'Escape', 27);
    await waitFor(`document.body.innerText.includes('Close it anyway?')`, 'Esc asks before dropping typing');
    await clickText('Close without saving');
    await waitFor(`!(${formOpen})`, 'form closed');
    await clickText('Invoice');
    await waitFor(`location.pathname === '/invoices' && document.querySelector('h1')?.textContent === 'New Invoice'`, 'toolbar opens a new invoice');
    await press('Escape', 'Escape', 27);
    await waitFor(`document.querySelector('h1')?.textContent === 'Invoices'`, 'Esc closes an untouched form');
    await press('f', 'KeyF', 70, 1);
    await waitFor(`!!document.querySelector('[role=menu]') && document.querySelector('[role=menu]').innerText.includes('Sign out')`, 'Alt+F opens the File menu');
    await press('Escape', 'Escape', 27);
    await waitFor(`!document.querySelector('[role=menu]')`, 'Esc closes the menu');
    await press('F1', 'F1', 112);
    await waitFor(`!!document.querySelector('[role=dialog]')?.innerText.includes('Keyboard shortcuts')`, 'F1 shows the shortcuts');
    await press('Escape', 'Escape', 27);
    await waitFor(`!document.querySelector('[role=dialog]')`, 'dialog closed');
    await press('b', 'KeyB', 66, 2);
    await waitFor(`!document.querySelector('.desk-nav')`, 'Ctrl+B hides the navigation pane');
    await press('b', 'KeyB', 66, 2);
    await waitFor(`!!document.querySelector('.desk-nav')`, 'Ctrl+B shows it again');
    const blocked = await evaluate(`!document.querySelector('main').dispatchEvent(new MouseEvent('contextmenu', { bubbles: true, cancelable: true }))`);
    if (!blocked) throw new Error('the browser right-click menu is still on');
    await press('F5', 'F5', 116);
    await sleep(500);
    if ((await evaluate('location.pathname')) !== '/invoices') throw new Error('F5 left the screen');
    await shot('15b-desktop-shell');
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
    await waitFor(`document.body.innerText.toLowerCase().includes('net income') && document.body.innerText.toLowerCase().includes('payables (ap)')`, 'KPI cards');
    const text = await pageText();
    // Revenue 2,500.63 (the paid invoice);
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

  await check('Reports drill into an account, a customer gets a statement, Ctrl+E exports it (D23)', async () => {
    await go('/reports/profit-loss');
    await waitFor(`[...document.querySelectorAll('a.report-link')].some((a) => a.textContent.includes('4000 Revenue'))`, 'revenue link');
    await evaluate(`[...document.querySelectorAll('a.report-link')].find((a) => a.textContent.includes('4000 Revenue')).click()`);
    await waitFor(`location.pathname.startsWith('/ledger/account/') && location.search.includes('dateFrom=')`, 'account activity page');
    await waitFor(`document.querySelector('[data-testid=closing-balance]')?.innerText.includes('2,500.63') && document.body.innerText.includes('INV-9000')`, 'revenue entries');
    await shot('18b-account-activity');

    await go('/customers');
    await rowAction('Acme Ltd', 'Statement');
    await waitFor(`location.pathname.endsWith('/statement') && document.body.innerText.includes('STATEMENT OF ACCOUNT')`, 'statement page');
    await waitFor(`['INV-9000', 'PAY-000001'].every((t) => document.body.innerText.includes(t)) && document.querySelector('[data-testid=amount-due]').innerText.includes('0.00')`, 'statement lines, nothing due');
    await shot('18c-statement');

    const dl = join(SHOTS, 'downloads');
    const before = new Set(readdirSync(dl));
    await press('e', 'KeyE', 69, 2);
    const end = Date.now() + 15000;
    let csv;
    while (Date.now() < end && !csv) {
      csv = readdirSync(dl).find((f) => f.endsWith('.csv') && !before.has(f));
      await sleep(250);
    }
    if (!csv) throw new Error('Ctrl+E saved no CSV');
    await sleep(300);
    const text = readFileSync(join(dl, csv), 'utf8');
    if (!csv.startsWith('Customer Statement ') || !text.startsWith('\ufeff')) throw new Error('unexpected file ' + csv);
    if (!text.includes('INV-9000') || !text.includes('2500.63') || text.includes('LKR')) throw new Error('CSV content:\n' + text.slice(0, 400));
  });

  await check('Cash flow ends at the cash balance; P&L compares with last year (D24)', async () => {
    await go('/reports/cash-flow');
    await waitFor(`document.body.innerText.includes('Net cash from operating activities') && document.body.innerText.includes('Net change in cash')`, 'cash flow sections');
    const cf = (await api('GET', '/api/reports/cash-flow?dateFrom=&dateTo=')).body;
    const tb = (await api('GET', '/api/ledger/trial-balance')).body;
    const cash = tb.find((r) => r.account.code === '1000');
    if (Math.round(cf.closingCash * 100) !== Math.round((cash.debit - cash.credit) * 100))
      throw new Error(`cash flow closes at ${cf.closingCash}, the ledger has ${cash.debit - cash.credit}`);
    await shot('19b-cash-flow');

    await go('/reports/profit-loss');
    await waitFor(`!!document.querySelector('select[aria-label="Compare with"]')`, 'compare picker');
    await evaluate(`(() => { const sel = document.querySelector('select[aria-label="Compare with"]');
      sel.value = 'lastYear'; sel.dispatchEvent(new Event('change', { bubbles: true })); return true; })()`);
    await clickText('Run');
    const lastYear = new Date().getFullYear() - 1;
    await waitFor(`[...document.querySelectorAll('th')].some((th) => th.textContent === 'Change') && [...document.querySelectorAll('th')].some((th) => th.textContent.startsWith('${lastYear}-01-01'))`, 'comparison columns');
    await shot('18d-profit-loss-compare');
  });

  // ── W4: employees ──────────────────────────────────────────────────────────
  /** Puts a local file into a file input the way a user picking it would (fires change). */
  const pickFile = async (selector, path) => {
    const { result } = await send('Runtime.evaluate', { expression: `document.querySelector(${JSON.stringify(selector)})` });
    if (!result.objectId) throw new Error('no file input ' + selector);
    await send('DOM.setFileInputFiles', { objectId: result.objectId, files: [path] });
  };
  /** Types into the input inside the <label> whose caption is `caption`. */
  const typeLabeled = async (caption, text) => {
    await evaluate(`(() => {
      const l = [...document.querySelectorAll('label')].find((x) => x.querySelector('span')?.textContent.trim() === ${JSON.stringify(caption)});
      if (!l) throw new Error('no field ' + ${JSON.stringify(caption)});
      const el = l.querySelector('input,textarea'); el.focus(); el.select?.(); return true; })()`);
    await send('Input.insertText', { text });
  };
  const PHOTO = join(SHOTS, 'photo.png');
  writeFileSync(PHOTO, Buffer.from('89504e470d0a1a0a0000000d49484452000000010000000108060000001f15c4890000000d49444154789c6360000002000154a24f5d0000000049454e44ae426082', 'hex'));
  const DOC = join(SHOTS, 'Degree certificate.pdf');
  writeFileSync(DOC, '%PDF-1.4\n% smoke test document\n%%EOF\n');
  let empId = '';

  await check('Employees page creates an employee and opens the profile', async () => {
    await go('/employees');
    await clickText('+ New Employee');
    await type('input[placeholder="e.g. CEN-042"]', 'CEN-042');
    await type('input[placeholder="Full name"]', 'Asha Perera');
    await type('input[placeholder="Department"]', 'Operations');
    await clickText('Save & open profile');
    await waitFor(`/^\\/employees\\/[0-9a-f-]{36}$/.test(location.pathname) && document.body.innerText.includes('Asha Perera')`, 'profile page');
    empId = await evaluate('location.pathname.split("/").pop()');
    await go('/employees');
    await type('input[placeholder="Search by ID, name, department, title or email"]', 'operations');
    await waitFor(`document.body.innerText.includes('CEN-042')`, 'search result');
    await shot('20-employees');
  });

  await check('Profile photo uploads, displays and is stored encrypted', async () => {
    await go('/employees/' + empId);
    await waitFor(`document.body.innerText.includes('Add photo')`, 'photo controls');
    await pickFile('input[type=file][accept^="image/"]', PHOTO);
    await waitFor(`document.body.innerText.includes('Change photo')`, 'photo saved');
    await waitFor(`[...document.querySelectorAll('img')].some((i) => i.src.startsWith('blob:') && i.complete && i.naturalWidth > 0)`, 'photo shown');
    const r = await api('GET', '/api/employees/' + empId);
    if (!/^employees\/.+\/photo\/.+\.png$/.test(r.body.photoKey ?? '')) throw new Error('photoKey ' + r.body.photoKey);
  });

  await check('Documents upload through the form and download intact', async () => {
    await clickText('+ Upload document');
    await pickFile('form input[type=file]', DOC);
    await type('input[placeholder="e.g. Office Safety Certificate"]', 'Degree certificate');
    await clickText('Upload');
    await waitFor(`document.body.innerText.includes('Degree certificate.pdf') || document.querySelectorAll('tr').length > 1 && document.body.innerText.includes('Degree certificate')`, 'document row');
    const docs = (await api('GET', `/api/employees/${empId}/documents`)).body;
    if (docs.length !== 1 || docs[0].size !== String(fs_size(DOC))) throw new Error('documents ' + JSON.stringify(docs));
    const got = await evaluate(`(async () => {
      const r = await fetch('/api/employees/${empId}/documents/' + ${JSON.stringify(docs[0].id)} + '/download',
        { headers: { authorization: 'Bearer ' + localStorage.getItem('accessToken') } });
      return { status: r.status, cd: r.headers.get('content-disposition'), text: await r.text() };
    })()`);
    if (got.status !== 200 || !got.text.includes('smoke test document') || !got.cd.startsWith('attachment; filename="Degree certificate.pdf"'))
      throw new Error('download ' + JSON.stringify(got));
    await shot('21-employee-profile');
  });

  await check('Salary section sets a salary and bank details (payroll)', async () => {
    await clickText('+ Set salary');
    await fillDates('2026-04-01');
    await typeLabeled('Basic *', '50000');
    await typeLabeled('BRA 2005', '1000');
    await evaluate(`(() => {
      const f = [...document.querySelectorAll('form')].find((x) => x.innerText.includes('Effective from'));
      f.querySelector('button[type=submit]').click(); return true; })()`);
    await waitFor(`document.body.innerText.includes('+ Add increment')`, 'salary saved');
    await clickText('Bank details');
    await typeLabeled('Bank', 'Bank of Ceylon');
    await typeLabeled('Staff loan balance (Rs.)', '1500.50');
    await evaluate(`(() => {
      const f = [...document.querySelectorAll('form')].find((x) => x.innerText.includes('Account number'));
      f.querySelector('button[type=submit]').click(); return true; })()`);
    await waitFor(`document.body.innerText.includes('Bank of Ceylon')`, 'bank saved');
    const v = (await api('GET', '/api/payroll/employees/' + empId)).body;
    if (v.current?.basic !== 50000 || v.current?.monthlyTotal !== 51000 || v.staffLoanBalance !== 1500.5)
      throw new Error('payroll view ' + JSON.stringify(v));
    await shot('22-salary');
  });

  // ── W5: payroll runs ────────────────────────────────────────────────────────
  /** Sets the n-th control matching `selector` inside the form containing `formText`, the way React notices. */
  const setInForm = (formText, selector, value, n = 0) => evaluate(`(() => {
    const f = [...document.querySelectorAll('form')].find((x) => x.innerText.includes(${JSON.stringify(formText)}));
    if (!f) throw new Error('no form with ' + ${JSON.stringify(formText)});
    const el = f.querySelectorAll(${JSON.stringify(selector)})[${n}];
    if (!el) throw new Error('no control ' + ${JSON.stringify(selector)});
    const proto = el instanceof HTMLSelectElement ? HTMLSelectElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(proto, 'value').set.call(el, ${JSON.stringify(value)});
    el.dispatchEvent(new Event(el instanceof HTMLSelectElement ? 'change' : 'input', { bubbles: true }));
    return true; })()`);
  const submitFormWith = (text) => evaluate(`(() => {
    const f = [...document.querySelectorAll('form')].find((x) => x.innerText.includes(${JSON.stringify(text)}));
    if (!f) throw new Error('no form with ' + ${JSON.stringify(text)});
    f.requestSubmit(); return true; })()`);
  const fetchText = (path) => evaluate(`(async () => {
    const r = await fetch(${JSON.stringify(path)}, { headers: { authorization: 'Bearer ' + localStorage.getItem('accessToken') } });
    return (await r.text()).slice(0, 5); })()`);
  const trialBalanced = async () => {
    const tb = (await api('GET', '/api/ledger/trial-balance')).body;
    const debit = tb.reduce((a, r) => a + Math.round(r.debit * 100), 0);
    const credit = tb.reduce((a, r) => a + Math.round(r.credit * 100), 0);
    if (debit !== credit) throw new Error(`trial balance off: ${debit} vs ${credit}`);
    return tb;
  };
  let runId = '';

  await check('Payroll page starts a month and the run lists the employee', async () => {
    await go('/payroll');
    await setInForm('Month', 'input[type=month]', '2026-04');
    await waitFor(`document.body.innerText.includes('Start payroll for')`, 'start button');
    await submitFormWith('Start payroll for');
    await waitFor(`/^\\/payroll\\/[0-9a-f-]{36}$/.test(location.pathname) && document.body.innerText.includes('Asha Perera')`, 'run page');
    runId = await evaluate('location.pathname.split("/").pop()');
    const run = (await api('GET', '/api/payroll/runs/' + runId)).body;
    if (run.status !== 'DRAFT' || run.lines.length !== 1 || run.lines[0].grossSalary !== '51000.00') throw new Error(JSON.stringify(run).slice(0, 300));
    await shot('23-payroll-draft');
  });

  await check('Run posts to the ledger, pays salaries and remits EPF', async () => {
    await quietDialogs();
    await clickText('Post to ledger');
    await waitFor(`document.body.innerText.includes('Pay salaries')`, 'posted');
    const cash = (await api('GET', '/api/accounts')).body.find((a) => a.code === '1000').id;
    await setInForm('Record payment of', 'select', cash);
    await submitFormWith('Record payment of');
    await waitFor(`!document.body.innerText.includes('Record payment of')`, 'payment recorded');
    await setInForm('Record remittance', 'select', cash, 1);
    await submitFormWith('Record remittance');
    await waitFor(`document.body.innerText.toLowerCase().includes('remittances recorded')`, 'remittance listed');
    const run = (await api('GET', '/api/payroll/runs/' + runId)).body;
    if (run.status !== 'PAID' || run.owed.EPF.outstanding !== 0 || run.remittances.length !== 1) throw new Error(run.status + ' ' + JSON.stringify(run.owed));
    await trialBalanced();
    await shot('24-payroll-paid');
  });

  await check('Salary slips are generated and filed into the employee record', async () => {
    await quietDialogs();
    await clickText('File slips to employee records');
    await waitFor(`document.body.innerText.includes('Re-file slips to employee records')`, 'slips filed', 30000);
    const docs = (await api('GET', `/api/employees/${empId}/documents`)).body;
    const slip = docs.find((d) => d.category === 'PAYSLIP');
    if (!slip || slip.mimeType !== 'application/pdf' || Number(slip.size) < 500) throw new Error('slip ' + JSON.stringify(slip));
    const head = await fetchText(`/api/employees/${empId}/documents/${slip.id}/download`);
    if (head !== '%PDF-') throw new Error('slip is not a PDF: ' + head);
  });

  await check('Voiding the run reverses every entry and restores the loan', async () => {
    await quietDialogs();
    await clickText('Void');
    await waitFor(`document.body.innerText.includes('VOID')`, 'void badge');
    const v = (await api('GET', '/api/payroll/employees/' + empId)).body;
    if (v.staffLoanBalance !== 1500.5) throw new Error('loan ' + v.staffLoanBalance);
    const tb = await trialBalanced();
    const salaries = tb.find((r) => r.account.code === '6100');
    if (salaries && salaries.balance !== 0) throw new Error('salaries expense ' + salaries.balance);
    await shot('25-payroll-void');
  });

  // ── W6: backup and restore page ─────────────────────────────────────────────
  await check('Backup page sets a password, writes a verified backup and checks it', async () => {
    await go('/system/backup');
    await waitFor(`!document.getElementById('password-card').hidden`, 'backup page loaded');
    await type('#pw-new', 'office backup 2026');
    await type('#pw-again', 'office backup 2026');
    await evaluate(`document.getElementById('pw-save').click()`);
    await waitFor(`document.getElementById('pw-msg').textContent.includes('Backup password saved')`, 'password saved');
    const folder = join(SHOTS, 'backups');
    await evaluate(`document.getElementById('bk-folder').value = ''`);
    await type('#bk-folder', folder);
    await evaluate(`document.getElementById('bk-run').click()`);
    await waitFor(`document.getElementById('bk-msg').textContent.includes('Backup written and verified')`, 'backup written', 60000);
    const made = readdirSync(folder).filter((f) => f.endsWith('.qbak'));
    if (made.length !== 1) throw new Error('backup files: ' + made.join(', '));
    await type('#rs-file', join(folder, made[0]));
    await evaluate(`document.getElementById('rs-verify').click()`);
    await waitFor(`document.getElementById('rs-msg').textContent.includes('complete and readable')`, 'backup checked', 60000);
    await shot('26-backup');
  });

  await check('Payments page voids a payment and the invoice is owed again (D18)', async () => {
    await go('/payments');
    await rowAction('PAY-000001', 'Void');
    await waitFor(`!!document.querySelector('[role=dialog]')?.innerText.includes('PAY-000001')`, 'confirm dialog');
    await clickText('Void payment');
    await waitFor(`[...document.querySelectorAll('tr')].some((tr) => tr.innerText.includes('PAY-000001') && tr.innerText.includes('VOID'))`, 'payment marked VOID');
    const inv = (await api('GET', '/api/invoices')).body.data.find((i) => i.invoiceNumber === 'INV-9000');
    if (inv.status !== 'POSTED' || inv.amountPaid !== '0.00') throw new Error(`invoice is ${inv.status} with ${inv.amountPaid} paid`);
    await shot('27-payment-void');
  });

  await check('Credit and debit notes reduce what is owed, and can be voided (D21)', async () => {
    const setAmount = async (value) => {
      await evaluate(`(() => { const el = document.querySelector('input[placeholder=Amount]'); el.focus(); el.select(); return true; })()`);
      await send('Input.insertText', { text: value });
    };
    await go('/credit-notes');
    await clickText('+ New Credit Note');
    await waitFor(`[...(document.querySelectorAll('form select')[0]?.options ?? [])].some((o) => o.textContent.includes('INV-9000'))`, 'invoice option');
    await pick(0, 'INV-9000');
    await waitFor(`document.querySelector('input[placeholder=Amount]').value === '2500.63'`, 'amount defaults to what is owed');
    await setAmount('500');
    await type('input[placeholder^="e.g. 10%"]', 'Discount for late delivery');
    await clickText('Save Note');
    await waitFor(`[...document.querySelectorAll('tr')].some((tr) => tr.innerText.includes('CN-000001') && tr.innerText.includes('INV-9000'))`, 'credit note row');
    const inv = async () => (await api('GET', '/api/invoices')).body.data.find((i) => i.invoiceNumber === 'INV-9000');
    if ((await inv()).amountCredited !== '500.00') throw new Error('invoice not credited');
    const ar = (await api('GET', '/api/reports/ar-aging')).body.grandTotal;
    if (ar !== 2000.63) throw new Error('AR ageing shows ' + ar);
    await shot('29-credit-notes');
    await rowAction('CN-000001', 'Void');
    await waitFor(`!!document.querySelector('[role=dialog]')`, 'void confirmation');
    await clickText('Void note');
    await waitFor(`[...document.querySelectorAll('tr')].some((tr) => tr.innerText.includes('CN-000001') && tr.innerText.includes('VOID'))`, 'credit note voided');
    if ((await inv()).amountCredited !== '0.00') throw new Error('void did not restore the invoice');

    await go('/debit-notes');
    await clickText('+ New Debit Note');
    await waitFor(`[...(document.querySelectorAll('form select')[0]?.options ?? [])].some((o) => o.textContent.includes('BILL-000001'))`, 'bill option');
    await pick(0, 'BILL-000001');
    await setAmount('10');
    await type('input[placeholder^="e.g. 10%"]', 'Two reams returned');
    await clickText('Save Note');
    await waitFor(`[...document.querySelectorAll('tr')].some((tr) => tr.innerText.includes('DN-000001') && tr.innerText.includes('Paper Co'))`, 'debit note row');
    const ap = (await api('GET', '/api/reports/ap-aging')).body.grandTotal;
    if (ap !== 27.5) throw new Error('AP ageing shows ' + ap);
  });

  await check('Settings page locks the books and postings on the lock date are refused (D19)', async () => {
    await go('/settings');
    await waitFor(`document.body.innerText.includes('not locked')`, 'lock card');
    await fillDates('2026-04-30');
    await clickText('Lock');
    await waitFor(`document.querySelector('[data-testid=lock-status]')?.innerText.includes('30 April 2026')`, 'locked');
    const cash = (await api('GET', '/api/accounts')).body.find((a) => a.code === '1000').id;
    const inv = (await api('GET', '/api/invoices')).body.data.find((i) => i.invoiceNumber === 'INV-9000');
    const late = await api('POST', '/api/payments', { type: 'AR', invoiceId: inv.id, date: '2026-04-30', amount: 1, paymentMethod: 'CASH', paymentAccountId: cash });
    if (late.status !== 400 || !String(late.body.message).startsWith('The books are locked up to and including 2026-04-30'))
      throw new Error(`payment on the lock date answered ${late.status} ${JSON.stringify(late.body)}`);
    await shot('28-books-locked');
    await clickText('Unlock');
    await waitFor(`!!document.querySelector('[role=dialog]')`, 'unlock confirmation');
    await evaluate(`document.querySelector('[role=dialog] .btn-primary').click()`);
    await waitFor(`document.querySelector('[data-testid=lock-status]')?.innerText.includes('not locked')`, 'unlocked');
  });

  await check('An administrator turns a user off', async () => {
    const users = (await api('GET', '/api/users')).body;
    const clerk = (users.data ?? users).find((u) => u.username === 'clerk.one');
    const r = await api('PATCH', `/api/users/${clerk.id}`, { isActive: false });
    if (r.status !== 200) throw new Error('deactivating answered ' + r.status);
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

  await check('Sign-in explains a turned-off account and too many attempts (item 11)', async () => {
    const signIn = async (user, password) => {
      await go('/login');
      await type('input[type=text]', user);
      await type('input[type=password]', password);
      await evaluate(`document.querySelector('button[type=submit]').click()`);
    };
    await signIn('clerk.one', 'clerkpass1');
    await waitFor(`document.body.innerText.includes('This account has been turned off')`, 'turned-off message');
    // Five failures lock the name for a while; the next try says so instead of "invalid".
    await evaluate(`(async () => {
      for (let i = 0; i < 5; i++) await fetch('/api/auth/login', { method: 'POST', headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ username: 'nobody', password: 'wrong-password' }) });
      return true; })()`);
    await signIn('nobody', 'wrong-password');
    await waitFor(`document.body.innerText.includes('Too many failed attempts. Try again in')`, 'throttle message');
    await shot('08b-login-throttled');
  });
} catch (e) {
  failures.push('harness');
  results.push(`FAIL  harness: ${e.message}`);
  console.log(results.at(-1));
} finally {
  console.log(`--- ${results.length - failures.length} passed, ${failures.length} failed`);
  console.log(`\nAPI responses >= 400 seen by the browser:\n  ${[...new Set(httpErrors)].join('\n  ') || '(none)'}`);
  console.log(`Console errors:\n  ${[...new Set(consoleErrors)].slice(0, 15).join('\n  ') || '(none)'}`);
  console.log(`Screenshots: ${SHOTS}`);
  try { ws?.close(); } catch { /* ignore */ }
  edge.kill();
  process.exit(failures.length ? 1 : 0);
}
