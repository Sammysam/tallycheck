// D59 UI test: "Move books in" from QuickBooks Online-style exports, in headless Microsoft Edge
// (Chrome DevTools Protocol, no npm dependencies) against a running accounting-server.
//
//   node tests/ui/move-books.mjs http://127.0.0.1:47800 <screenshot-dir>
//
// Start the server on a fresh, empty data folder with ADMIN_USERNAME=admin and
// ADMIN_PASSWORD=admin12345. The files below are laid out like QuickBooks Online's own exports:
// title lines, a blank first heading, customers on their own rows, "Total for …" rows, US dates.

import { spawn } from 'node:child_process';
import { mkdirSync, writeFileSync, mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const BASE = process.argv[2] ?? 'http://127.0.0.1:47800';
const SHOTS = process.argv[3] ?? join(tmpdir(), 'accounting-ui-move-books');
mkdirSync(SHOTS, { recursive: true });
const EDGE = 'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe';
const PORT = 9334;
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

// ── QuickBooks Online-style exports ───────────────────────────────────────
const dir = mkdtempSync(join(tmpdir(), 'move-books-'));
const file = (name, lines) => { const p = join(dir, name); writeFileSync(p, '\ufeff' + lines.join('\r\n') + '\r\n'); return p; };
const FILES = {
  balances: file('Trial Balance.csv', [
    'Sample Company', 'Trial Balance', 'As of October 31, 2026', '',
    ',Debit,Credit',
    'Checking,"50,000.00",',
    'Accounts Receivable (A/R),"1,500.00",',
    'Office Equipment,"10,000.00",',
    'Accounts Payable (A/P),,800.00',
    "Owner's Equity,,\"60,000.00\"",
    'Sales,,700.00',
    'TOTAL,"$61,500.00","$61,500.00"', '',
    'Accrual basis Monday, November 17, 2026 10:00 AM GMT+05:30',
  ]),
  accounts: file('Account List.csv', [
    'Account List', '',
    'Account,Type,Detail type,Description,Balance',
    'Checking,Bank,Checking,,"50,000.00"',
    'Accounts Receivable (A/R),Accounts receivable (A/R),Accounts Receivable (A/R),,"1,500.00"',
    'Office Equipment,Fixed Assets,Machinery & Equipment,,"10,000.00"',
    'Accounts Payable (A/P),Accounts payable (A/P),Accounts Payable (A/P),,800.00',
    "Owner's Equity,Equity,Owner's Equity,,\"60,000.00\"",
    'Sales,Income,Sales of Product Income,,700.00',
  ]),
  invoices: file('Open Invoices.csv', [
    'Sample Company', 'Open Invoices', 'All Dates', '',
    ',Date,Transaction Type,Num,Terms,Due Date,Open Balance',
    'Ceylon Tea',
    ',10/20/2026,Invoice,1002,Net 30,11/19/2026,500.00',
    'Total for Ceylon Tea,,,,,,$500.00',
    'Lanka Traders',
    ',09/15/2026,Invoice,1001,Net 30,10/15/2026,"1,000.00"',
    'Total for Lanka Traders,,,,,,"$1,000.00"',
    'TOTAL,,,,,,"$1,500.00"',
  ]),
  bills: file('Unpaid Bills.csv', [
    'Sample Company', 'Unpaid Bills', 'All Dates', '',
    ',Date,Transaction Type,Num,Due Date,Past Due,Amount,Open Balance',
    'Colombo Packaging',
    ',10/01/2026,Bill,B-77,10/31/2026,20,800.00,800.00',
    'Total for Colombo Packaging,,,,,,,$800.00',
    'TOTAL,,,,,,,$800.00',
  ]),
  customers: file('Customers.csv', [
    'Customer,Phone Numbers,Email,Full Name,Billing Address,Open Balance',
    'Ceylon Tea,011 222 3333,orders@ceylontea.example,Nimal Perera,"8 Hill Street, Kandy",500.00',
    'Lanka Traders,011 234 5678,accounts@lanka.example,Sunil Fernando,"12 Main Street, Colombo","1,000.00"',
    'Galle Spice Co,091 222 1111,hello@gallespice.example,,"3 Fort Road, Galle",0.00',
  ]),
};

// ── Browser plumbing ──────────────────────────────────────────────────────
const profile = mkdtempSync(join(tmpdir(), 'edge-move-books-'));
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
/** Puts a local file into the file input of the section card labelled `label`. */
async function pick(label, path) {
  const { result } = await send('Runtime.evaluate', {
    expression: `document.querySelector('section[aria-label=${JSON.stringify(label)}] input[type=file]')`,
  });
  if (!result.objectId) throw new Error('no file input for ' + label);
  await send('DOM.setFileInputFiles', { objectId: result.objectId, files: [path] });
  await waitFor(`document.querySelector('section[aria-label=${JSON.stringify(label)}]').innerText.includes('row')`, `${label} read`);
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

try {
  await connect();
  await send('Page.enable');
  await send('Runtime.enable');
  await send('Network.enable');

  await check('sign in and set up the company', async () => {
    await go('/login');
    await type('input[type=text]', 'admin');
    await type('input[type=password]', 'admin12345');
    await evaluate(`document.querySelector('button[type=submit]').click()`);
    await waitFor(`location.pathname !== '/login'`, 'signed in');
    const r = await api('PATCH', '/settings', { 'company.name': 'Sample Trading', 'region.country': 'LK', 'region.currency': 'LKR', 'region.locale': 'en-LK' });
    if (r.status !== 200) throw new Error(JSON.stringify(r));
  });

  await check('File menu and Import from File lead to Move Books In', async () => {
    await go('/import');
    await waitFor(`[...document.querySelectorAll('a')].some((a) => a.textContent.includes('Moving from QuickBooks'))`, 'link on import page');
    await evaluate(`[...document.querySelectorAll('a')].find((a) => a.textContent.includes('Moving from QuickBooks')).click()`);
    await waitFor(`location.pathname === '/move-books' && document.body.innerText.includes('How it works')`, 'move books page');
    await shot('01-start');
  });

  await check('reads QuickBooks exports: title lines, blank headings, grouped names, totals', async () => {
    await type('input[type=date]', '');
    await evaluate(`(() => { const d = document.querySelector('input[type=date]');
      const set = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set;
      set.call(d, '2026-10-31'); d.dispatchEvent(new Event('input', { bubbles: true })); d.dispatchEvent(new Event('change', { bubbles: true })); return true; })()`);
    await clickText('Next: choose the files');
    await waitFor(`!!document.querySelector('section[aria-label="Trial balance"]')`, 'file cards');
    await pick('Trial balance', FILES.balances);
    await pick('Chart of accounts', FILES.accounts);
    await pick('Unpaid invoices', FILES.invoices);
    await pick('Unpaid bills', FILES.bills);
    await pick('Customers', FILES.customers);
    const text = await pageText();
    for (const want of ['Trial Balance.csv · 6 rows', 'Open Invoices.csv · 2 rows', 'Unpaid Bills.csv · 1 row', 'Customers.csv · 3 rows']) {
      if (!text.includes(want)) throw new Error('missing: ' + want);
    }
    if (text.includes('Still to match')) throw new Error('columns not matched');
    // US dates in the QuickBooks files switch the date order.
    await evaluate(`[...document.querySelectorAll('.card ~ div button, div > button')].filter((b) => b.textContent.trim() === 'Back').at(-1).click()`);
    await waitFor(`document.body.innerText.includes('How it works')`, 'back at the start');
    const order = await evaluate(`[...document.querySelectorAll('select')].find((s) => [...s.options].some((o) => o.value === 'MDY')).value`);
    if (order !== 'MDY') throw new Error('date order ' + order);
    await clickText('Next: choose the files');
    await shot('02-files');
  });

  await check('suggests account matches and new accounts', async () => {
    await clickText('Next: match the accounts');
    await waitFor(`document.body.innerText.includes('Match each QuickBooks Online account')`, 'accounts step');
    const chosen = (k) => evaluate(`(() => { const s = document.querySelector(${JSON.stringify(`select[aria-label="TallyCheck account for ${k}"]`)}); return s.options[s.selectedIndex].textContent; })()`);
    if (!(await chosen('Accounts Receivable (A/R)')).startsWith('1100')) throw new Error('A/R not matched to 1100');
    if (!(await chosen('Accounts Payable (A/P)')).startsWith('2000')) throw new Error('A/P not matched to 2000');
    if (!(await chosen("Owner's Equity")).startsWith('3000')) throw new Error("Owner's Equity not matched");
    const officeType = await evaluate(`document.querySelector('select[aria-label="Type for Office Equipment"]').value`);
    if (officeType !== 'ASSET') throw new Error('Office Equipment type ' + officeType);
    // The bank account: use the app's own 1000 Cash.
    await selectByLabel('TallyCheck account for Checking', '1000 ');
    await shot('03-accounts');
  });

  await check('the check shows everything adds up, then the books move in', async () => {
    await clickText('Next: check everything');
    await waitFor(`document.body.innerText.includes('Everything adds up')`, 'all agree');
    await shot('04-check');
    await clickText('Move the books in');
    await waitFor(`!!document.querySelector('[role=dialog]')`, 'confirm dialog');
    await evaluate(`[...document.querySelectorAll('[role=dialog] button')].find((b) => b.textContent.trim() === 'Move the books in').click()`);
    await waitFor(`document.body.innerText.includes('Moved books in from QuickBooks Online, cut-over 2026-10-31')`, 'done');
    await shot('05-done');
    const want = { '1000': 50000, '1100': 1500, '2000': 800, '3900': 0 };
    for (const [code, v] of Object.entries(want)) {
      const got = await balanceOf(code);
      if (got !== v) throw new Error(`balance ${code} = ${got}, expected ${v}`);
    }
    const invoices = (await api('GET', '/invoices')).body.data;
    const i1001 = invoices.find((i) => i.poNumber === '1001');
    if (invoices.length !== 2 || i1001?.status !== 'POSTED' || i1001.date !== '2026-09-15' || i1001.dueDate !== '2026-10-15') {
      throw new Error(JSON.stringify(invoices.map((i) => [i.poNumber, i.status, i.date, i.dueDate])));
    }
    const customers = (await api('GET', '/customers')).body;
    const lanka = customers.find((c) => c.name === 'Lanka Traders');
    if (customers.length !== 3 || lanka?.email !== 'accounts@lanka.example') throw new Error(JSON.stringify(customers.map((c) => [c.name, c.email])));
  });

  await check('a second move is refused while the first is in force', async () => {
    await clickText('Back to the start');
    await waitFor(`document.body.innerText.includes('Books were already moved in from QuickBooks Online')`, 'warning');
    const disabled = await evaluate(`[...document.querySelectorAll('button')].find((b) => b.textContent.trim() === 'Next: choose the files').disabled`);
    if (!disabled) throw new Error('Next is not disabled');
  });

  await check('the conversion can be reversed, nothing deleted', async () => {
    await evaluate(`[...document.querySelectorAll('button')].find((b) => b.textContent.trim() === 'Reverse').click()`);
    await waitFor(`!!document.querySelector('[role=dialog]')`, 'confirm dialog');
    await shot('06-reverse-confirm');
    await evaluate(`[...document.querySelectorAll('[role=dialog] button')].find((b) => b.textContent.trim() === 'Reverse').click()`);
    await waitFor(`document.body.innerText.includes('Reversed')`, 'reversed');
    for (const code of ['1000', '1100', '2000', '3900']) {
      const got = await balanceOf(code);
      if (got !== 0) throw new Error(`balance ${code} = ${got} after reversing`);
    }
    const invoices = (await api('GET', '/invoices')).body.data;
    if (!invoices.every((i) => i.status === 'VOID')) throw new Error('invoices not voided');
    await shot('07-reversed');
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
