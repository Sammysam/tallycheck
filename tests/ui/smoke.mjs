// UI smoke test: drives the real React frontend in headless Microsoft Edge (Chrome DevTools
// Protocol, no npm dependencies) against a running accounting-server.
//
//   node tests/ui/smoke.mjs http://127.0.0.1:47800 <screenshot-dir>
//
// The server must have a fresh database with ADMIN_USERNAME=admin / ADMIN_PASSWORD=admin12345.

import { spawn } from 'node:child_process';
import { createHmac } from 'node:crypto';
import { mkdirSync, writeFileSync, mkdtempSync } from 'node:fs';
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
