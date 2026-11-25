// D61 UI test: recurring invoices in headless Microsoft Edge (Chrome DevTools Protocol, no npm
// dependencies) against a running accounting-server.
//
//   node tests/ui/recurring-invoices.mjs http://127.0.0.1:47800 <screenshot-dir>
//
// Start the server on a fresh, empty data folder with ADMIN_USERNAME=admin and
// ADMIN_PASSWORD=admin12345.

import { spawn } from 'node:child_process';
import { mkdirSync, writeFileSync, mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const BASE = process.argv[2] ?? 'http://127.0.0.1:47800';
const SHOTS = process.argv[3] ?? join(tmpdir(), 'accounting-ui-recurring');
mkdirSync(SHOTS, { recursive: true });
const EDGE = 'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe';
const PORT = 9336;
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

// ── Browser plumbing ──────────────────────────────────────────────────────
const profile = mkdtempSync(join(tmpdir(), 'edge-recurring-'));
const edge = spawn(EDGE, ['--headless=new', `--remote-debugging-port=${PORT}`, `--user-data-dir=${profile}`,
  '--no-first-run', '--disable-extensions', '--window-size=1366,900', 'about:blank'], { stdio: 'ignore' });
let ws; let nextId = 1;
const pending = new Map();
const failures = []; const results = []; const httpErrors = []; const consoleErrors = [];

async function connect() {
  for (let i = 0; i < 50; i++) {
    try {
      const page = (await (await fetch(`http://127.0.0.1:${PORT}/json`)).json()).find((t) => t.type === 'page');
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
          } else if (msg.method === 'Runtime.exceptionThrown') {
            const d = msg.params.exceptionDetails;
            consoleErrors.push(`exception: ${(d.exception?.description ?? d.text ?? '').slice(0, 400)}`);
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
async function waitFor(expr, label, timeout = 15000) {
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
  if (el.disabled) throw new Error('${tag} is disabled: ' + ${JSON.stringify(text)});
  el.click(); return true; })()`);
const pageText = () => evaluate('document.body.innerText');
async function shot(name) {
  const { data } = await send('Page.captureScreenshot', { format: 'png', captureBeyondViewport: true });
  writeFileSync(join(SHOTS, `${name}.png`), Buffer.from(data, 'base64'));
}
async function go(path) {
  await send('Page.navigate', { url: BASE + path });
  await waitFor(`document.readyState === 'complete' && location.pathname === ${JSON.stringify(path)}`, `load ${path}`);
  await sleep(600);
}
async function check(name, fn) {
  try {
    await Promise.race([fn(), sleep(90000).then(() => { throw new Error('step timed out after 90 s'); })]);
    results.push(`PASS  ${name}`);
  } catch (e) {
    failures.push(name);
    results.push(`FAIL  ${name}: ${e.message}`);
    await shot(`FAIL-${name.replace(/\W+/g, '_')}`).catch(() => {});
  }
  console.log(results.at(-1));
}
/** The signed-in user's API call from inside the page. */
const api = (method, path, body) => evaluate(`(async () => {
  const token = localStorage.getItem('accessToken');
  const r = await fetch('/api' + ${JSON.stringify(path)}, { method: ${JSON.stringify(method)},
    headers: { 'content-type': 'application/json', authorization: 'Bearer ' + token }, body: ${body ? JSON.stringify(JSON.stringify(body)) : 'undefined'} });
  return { status: r.status, body: await r.json().catch(() => null) };
})()`);
const balanceOf = async (code) => {
  const accounts = (await api('GET', '/accounts')).body;
  const a = accounts.find((x) => x.code === code);
  return a ? (await api('GET', `/ledger/balance/${a.id}`)).body.balance : null;
};
const selectByLabel = (aria, text) => evaluate(`(() => {
  const s = document.querySelector(${JSON.stringify(`select[aria-label="${aria}"]`)});
  if (!s) throw new Error('no select ' + ${JSON.stringify(aria)});
  const o = [...s.options].find((x) => x.textContent.startsWith(${JSON.stringify(text)}));
  if (!o) throw new Error('no option ' + ${JSON.stringify(text)});
  s.value = o.value; s.dispatchEvent(new Event('change', { bubbles: true })); return true; })()`);

/** Sets a React-controlled input or select by selector and fires its change. */
const setValue = (selector, value) => evaluate(`(() => {
  const el = document.querySelector(${JSON.stringify(selector)});
  if (!el) throw new Error('no element ' + ${JSON.stringify(selector)});
  const proto = el.tagName === 'SELECT' ? HTMLSelectElement.prototype : HTMLInputElement.prototype;
  Object.getOwnPropertyDescriptor(proto, 'value').set.call(el, ${JSON.stringify(value)});
  el.dispatchEvent(new Event(el.tagName === 'SELECT' ? 'change' : 'input', { bubbles: true }));
  el.dispatchEvent(new Event('change', { bubbles: true }));
  return true; })()`);
/** Opens the ⋮ menu of the row containing `text` and clicks `item`. */
const rowAction = async (text, item) => {
  // The list reloads after each action; retry while a stale menu or row is on screen.
  for (let attempt = 1; ; attempt++) {
    try { await rowActionOnce(text, item); return; } catch (e) {
      if (attempt >= 4) throw e;
      await evaluate('document.body.click(); true');
      await sleep(400);
    }
  }
};
const rowActionOnce = async (text, item) => {
  await evaluate(`(() => {
    const row = [...document.querySelectorAll('tbody tr')].find((r) => r.innerText.includes(${JSON.stringify(text)}));
    if (!row) throw new Error('no row ' + ${JSON.stringify(text)});
    row.querySelector('button[aria-label=Actions]').click();
    return true; })()`);
  await sleep(200);
  await evaluate(`(() => {
    const b = [...document.querySelectorAll('[role=menu] button')].find((x) => x.textContent.trim() === ${JSON.stringify(item)});
    if (!b) throw new Error('no menu item ' + ${JSON.stringify(item)});
    b.click(); return true; })()`);
};
const daysAgo = (n) => { const d = new Date(); d.setDate(d.getDate() - n); return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`; };

try {
  await connect();
  await send('Page.enable');
  await send('Runtime.enable');
  await send('Network.enable');

  await check('sign in, company, customer and an invoice', async () => {
    await go('/login');
    await type('input[type=text]', 'admin');
    await type('input[type=password]', 'admin12345');
    await evaluate(`document.querySelector('button[type=submit]').click()`);
    await waitFor(`location.pathname !== '/login'`, 'signed in');
    const r = await api('PATCH', '/settings', { 'company.name': 'Sample Trading', 'region.country': 'LK', 'region.currency': 'LKR', 'region.locale': 'en-LK' });
    if (r.status !== 200) throw new Error(JSON.stringify(r));
    const c = await api('POST', '/customers', { name: 'Lanka Rentals' });
    const inv = await api('POST', '/invoices', { customerId: c.body.id, date: daysAgo(14), lines: [{ description: 'Office rent', unitPrice: 50000 }] });
    if (inv.status !== 201) throw new Error(JSON.stringify(inv));
  });

  await check('Make recurring from an invoice catches up missed weeks as drafts', async () => {
    await go('/invoices');
    await waitFor(`document.body.innerText.includes('INV-')`, 'invoice listed');
    await rowAction('Lanka Rentals', 'Make recurring');
    await waitFor(`!!document.querySelector('form[aria-label^="Make INV-"]')`, 'schedule form');
    await shot('01-make-recurring');
    await setValue('select[aria-label=Frequency]', 'WEEKLY');
    await setValue('input[aria-label="First invoice on"]', daysAgo(14));
    await clickText('Make recurring');
    await waitFor(`document.body.innerText.includes('3 draft invoice(s) made')`, 'toast');
    await waitFor(`document.body.innerText.includes('3 invoices made by recurring schedules are waiting')`, 'waiting banner');
    const drafts = (await api('GET', '/invoices?status=DRAFT&limit=100')).body;
    if (drafts.total !== 4) throw new Error('drafts: ' + drafts.total);
    await shot('02-waiting-banner');
  });

  await check('the Recurring Invoices list shows the schedule', async () => {
    await evaluate(`[...document.querySelectorAll('a')].find((a) => a.textContent.trim() === 'Recurring Invoices').click()`);
    await waitFor(`location.pathname === '/recurring-invoices' && document.body.innerText.includes('Every week')`, 'list');
    const t = await pageText();
    for (const want of ['Lanka Rentals', '3 to post', 'ACTIVE']) if (!t.includes(want)) throw new Error('missing ' + want);
    await shot('03-list');
  });

  await check('pause, resume and change the schedule', async () => {
    await rowAction('Lanka Rentals', 'Pause');
    await waitFor(`document.body.innerText.includes('PAUSED')`, 'paused');
    await rowAction('Lanka Rentals', 'Resume');
    await waitFor(`document.body.innerText.includes('ACTIVE')`, 'resumed');
    await rowAction('Lanka Rentals', 'Change schedule');
    await waitFor(`!!document.querySelector('input[aria-label="Repeat every"]')`, 'edit form');
    await setValue('input[aria-label="Repeat every"]', '2');
    await clickText('Save schedule');
    await waitFor(`document.body.innerText.includes('Every 2 weeks')`, 'changed');
    const list = (await api('GET', '/recurring-invoices')).body;
    if (list[0].invoicesMade !== 3) throw new Error('changing made new invoices: ' + list[0].invoicesMade);
  });

  await check('stop keeps the invoices it made', async () => {
    await evaluate('window.confirm = () => true; true');
    await rowAction('Lanka Rentals', 'Stop');
    await waitFor(`document.body.innerText.includes('stopped')`, 'stopped toast');
    const list = (await api('GET', '/recurring-invoices')).body;
    if (list.length !== 0) throw new Error('schedule still listed');
    const drafts = (await api('GET', '/invoices?status=DRAFT&limit=100')).body;
    if (drafts.total !== 4) throw new Error('drafts after stop: ' + drafts.total);
    const tb = (await api('GET', '/ledger/trial-balance')).body;
    if (tb.some((r) => r.balance !== 0)) throw new Error('a draft reached the ledger');
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
