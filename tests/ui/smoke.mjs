// UI smoke test: drives the real React frontend in headless Microsoft Edge (Chrome DevTools
// Protocol, no npm dependencies) against a running accounting-server.
//
//   node tests/ui/smoke.mjs http://127.0.0.1:47800 <screenshot-dir>
//
// The server must have a fresh, empty database (no ADMIN_USERNAME / ADMIN_PASSWORD): the test
// creates admin / admin12345 through the first-run screen (D17).
//
// D39: the Shopify step talks to a stand-in Shopify that this script serves on port 47950; start
// the server with ACCOUNTING_SHOPIFY_BASE_URL=http://127.0.0.1:47950 (otherwise that step fails).

import { spawn } from 'node:child_process';
import { createServer } from 'node:http';
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

// D39: a stand-in Shopify Admin API (one order, its payment, fee and payout).
const fakeShopify = createServer((req, res) => {
  if (req.headers['x-shopify-access-token'] !== 'shpat_smoke_1234567890') { res.writeHead(401); res.end('{}'); return; }
  const body = req.url.includes('/shop.json') ? { shop: { name: 'Smoke Tea Shop', currency: 'LKR' } }
    : req.url.includes('/orders.json') ? { orders: [{
        id: 7001, name: '#2001', processed_at: '2026-10-20T10:00:00+05:30', currency: 'LKR', financial_status: 'paid', cancelled_at: null,
        taxes_included: false, total_price: '1500.00', customer: { first_name: 'Nimali', last_name: 'Fernando', email: 'nimali@example.lk' },
        line_items: [{ title: 'Tea gift box', sku: 'GIFT-1', quantity: 1, price: '1500.00', discount_allocations: [], tax_lines: [] }],
        shipping_lines: [] }] }
    : req.url.includes('/balance/transactions.json') ? { transactions: [
        { id: 8001, type: 'charge', test: false, amount: '1500.00', fee: '45.00', source_order_id: 7001, processed_at: '2026-10-20T10:01:00+05:30' },
        { id: 8002, type: 'payout', test: false, amount: '-1455.00', fee: '0.00', source_order_id: null, processed_at: '2026-10-21T08:00:00+05:30' }] }
    : null;
  res.writeHead(body ? 200 : 404, { 'content-type': 'application/json' });
  res.end(JSON.stringify(body ?? {}));
}).listen(47950, '127.0.0.1');

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
    // D25: no company ships with the app; the new administrator enters theirs first.
    await waitFor(`location.pathname === '/company-setup' && !!document.querySelector('form[aria-label="Company setup"]')`, 'company setup');
    await shot('00b-company-setup');
    // D31: one question per page, with Next, Skip and Back.
    const page = (title) => waitFor(`document.querySelector('form[aria-label="Company setup"] h1')?.innerText.includes(${JSON.stringify(title)})`, title);
    await page('What is your company called?');
    await clickText('Next');
    await waitFor(`document.body.innerText.includes('Enter the company name')`, 'name is asked for');
    await type('input[aria-label="Company name"]', 'Acme Trading (Pvt) Ltd');
    await clickText('Next');
    await page('address');
    await clickText('Skip');
    await page('email');
    await type('input[aria-label="Email"]', 'not-an-email');
    await clickText('Next');
    await waitFor(`document.body.innerText.includes('does not look like an email')`, 'email checked');
    await evaluate(`(() => { const el = document.querySelector('input[aria-label="Email"]'); el.select(); return true; })()`);
    await send('Input.insertText', { text: 'accounts@acme.test' });
    await clickText('Next');
    await page('phone number');
    await clickText('Back');
    await page('email');
    await clickText('Next');
    await page('phone number');
    await type('input[aria-label="Phone"]', '+94 11 234 5678');
    await clickText('Next');
    await page('website');
    await clickText('Skip');
    await page('Which country');
    const countries = await evaluate(`document.querySelector('select[aria-label="Country"]').options.length`);
    if (countries < 240) throw new Error('only ' + countries + ' countries listed');
    const choose = (label, value) => evaluate(`(() => { const sel = document.querySelector('select[aria-label="${label}"]');
      sel.value = ${JSON.stringify(value)}; sel.dispatchEvent(new Event('change', { bubbles: true })); return true; })()`);
    await choose('Country', 'LK');
    await waitFor(`document.querySelector('[data-testid=money-example]').innerText.includes('LKR')`, 'Sri Lanka preset');
    await choose('Financial year starts in', '1');
    await shot('00c-company-country');
    await clickText('Next');
    for (const t of ['registered for tax', 'logo', 'How do customers pay', 'bottom of your invoices']) {
      await page(t);
      await clickText('Skip');
    }
    await page('check your details');
    await waitFor(`document.body.innerText.includes('accounts@acme.test') && document.body.innerText.includes('Sri Lanka')`, 'review lists the answers');
    await shot('00d-company-review');
    await clickText('Finish');
    await waitFor(`location.pathname === '/' && !!localStorage.getItem('accessToken')`, 'dashboard after setup');
    const settings = await evaluate(`fetch('/api/settings', { headers: { authorization: 'Bearer ' + localStorage.getItem('accessToken') } }).then((r) => r.json())`);
    if (settings['company.name'] !== 'Acme Trading (Pvt) Ltd' || settings['region.currency'] !== 'LKR')
      throw new Error('company not saved: ' + JSON.stringify(settings).slice(0, 300));
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
    await type('input[placeholder="Confirm password"]', 'clerkpass1');
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

  /** D36: Settings is tabbed. */
  const settingsTab = async (label) => {
    await go('/settings');
    await waitFor(`[...document.querySelectorAll('[role=tab]')].some((b) => b.textContent.trim() === ${JSON.stringify(label)})`, 'settings tabs');
    await evaluate(`[...document.querySelectorAll('[role=tab]')].find((b) => b.textContent.trim() === ${JSON.stringify(label)}).click()`);
    await sleep(500);
  };

  await check('Settings page saves the signatory', async () => {
    await settingsTab('Invoices & documents');
    await waitFor(`!!document.querySelector('input[placeholder="e.g. Jane Smith"]')`, 'settings form');
    await type('input[placeholder="e.g. Jane Smith"]', 'Jane Doe');
    // D25: no title ships with the app either.
    await type('input[placeholder="e.g. Director"]', 'Director');
    await evaluate(`document.querySelector('input[placeholder="e.g. Jane Smith"]').closest('form').querySelector('button[type=submit]').click()`);
    await sleep(800);
    const saved = await evaluate(`fetch('/api/settings', { headers: { authorization: 'Bearer ' + localStorage.getItem('accessToken') } }).then((r) => r.json())`);
    if (saved.signatoryName !== 'Jane Doe') throw new Error(`signatoryName is ${saved.signatoryName}`);
    await shot('06-settings');
  });

  await check('Activity log shows the audited actions', async () => {
    await go('/audit');
    await waitFor(`['Created a user', 'Updated company settings', 'Logged in'].every((a) => document.body.innerText.includes(a))`, 'audit rows');
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
    await waitFor(`[...document.querySelectorAll('tr')].some((tr) => tr.innerText.includes(new Intl.DateTimeFormat('en-LK', { day: 'numeric', month: 'short', year: 'numeric' }).format(new Date(2026, 3, 30))) && tr.innerText.includes('COMPLETED'))`, 'completed in history');
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
  const pickAria = (label, text) => evaluate(`(() => {
    const sel = document.querySelector(${JSON.stringify(`select[aria-label="${label}"]`)});
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
    await pickAria('Account for line 1', '6000');
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
    await waitFor(`[...document.querySelectorAll('th')].some((th) => th.textContent === 'Change') && [...document.querySelectorAll('th')].some((th) => th.textContent.startsWith(new Intl.DateTimeFormat('en-LK', { day: 'numeric', month: 'short', year: 'numeric' }).format(new Date(${lastYear}, 0, 1))))`, 'comparison columns');
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
    await settingsTab('Backup & restore');
    await waitFor(`!!document.getElementById('pw-new')`, 'backup page loaded');
    await type('#pw-new', 'office backup 2026');
    await type('#pw-again', 'office backup 2026');
    await evaluate(`document.getElementById('pw-save').click()`);
    await waitFor(`document.getElementById('pw-msg').textContent.includes('Backup password saved')`, 'password saved');
    const folder = join(SHOTS, 'backups');
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

  await check('Taxes: a tax code, a tax invoice from the form, and the tax return (D26)', async () => {
    await go('/taxes');
    await clickText('+ New Tax Code');
    await waitFor(`!!document.querySelector('form[aria-label="Tax code"]')`, 'tax code form');
    await type('input[placeholder="e.g. VAT 18%"]', 'VAT 18%');
    await type('input[placeholder="e.g. 18"]', '18');
    await clickText('Add Tax Payable and Tax Receivable');
    await waitFor(`!!document.querySelector('select[aria-label="Sales tax account"]').value && !!document.querySelector('select[aria-label="Purchase tax account"]').value`, 'tax accounts chosen');
    await clickText('Save Tax Code');
    await waitFor(`[...document.querySelectorAll('tr')].some((tr) => tr.innerText.includes('VAT 18%') && tr.innerText.includes('18%'))`, 'tax code row');
    await evaluate(`(() => { const sel = document.querySelector('select[aria-label="Default sales tax"]');
      sel.value = [...sel.options].find((o) => o.textContent === 'VAT 18%').value;
      sel.dispatchEvent(new Event('change', { bubbles: true })); return true; })()`);
    await sleep(600);
    await shot('30-taxes');

    // A new invoice line starts with the default tax; the sheet shows subtotal, VAT and total.
    await go('/invoices');
    await clickText('+ New Invoice');
    await waitFor(`!!document.querySelector('select[aria-label="Tax for line 1"]')?.value`, 'line starts with VAT');
    await type('input[placeholder="Name as it should print"]', 'Walk-in Client');
    await type('input[placeholder="Item description"]', 'Consulting');
    await evaluate(`(() => { const el = document.querySelectorAll('.inv-sheet input[type=number]')[1]; el.focus(); el.select(); return true; })()`);
    await send('Input.insertText', { text: '1000' });
    await waitFor(`document.querySelector('.inv-sheet h1').textContent === 'Tax Invoice' && document.querySelector('[data-testid=invoice-total]')?.innerText.includes('1,180.00')`, 'tax invoice totals');
    await shot('31-tax-invoice');
    await clickText('Save Invoice');
    await waitFor(`document.body.innerText.includes('Walk-in Client')`, 'taxed invoice listed');
    const taxed = (await api('GET', '/api/invoices')).body.data.find((i) => i.billToName === 'Walk-in Client');
    if (taxed.totalAmount !== '1180.00' || taxed.taxAmount !== '180.00') throw new Error(`invoice ${taxed.totalAmount} / tax ${taxed.taxAmount}`);
    const posted = await api('POST', `/api/invoices/${taxed.id}/post`);
    if (posted.status !== 201) throw new Error('posting answered ' + posted.status);

    await go('/reports/tax');
    await evaluate(`(() => { const set = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set;
      for (const [label, v] of [['From', '2026-01-01'], ['To', '${new Date().toISOString().slice(0, 10)}']]) {
        const el = document.querySelector('input[aria-label="' + label + '"]'); set.call(el, v); el.dispatchEvent(new Event('input', { bubbles: true })); }
      return true; })()`);
    await clickText('Run');
    await waitFor(`[...document.querySelectorAll('tr')].some((tr) => tr.innerText.includes('VAT 18%') && tr.innerText.includes('180.00'))`, 'VAT row in the return');
    await waitFor(`document.querySelector('[data-testid=net-tax]').innerText.includes('180.00')`, 'tax to pay');
    await shot('32-tax-return');
  });

  await check('Tax groups and withholding tax through the screens (D27)', async () => {
    // Two standard codes, then a group of them built on the Taxes screen.
    const accounts = (await api('GET', '/api/accounts')).body;
    const acc = (code) => accounts.find((a) => a.code === code).id;
    for (const [name, rate] of [['State 6%', 6], ['City 2%', 2]]) {
      const r = await api('POST', '/api/tax-codes', { name, rate, salesAccountId: acc('2300'), purchaseAccountId: acc('1400') });
      if (r.status !== 201) throw new Error(name + ' answered ' + r.status);
    }
    await go('/taxes');
    await clickText('+ New Tax Code');
    await type('input[placeholder="e.g. VAT 18%"]', 'State + City');
    await evaluate(`(() => { const sel = document.querySelector('select[aria-label="Kind"]');
      sel.value = 'GROUP'; sel.dispatchEvent(new Event('change', { bubbles: true })); return true; })()`);
    await waitFor(`!!document.querySelector('input[aria-label="Include State 6%"]')`, 'group parts');
    await evaluate(`document.querySelector('input[aria-label="Include State 6%"]').click()`);
    await evaluate(`document.querySelector('input[aria-label="Include City 2%"]').click()`);
    await clickText('Save Tax Code');
    await waitFor(`[...document.querySelectorAll('tr')].some((tr) => tr.innerText.includes('State + City') && tr.innerText.includes('8%'))`, 'group row');

    // A withholding code, then a customer payment with 5% held back from the 1,180 tax invoice.
    await api('POST', '/api/accounts', { code: '1450', name: 'Withholding Tax Receivable', type: 'ASSET' });
    await api('POST', '/api/accounts', { code: '2350', name: 'Withholding Tax Payable', type: 'LIABILITY' });
    const fresh = (await api('GET', '/api/accounts')).body;
    const wht = await api('POST', '/api/tax-codes', { name: 'WHT 5%', kind: 'WITHHOLDING', rate: 5,
      salesAccountId: fresh.find((a) => a.code === '1450').id, purchaseAccountId: fresh.find((a) => a.code === '2350').id });
    if (wht.status !== 201) throw new Error('WHT answered ' + wht.status);
    await go('/payments');
    await clickText('+ Record Payment');
    await waitFor(`[...(document.querySelectorAll('form select')[1]?.options ?? [])].some((o) => o.textContent.includes('Walk-in Client'))`, 'taxed invoice option');
    await pick(1, 'Walk-in Client');
    await pick(3, '1000');
    await fillDates('2026-10-01');
    await evaluate(`(() => { const sel = document.querySelector('select[aria-label="Withholding tax"]');
      sel.value = [...sel.options].find((o) => o.textContent === 'WHT 5%').value;
      sel.dispatchEvent(new Event('change', { bubbles: true })); return true; })()`);
    await waitFor(`document.querySelector('input[placeholder="Amount withheld"]')?.value === '50.00' && document.querySelector('input[placeholder="Amount"]').value === '1130.00'`, '5% of the 1,000 before VAT suggested');
    await shot('33-payment-withholding');
    await clickText('Record Payment');
    await waitFor(`[...document.querySelectorAll('tr')].some((tr) => tr.innerText.includes('1,130.00') && tr.innerText.includes('50.00'))`, 'payment with withholding listed');
    const inv = (await api('GET', '/api/invoices')).body.data.find((i) => i.billToName === 'Walk-in Client');
    if (inv.status !== 'PAID') throw new Error('the withholding should complete the payment, invoice is ' + inv.status);
    const ret = (await api('GET', '/api/reports/tax?dateFrom=2026-01-01&dateTo=2026-12-31')).body;
    if (ret.withholdingTotals.withheldByCustomers !== 50) throw new Error('return shows ' + JSON.stringify(ret.withholdingTotals));
  });

  await check('A USD invoice from the form, paid at a better rate: an exchange gain (D30)', async () => {
    await go('/invoices');
    await clickText('+ New Invoice');
    await waitFor(`!!document.querySelector('select[aria-label="Currency"]')`, 'currency picker');
    await evaluate(`(() => { const sel = document.querySelector('select[aria-label="Currency"]');
      sel.value = 'USD'; sel.dispatchEvent(new Event('change', { bubbles: true })); return true; })()`);
    await waitFor(`!!document.querySelector('input[aria-label="Exchange rate"]')`, 'rate field');
    await type('input[aria-label="Exchange rate"]', '300');
    // No tax on this one: switch the line's default VAT off.
    await evaluate(`(() => { const sel = document.querySelector('select[aria-label="Tax for line 1"]');
      if (sel) { sel.value = ''; sel.dispatchEvent(new Event('change', { bubbles: true })); } return true; })()`);
    await type('input[placeholder="Name as it should print"]', 'US Buyer');
    await type('input[placeholder="Item description"]', 'Export order');
    await evaluate(`(() => { const el = document.querySelectorAll('.inv-sheet input[type=number]')[1]; el.focus(); el.select(); return true; })()`);
    await send('Input.insertText', { text: '100' });
    await waitFor(`[...document.querySelectorAll('.inv-sheet th')].some((th) => th.textContent.includes('(USD)'))`, 'USD columns');
    await shot('34-usd-invoice');
    await clickText('Save Invoice');
    await waitFor(`document.body.innerText.includes('US Buyer')`, 'USD invoice listed');
    const inv = (await api('GET', '/api/invoices')).body.data.find((i) => i.billToName === 'US Buyer');
    if (inv.currency !== 'USD' || inv.baseTotal !== '30000.00') throw new Error(`invoice ${inv.currency} ${inv.baseTotal}`);
    await api('POST', `/api/invoices/${inv.id}/post`);

    await go('/payments');
    await clickText('+ Record Payment');
    await waitFor(`[...(document.querySelectorAll('form select')[1]?.options ?? [])].some((o) => o.textContent.includes('US Buyer'))`, 'USD invoice option');
    await pick(1, 'US Buyer');
    await waitFor(`document.querySelector('input[aria-label="Payment exchange rate"]')?.value === '300'`, 'rate starts at the invoice rate');
    await type('input[placeholder="Amount"]', '100');
    await type('input[aria-label="Payment exchange rate"]', '310');
    await pick(3, '1000');
    await fillDates('2026-10-02');
    await clickText('Record Payment');
    await waitFor(`[...document.querySelectorAll('tr')].some((tr) => tr.innerText.includes('31,000.00'))`, 'payment in the books at 310');
    const paid = (await api('GET', '/api/invoices')).body.data.find((i) => i.billToName === 'US Buyer');
    if (paid.status !== 'PAID') throw new Error('USD invoice is ' + paid.status);
    const tb = (await api('GET', '/api/ledger/trial-balance')).body;
    const fx = tb.find((r) => r.account.code === '7900');
    if (!fx || fx.balance !== -1000) throw new Error('exchange account ' + JSON.stringify(fx && fx.balance));
    await shot('35-fx-payment');
  });

  await check('Payable accounts, account edits, transaction details and a USD bank account (D33-D35)', async () => {
    const setv = (sel, v, n = 0) => evaluate(`(() => { const el = document.querySelectorAll(${JSON.stringify(sel)})[${n}];
      const p = el instanceof HTMLSelectElement ? HTMLSelectElement.prototype : HTMLInputElement.prototype;
      Object.getOwnPropertyDescriptor(p, 'value').set.call(el, ${JSON.stringify(String(v))});
      el.dispatchEvent(new Event(el instanceof HTMLSelectElement ? 'change' : 'input', { bubbles: true })); return true; })()`);
    const pickIn = (sel, text) => evaluate(`(() => { const el = document.querySelector(${JSON.stringify(sel)});
      const o = [...el.options].find((x) => x.textContent.includes(${JSON.stringify(text)})); if (!o) throw new Error('no option ' + ${JSON.stringify(text)});
      Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, 'value').set.call(el, o.value);
      el.dispatchEvent(new Event('change', { bubbles: true })); return true; })()`);
    // Chart of accounts: a payable, a USD bank account, and an edit.
    await go('/accounts');
    for (const [code, name, type, currency] of [['2400', 'Telephone Payable', 'LIABILITY', ''], ['1020', 'USD Bank', 'ASSET', 'USD']]) {
      await clickText('+ New Account');
      await waitFor(`!!document.querySelector('form[aria-label="Account"]')`, 'account form');
      await setv('input[placeholder="Code (e.g. 1000)"]', code);
      await setv('input[placeholder="Account Name"]', name);
      await setv('select[aria-label="Account type"]', type);
      if (currency) await setv('select[aria-label="Account currency"]', currency);
      await clickText('Save Account');
      await waitFor(`document.body.innerText.includes(${JSON.stringify(name)})`, name);
    }
    await rowAction('Telephone Payable', 'Edit');
    await waitFor(`document.body.innerText.includes('Edit 2400')`, 'edit form');
    await setv('input[placeholder="Account Name"]', 'Telephone & Internet Payable');
    await clickText('Save Changes');
    await waitFor(`document.body.innerText.includes('Telephone & Internet Payable')`, 'renamed');
    await shot('36-accounts-edit');

    // The senior's example as a journal entry, then opened in full.
    await go('/ledger');
    await clickText('+ New Journal Entry');
    await setv('input[aria-label="Entry date"]', '2026-10-05');
    await setv('input[placeholder^="Description"]', 'Telephone bill September');
    await pickIn('select[aria-label="Account for line 1"]', '6000');
    await setv('input[aria-label="Debit for line 1"]', '18337.13');
    await pickIn('select[aria-label="Account for line 2"]', '2400');
    await setv('input[aria-label="Credit for line 2"]', '18337.13');
    await waitFor(`document.querySelector('[data-testid=je-difference]').innerText.includes('Balanced')`, 'balanced');
    await clickText('Post Entry');
    await waitFor(`document.body.innerText.includes('Telephone bill September')`, 'entry listed');
    await evaluate(`[...document.querySelectorAll('tr')].find((tr) => tr.innerText.includes('Telephone bill September')).click()`);
    await waitFor(`location.pathname.startsWith('/ledger/entries/') && document.body.innerText.includes('Ledger lines') && document.body.innerText.includes('Telephone & Internet Payable')`, 'transaction detail');
    await shot('37-transaction-detail');

    // A bill owed on the telephone payable.
    await go('/bills');
    await clickText('+ New Bill');
    await waitFor(`[...(document.querySelectorAll('form select')[0]?.options ?? [])].some((o) => o.textContent.includes('Paper Co'))`, 'vendor option');
    await pick(0, 'Paper Co');
    await type('input[placeholder="Item description"]', 'Internet October');
    await evaluate(`(() => { const nums = document.querySelectorAll('form input[type=number]'); nums[0].focus(); nums[0].select(); return true; })()`);
    await send('Input.insertText', { text: '1' });
    await evaluate(`(() => { const nums = document.querySelectorAll('form input[type=number]'); nums[1].focus(); return true; })()`);
    await send('Input.insertText', { text: '5000' });
    await pickIn('select[aria-label="Account for line 1"]', '6000');
    await pickIn('select[aria-label="Payable account"]', '2400');
    await fillDates('2026-10-06', true);
    await shot('37c-bill-payable');
    await clickText('Save Bill');
    await waitFor(`document.body.innerText.includes('BILL-000002')`, 'bill saved');
    const bill = (await api('GET', '/api/bills')).body.data.find((b) => b.payableAccountId);
    if (!bill) throw new Error('bill saved without its payable account');
    await go('/bills');
    await rowAction(bill.billNumber, 'Post');
    await waitFor(`[...document.querySelectorAll('tr')].some((tr) => tr.innerText.includes(${JSON.stringify(bill.billNumber)}) && tr.innerText.includes('POSTED'))`, 'bill posted');
    const owed = (await api('GET', '/api/ledger/trial-balance')).body.find((r) => r.account.code === '2400');
    if (!owed || owed.balance !== 23337.13) throw new Error('telephone payable ' + JSON.stringify(owed && owed.balance));

    // Paying the payable account directly.
    await go('/payments');
    await clickText('+ Record Payment');
    await setv('form select', 'ACCOUNT', 0);
    await waitFor(`!!document.querySelector('select[aria-label="Payable account"]')`, 'payable picker');
    await pickIn('select[aria-label="Payable account"]', '2400');
    await setv('input[placeholder="Amount"]', '18337.13');
    await pick(3, '1000');
    await fillDates('2026-10-07');
    await shot('38-pay-payable-account');
    await clickText('Record Payment');
    await waitFor(`[...document.querySelectorAll('tr')].some((tr) => tr.innerText.includes('18,337.13') && tr.innerText.includes('2400'))`, 'payable payment listed');
    const left = (await api('GET', '/api/ledger/trial-balance')).body.find((r) => r.account.code === '2400');
    if (left.balance !== 5000) throw new Error('payable after payment ' + left.balance);

    // The USD account: a deposit in USD by journal entry, then its activity in USD.
    await go('/ledger');
    await clickText('+ New Journal Entry');
    await setv('input[aria-label="Entry date"]', '2026-10-08');
    await setv('input[placeholder^="Description"]', 'Transfer to the USD account');
    await pickIn('select[aria-label="Account for line 1"]', '1020');
    await waitFor(`!!document.querySelector('input[aria-label="Rate for line 1"]')`, 'rate field');
    await setv('input[aria-label="Debit for line 1"]', '100');
    await setv('input[aria-label="Rate for line 1"]', '300');
    await pickIn('select[aria-label="Account for line 2"]', '1000');
    await setv('input[aria-label="Credit for line 2"]', '30000');
    await waitFor(`document.querySelector('[data-testid=je-difference]').innerText.includes('Balanced')`, 'USD entry balanced');
    await shot('37b-journal-entry');
    await clickText('Post Entry');
    await waitFor(`document.body.innerText.includes('Transfer to the USD account')`, 'USD entry listed');
    const usd = (await api('GET', '/api/accounts')).body.find((a) => a.code === '1020');
    await go('/ledger/account/' + usd.id);
    await waitFor(`document.querySelector('[data-testid=foreign-closing]')?.innerText.includes('100.00')`, 'USD balance');
    await shot('39-usd-account');
    // The month-end revaluation dialog (closed again without posting).
    await go('/accounts');
    await rowAction('USD Bank', 'Revalue');
    await waitFor(`!!document.querySelector('input[aria-label="Revaluation rate"]')`, 'revalue dialog');
    await setv('input[aria-label="Revaluation rate"]', '305');
    await shot('39b-revalue');
    await clickText('Close');
  });

  await check('Settings page locks the books and postings on the lock date are refused (D19)', async () => {
    await settingsTab('Closing');
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

  await check('Administration: Admin Centre, user dialogs, plain-word roles, log filters (D36)', async () => {
    await go('/admin');
    await waitFor(`document.querySelectorAll('[data-testid=admin-tile]').length === 6 && document.body.innerText.includes('Latest activity')`, 'admin centre');
    await shot('40-admin-centre');
    await go('/users');
    await waitFor(`document.body.innerText.includes('clerk.one') && document.body.innerText.includes('Last sign-in')`, 'users table');
    // Deactivating yourself is not offered; the server refuses it anyway.
    const me = (await api('GET', '/api/users')).body.find((u) => u.username === 'admin');
    const self = await api('PATCH', `/api/users/${me.id}`, { isActive: false });
    if (self.status !== 400) throw new Error('self-deactivation answered ' + self.status);
    // Change role through the dialog.
    await evaluate(`(() => { const row = [...document.querySelectorAll('tr')].find((tr) => tr.innerText.includes('clerk.one'));
      row.querySelector('button').click(); return true; })()`);
    await waitFor(`[...document.querySelectorAll('button,[role=menuitem]')].some((x) => x.textContent.trim() === 'Change role…')`, 'user menu');
    await evaluate(`[...document.querySelectorAll('button,[role=menuitem]')].find((x) => x.textContent.trim() === 'Change role…').click()`);
    await waitFor(`!!document.querySelector('select[aria-label="New role"]')`, 'role dialog');
    await evaluate(`(() => { const sel = document.querySelector('select[aria-label="New role"]');
      const o = [...sel.options].find((x) => x.textContent === 'Accountant');
      Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, 'value').set.call(sel, o.value);
      sel.dispatchEvent(new Event('change', { bubbles: true })); return true; })()`);
    await shot('41b-change-role');
    await clickText('Save change');
    await waitFor(`!!document.querySelector('[data-testid=toast]') && [...document.querySelectorAll('tr')].some((tr) => tr.innerText.includes('clerk.one') && tr.innerText.includes('Accountant'))`, 'role changed');
    await shot('41-users');
    await go('/roles');
    await waitFor(`document.querySelectorAll('[data-testid=role-card]').length >= 3 && document.body.innerText.includes('Sales:')`, 'role cards');
    await shot('42-roles');
    // The activity log narrows to sign-ins.
    await go('/audit');
    await evaluate(`(() => { const sel = document.querySelector('select[aria-label="Kind of activity"]');
      Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, 'value').set.call(sel, 'signin');
      sel.dispatchEvent(new Event('change', { bubbles: true })); return true; })()`);
    await waitFor(`document.body.innerText.includes('the filters') && [...document.querySelectorAll('tbody tr')].every((tr) => /log/i.test(tr.innerText))`, 'sign-ins only');
    await shot('43-activity-log');
    const searched = await api('GET', '/api/audit-logs?search=company&limit=50');
    if (!searched.body.data.length || !searched.body.data.every((r) => /company/i.test(r.action + (r.detail ?? '') + (r.username ?? ''))))
      throw new Error('search returned ' + JSON.stringify(searched.body.data.map((r) => r.action)));
  });

  await check('Import from file: a bank statement is read, checked, imported and matched (D37)', async () => {
    const choose = (sel, text) => evaluate(`(() => { const el = document.querySelector(${JSON.stringify(sel)});
      const o = [...el.options].find((x) => x.textContent.includes(${JSON.stringify(text)})); if (!o) throw new Error('no option ' + ${JSON.stringify(text)});
      Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, 'value').set.call(el, o.value);
      el.dispatchEvent(new Event('change', { bubbles: true })); return true; })()`);
    const csv = join(SHOTS, 'statement.csv');
    writeFileSync(csv, '\uFEFFDate,Narration,Ref,Credit,Debit\r\n09/10/2026,Cash deposit,DEP-1,"12,500.00",\r\n10/10/2026,Bank charges October,,,250.00\r\n');
    await go('/import');
    await waitFor(`!!document.querySelector('[data-testid=kind-BANK]')`, 'import kinds');
    await evaluate(`document.querySelector('[data-testid=kind-BANK]').click()`);
    const doc = await send('DOM.getDocument', {});
    const { nodeId } = await send('DOM.querySelector', { nodeId: doc.root.nodeId, selector: 'input[type=file]' });
    await send('DOM.setFileInputFiles', { nodeId, files: [csv] });
    await waitFor(`!!document.querySelector('select[aria-label="Bank account"]')`, 'mapping step');
    // The headings were recognised: Credit = money in, Debit = money out, Narration = description.
    const mapped = await evaluate(`['Date', 'Description', 'Money in', 'Money out'].map((f) => document.querySelector('select[aria-label="Column for ' + f + '"]').selectedOptions[0].textContent)`);
    if (!mapped[1].includes('Narration') || !mapped[2].includes('Credit') || !mapped[3].includes('Debit')) throw new Error('mapping ' + mapped.join(' / '));
    await choose('select[aria-label="Bank account"]', '1000');
    await shot('44-import-map');
    await clickText('Next: check the rows');
    await waitFor(`document.querySelector('[data-testid=import-counts]')?.innerText.includes('2 new')`, 'preview counts');
    await shot('45-import-preview');
    await clickText('Import 2 rows');
    await waitFor(`[...document.querySelectorAll('button')].some((b) => b.textContent.trim() === 'Match the transactions now')`, 'import done');
    await clickText('Match the transactions now');
    await waitFor(`document.querySelectorAll('[data-testid=bank-lines] tbody tr').length === 2`, 'bank lines');
    // Record the bank charges as an expense through the row menu.
    await evaluate(`(() => { const row = [...document.querySelectorAll('[data-testid=bank-lines] tbody tr')].find((tr) => tr.innerText.includes('Bank charges'));
      row.querySelector('td:last-child button').click(); return true; })()`);
    await waitFor(`[...document.querySelectorAll('button,[role=menuitem]')].some((x) => x.textContent.trim() === 'Record as an expense…')`, 'line menu');
    await evaluate(`[...document.querySelectorAll('button,[role=menuitem]')].find((x) => x.textContent.trim() === 'Record as an expense…').click()`);
    await waitFor(`!!document.querySelector('select[aria-label="Expense account"]')`, 'expense dialog');
    await choose('select[aria-label="Expense account"]', '6000');
    await shot('46-bank-expense');
    await clickText('Record and post');
    await waitFor(`document.querySelectorAll('[data-testid=bank-lines] tbody tr').length === 1 && [...document.querySelectorAll('[role=tab]')].some((t) => t.innerText.includes('Matched') && t.innerText.includes('(1)'))`, 'one matched');
    await shot('47-bank-transactions');
    const exp = (await api('GET', '/api/expenses')).body.data.find((e) => e.description === 'Bank charges October');
    if (!exp || exp.status !== 'POSTED') throw new Error('expense from the bank line: ' + JSON.stringify(exp));
    // The import is listed and can be undone only once its lines are unmatched.
    const imports = (await api('GET', '/api/imports')).body;
    if (!imports.length || imports[0].created !== 2) throw new Error('import history ' + JSON.stringify(imports[0]));
    const blocked = await api('DELETE', `/api/imports/${imports[0].id}`);
    if (blocked.status !== 400) throw new Error('undo of a matched import answered ' + blocked.status);
  });

  await check('Products & stock: an item, opening stock, a sale with a discount at average cost (D38)', async () => {
    const choose = (sel, text) => evaluate(`(() => { const el = document.querySelector(${JSON.stringify(sel)});
      const o = [...el.options].find((x) => x.textContent.includes(${JSON.stringify(text)})); if (!o) throw new Error('no option ' + ${JSON.stringify(text)});
      Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, 'value').set.call(el, o.value);
      el.dispatchEvent(new Event('change', { bubbles: true })); return true; })()`);
    const rowMenu = async (rowText, label) => {
      await evaluate(`(() => { const row = [...document.querySelectorAll('[data-testid=items-table] tbody tr')].find((tr) => tr.innerText.includes(${JSON.stringify(rowText)}));
        row.querySelector('td:last-child button').click(); return true; })()`);
      await waitFor(`[...document.querySelectorAll('button,[role=menuitem]')].some((x) => x.textContent.trim() === ${JSON.stringify(label)})`, 'row menu');
      await evaluate(`[...document.querySelectorAll('button,[role=menuitem]')].find((x) => x.textContent.trim() === ${JSON.stringify(label)}).click()`);
    };
    await go('/items');
    await clickText('+ New product or service');
    await waitFor(`!!document.querySelector('input[aria-label="Name"]')`, 'item dialog');
    await type('input[aria-label="Name"]', 'Ceylon Tea 400g');
    await type('input[aria-label="Sale price"]', '850');
    await type('input[aria-label="Purchase cost"]', '500');
    await type('input[aria-label="Reorder point"]', '5');
    await clickText('Save');
    await waitFor(`document.querySelector('[data-testid=items-table]')?.innerText.includes('Ceylon Tea 400g')`, 'item listed');
    // Opening stock: 20 counted at 500, against Owner's Equity.
    await rowMenu('Ceylon Tea 400g', 'Adjust stock…');
    await waitFor(`!!document.querySelector('input[aria-label="Counted quantity"]')`, 'adjust dialog');
    await type('input[aria-label="Counted quantity"]', '20');
    await choose('select[aria-label="Adjustment account"]', '3000');
    await clickText('Adjust stock');
    await waitFor(`[...document.querySelectorAll('[data-testid=items-table] tbody tr')].some((tr) => tr.innerText.includes('Ceylon Tea') && tr.innerText.includes('10,000.00'))`, 'stock valued');
    await shot('48-items');
    // Sell 4 at 850 less 10%, no tax: 3,060.00; cost 4 × 500.
    await go('/invoices');
    await clickText('+ New Invoice');
    await waitFor(`!!document.querySelector('select[aria-label="Product for line 1"]')`, 'product picker');
    await type('input[placeholder="Name as it should print"]', 'Tea Buyer');
    await choose('select[aria-label="Product for line 1"]', 'Ceylon Tea');
    await waitFor(`document.querySelector('input[placeholder="Item description"]').value === 'Ceylon Tea 400g'`, 'line filled from the product');
    await evaluate(`(() => { const sel = document.querySelector('select[aria-label="Tax for line 1"]');
      if (sel) { sel.value = ''; sel.dispatchEvent(new Event('change', { bubbles: true })); } return true; })()`);
    await evaluate(`(() => { const el = document.querySelectorAll('.inv-sheet input[type=number]')[0]; el.focus(); el.select(); return true; })()`);
    await send('Input.insertText', { text: '4' });
    await type('input[aria-label="Discount percent for line 1"]', '10');
    await waitFor(`document.querySelector('.inv-discount')?.innerText.includes('340.00')`, 'discount shown on the invoice');
    await shot('49-invoice-product');
    await clickText('Save Invoice');
    await waitFor(`document.body.innerText.includes('Tea Buyer')`, 'invoice listed');
    const inv = (await api('GET', '/api/invoices')).body.data.find((i) => i.billToName === 'Tea Buyer');
    if (inv.totalAmount !== '3060.00') throw new Error('invoice total ' + inv.totalAmount);
    const posted = await api('POST', `/api/invoices/${inv.id}/post`);
    if (posted.status !== 201) throw new Error('post answered ' + posted.status + ' ' + JSON.stringify(posted.body));
    const item = (await api('GET', '/api/items')).body.find((i) => i.name === 'Ceylon Tea 400g');
    if (item.quantityOnHand !== 16 || item.stockValue !== 8000) throw new Error('stock after the sale ' + JSON.stringify(item));
    const tb = (await api('GET', '/api/ledger/trial-balance')).body;
    const disc = tb.find((r) => r.account.code === '4900');
    if (!disc || disc.balance !== -340) throw new Error('discounts given ' + JSON.stringify(disc));
    await go('/items');
    await waitFor(`!!document.querySelector('[data-testid=items-table]')`, 'items');
    await rowMenu('Ceylon Tea 400g', 'Stock history…');
    await waitFor(`document.querySelectorAll('[data-testid=stock-history] tbody tr').length === 2`, 'stock history');
    await shot('50-stock-history');
  });

  await check('Shopify: connect, match accounts, sync, and a fee recorded in one click (D39)', async () => {
    const choose = (sel, text) => evaluate(`(() => { const el = document.querySelector(${JSON.stringify(sel)});
      const o = [...el.options].find((x) => x.textContent.includes(${JSON.stringify(text)})); if (!o) throw new Error('no option ' + ${JSON.stringify(text)});
      Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, 'value').set.call(el, o.value);
      el.dispatchEvent(new Event('change', { bubbles: true })); return true; })()`);
    await settingsTab('Integrations');
    await waitFor(`!!document.querySelector('input[aria-label="Store address"]')`, 'connect form');
    await type('input[aria-label="Store address"]', 'smoke-tea');
    await type('input[aria-label="Access key"]', 'shpat_smoke_1234567890');
    await clickText('Connect');
    await waitFor(`document.body.innerText.includes('Connected to Smoke Tea Shop')`, 'connected');
    await clickText('+ Add “1090 Shopify Clearing”');
    await waitFor(`document.querySelector('select[aria-label="Shopify clearing account"]')?.selectedOptions[0]?.textContent.includes('1090')`, 'clearing account');
    await choose('select[aria-label="Sales account"]', '4000');
    await choose('select[aria-label="Payouts reach"]', '1000');
    await choose('select[aria-label="Shopify fees"]', '6000');
    await clickText('Save');
    await waitFor(`[...document.querySelectorAll('[data-testid=toast]')].some((t) => t.innerText.includes('Shopify settings saved'))`, 'saved');
    await clickText('Sync now');
    await waitFor(`document.querySelector('[data-testid=shopify-report]')?.innerText.includes('1 new order') && document.querySelector('[data-testid=shopify-report]')?.innerText.includes('3 payment lines')`, 'sync report', 30000);
    await shot('51-shopify');
    const draft = (await api('GET', '/api/invoices?status=DRAFT')).body.data.find((i) => i.poNumber === '#2001');
    if (!draft || draft.totalAmount !== '1500.00') throw new Error('Shopify order as a draft: ' + JSON.stringify(draft));
    // The clearing account: the fee is one click (its hint), the payout a transfer to Cash.
    const st = (await api('GET', '/api/integrations/shopify')).body;
    await go('/bank-transactions');
    await waitFor(`[...(document.querySelector('select[aria-label="Account"]')?.options ?? [])].some((o) => o.textContent.includes('1090'))`, 'account list');
    await choose('select[aria-label="Account"]', '1090');
    await waitFor(`document.querySelectorAll('[data-testid=bank-lines] tbody tr').length === 3`, 'clearing lines');
    await evaluate(`(() => { const row = [...document.querySelectorAll('[data-testid=bank-lines] tbody tr')].find((tr) => tr.innerText.includes('Shopify fee'));
      row.querySelector('[data-testid=hint]').click(); return true; })()`);
    await waitFor(`document.querySelectorAll('[data-testid=bank-lines] tbody tr').length === 2`, 'fee recorded');
    await evaluate(`(() => { const row = [...document.querySelectorAll('[data-testid=bank-lines] tbody tr')].find((tr) => tr.innerText.includes('payout'));
      row.querySelector('[data-testid=hint]').click(); return true; })()`);
    await waitFor(`document.querySelectorAll('[data-testid=bank-lines] tbody tr').length === 1`, 'payout recorded');
    await shot('52-shopify-clearing');
    const tb = (await api('GET', '/api/ledger/trial-balance')).body;
    // Fee and payout are booked; the payment waits for its invoice to be posted, so until then
    // the clearing account shows what left it (45 + 1,455).
    const clearing = tb.find((r) => r.account.code === '1090');
    if (!clearing || clearing.balance !== -1500) throw new Error('clearing account: ' + JSON.stringify(clearing));
    if (st.lastError) throw new Error('last error ' + st.lastError);
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
