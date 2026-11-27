// D60 UI test: estimates in headless Microsoft Edge (Chrome DevTools Protocol, no npm
// dependencies) against a running accounting-server.
//
//   node tests/ui/estimates.mjs http://127.0.0.1:47800 <screenshot-dir>
//
// Start the server on a fresh, empty data folder with ADMIN_USERNAME=admin and
// ADMIN_PASSWORD=admin12345.

import { spawn } from 'node:child_process';
import { mkdirSync, writeFileSync, mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const BASE = process.argv[2] ?? 'http://127.0.0.1:47800';
const SHOTS = process.argv[3] ?? join(tmpdir(), 'accounting-ui-estimates');
mkdirSync(SHOTS, { recursive: true });
const EDGE = 'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe';
const PORT = 9335;
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

// ── Browser plumbing ──────────────────────────────────────────────────────
const profile = mkdtempSync(join(tmpdir(), 'edge-estimates-'));
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
  const proto = el.tagName === 'SELECT' ? HTMLSelectElement.prototype : el.tagName === 'TEXTAREA' ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
  Object.getOwnPropertyDescriptor(proto, 'value').set.call(el, ${JSON.stringify(value)});
  el.dispatchEvent(new Event(el.tagName === 'SELECT' ? 'change' : 'input', { bubbles: true }));
  el.dispatchEvent(new Event('change', { bubbles: true }));
  return true; })()`);
/** Opens the ⋮ menu of the row containing `text` and clicks `item`. */
const rowAction = async (text, item) => {
  await evaluate(`(() => {
    const row = [...document.querySelectorAll('tr')].find((r) => r.innerText.includes(${JSON.stringify(text)}));
    if (!row) throw new Error('no row ' + ${JSON.stringify(text)});
    row.querySelector('button[aria-label=Actions]').click();
    return true; })()`);
  await sleep(200);
  await evaluate(`(() => {
    const b = [...document.querySelectorAll('[role=menu] button')].find((x) => x.textContent.trim() === ${JSON.stringify(item)});
    if (!b) throw new Error('no menu item ' + ${JSON.stringify(item)});
    b.click(); return true; })()`);
};

let customerId;
try {
  await connect();
  await send('Page.enable');
  await send('Runtime.enable');
  await send('Network.enable');

  await check('sign in, company and a customer', async () => {
    await go('/login');
    await type('input[type=text]', 'admin');
    await type('input[type=password]', 'admin12345');
    await evaluate(`document.querySelector('button[type=submit]').click()`);
    await waitFor(`location.pathname !== '/login'`, 'signed in');
    const r = await api('PATCH', '/settings', { 'company.name': 'Sample Trading', 'region.country': 'LK', 'region.currency': 'LKR', 'region.locale': 'en-LK' });
    if (r.status !== 200) throw new Error(JSON.stringify(r));
    const c = await api('POST', '/customers', { name: 'Perera Stores', address: '8 Hill Street, Kandy' });
    if (c.status !== 201) throw new Error(JSON.stringify(c));
    customerId = c.body.id;
  });

  await check('Estimates is in the navigation and opens an estimate sheet', async () => {
    await go('/invoices');
    await waitFor(`[...document.querySelectorAll('a')].some((a) => a.textContent.trim() === 'Estimates')`, 'nav link');
    await evaluate(`[...document.querySelectorAll('a')].find((a) => a.textContent.trim() === 'Estimates').click()`);
    await waitFor(`location.pathname === '/estimates' && document.body.innerText.includes('+ New Estimate')`, 'estimates page');
    await clickText('+ New Estimate');
    await waitFor(`!!document.querySelector('.inv-sheet h1')`, 'sheet');
    const t = await pageText();
    const h1 = await evaluate(`document.querySelector('.inv-sheet h1').textContent`);
    if (h1 !== 'Estimate') throw new Error('heading is ' + h1);
    if (!t.includes('Valid until:')) throw new Error('no Valid until');
    if (t.includes('PO No:') || t.includes('Due Date:')) throw new Error('invoice-only fields shown');
    await shot('01-new-estimate');
  });

  await check('saves an estimate with a discount', async () => {
    await setValue('.inv-side select', customerId);
    await waitFor(`document.querySelector('.inv-input.bill-to').value === 'Perera Stores'`, 'bill to filled');
    await setValue('.inv-sheet tbody input[placeholder="Item description"]', 'Ceylon tea 1kg');
    const cells = await evaluate(`document.querySelectorAll('.inv-sheet tbody tr:first-child td.c-num input').length`);
    if (cells !== 2) throw new Error('expected quantity and price inputs, found ' + cells);
    await evaluate(`(() => {
      const [q, p] = document.querySelectorAll('.inv-sheet tbody tr:first-child td.c-num input');
      q.setAttribute('data-t', 'qty'); p.setAttribute('data-t', 'price'); return true; })()`);
    await setValue('input[data-t=qty]', '10');
    await setValue('input[data-t=price]', '1500');
    await setValue('input[aria-label="Discount for line 1"]', '1000');
    await setValue('.meta-right .meta-row:last-child input[type=date]', '2099-12-31');
    await waitFor(`document.body.innerText.includes('14,000.00')`, 'line total 14,000');
    await clickText('Save Estimate');
    await waitFor(`location.pathname === '/estimates' && document.body.innerText.includes('EST-0001')`, 'listed');
    const e = (await api('GET', '/estimates')).body.data[0];
    if (e.totalAmount !== '14000.00' || e.expiryDate !== '2099-12-31' || e.status !== 'DRAFT') throw new Error(JSON.stringify(e));
    await shot('02-list');
  });

  await check('marks it accepted from the list', async () => {
    await setValue('select[aria-label="Status of EST-0001"]', 'ACCEPTED');
    await waitFor(`document.body.innerText.includes('marked accepted')`, 'toast');
    const e = (await api('GET', '/estimates')).body.data[0];
    if (e.status !== 'ACCEPTED') throw new Error(e.status);
  });

  await check('turns it into a draft invoice', async () => {
    await evaluate('window.confirm = () => true; window.__tcAutoConfirm = true; true');
    await rowAction('EST-0001', 'Turn into invoice');
    await waitFor(`document.body.innerText.includes('is now draft invoice')`, 'converted toast');
    const inv = (await api('GET', '/invoices')).body.data[0];
    if (inv.status !== 'DRAFT' || inv.totalAmount !== '14000.00' || inv.customerId !== customerId) throw new Error(JSON.stringify(inv));
    await waitFor(`document.body.innerText.includes(${JSON.stringify(inv.invoiceNumber)})`, 'invoice number on the row');
    const tb = (await api('GET', '/ledger/trial-balance')).body;
    if (tb.some((r) => r.balance !== 0)) throw new Error('an estimate or draft reached the ledger');
    await shot('03-converted');
  });

  await check('a converted estimate offers no edit, convert or delete', async () => {
    await evaluate(`document.querySelector('tbody tr button[aria-label=Actions]').click(); true`);
    await sleep(200);
    const items = await evaluate(`[...document.querySelectorAll('[role=menu] button')].map((b) => b.textContent.trim())`);
    for (const bad of ['Edit', 'Turn into invoice', 'Delete']) if (items.includes(bad)) throw new Error('menu has ' + bad);
    for (const good of ['Preview', 'Print', 'Download']) if (!items.includes(good)) throw new Error('menu lacks ' + good);
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
