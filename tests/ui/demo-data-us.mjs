// US demo company: types a realistic, made-up US business into the app THROUGH THE SCREENS (the
// same forms a person uses) into an empty data folder; employees and US payroll go through the API.
// The Sri Lankan demo is demo-data.mjs.
//
//   node tests/ui/demo-data-us.mjs http://127.0.0.1:47811 <screenshot-dir>
//
// The server must point at an EMPTY data folder. Everything here is fictitious:
// Lone Star Office Supply Co., Austin, Texas, April – October 2026 (fiscal year from April).

import { spawn } from 'node:child_process';
import { existsSync, mkdirSync, writeFileSync, mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';

const BASE = process.argv[2] ?? 'http://127.0.0.1:47811';
const SHOTS = process.argv[3] ?? join(tmpdir(), 'accounting-demo');
mkdirSync(SHOTS, { recursive: true });
const LOGO = resolve('docs/demo/lonestar-logo.png');
// A Chromium browser to drive: DEMO_BROWSER, or Edge / Chrome where Windows and macOS install them.
const EDGE = process.env.DEMO_BROWSER ?? [
  'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe',
  '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',
  '/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge',
  '/Applications/Chromium.app/Contents/MacOS/Chromium',
].find((p) => existsSync(p)) ?? 'msedge';
const PORT = 9334;
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

// Sign-in details for the demo copy only; also listed in docs/demo/README.md.
const ADMIN = { username: 'admin', password: 'LoneStarDemo2026' };
const USERS = [
  { username: 'emily.accounts', password: 'LoneStarAcct26', role: 'Accountant' },
  { username: 'owner.view', password: 'LoneStarView26', role: 'Viewer' },
];

// ── Browser plumbing (Chrome DevTools Protocol, as in smoke.mjs) ─────────────
const profile = mkdtempSync(join(tmpdir(), 'edge-demo-'));
const edge = spawn(EDGE, ['--headless=new', `--remote-debugging-port=${PORT}`, `--user-data-dir=${profile}`,
  '--no-first-run', '--disable-extensions', '--window-size=1366,900', 'about:blank'], { stdio: 'ignore' });
let ws;
let nextId = 1;
const pending = new Map();
const problems = [];

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
            const { resolve: ok, reject } = pending.get(msg.id);
            pending.delete(msg.id);
            msg.error ? reject(new Error(msg.error.message)) : ok(msg.result);
          } else if (msg.method === 'Network.responseReceived') {
            const r = msg.params.response;
            if (r.status >= 400 && r.url.includes('/api/')) problems.push(`${r.status} ${new URL(r.url).pathname}`);
          }
        };
        return;
      }
    } catch { /* Edge still starting */ }
    await sleep(200);
  }
  throw new Error('could not connect to Edge');
}
const send = (method, params = {}) => new Promise((ok, reject) => {
  const id = nextId++;
  pending.set(id, { resolve: ok, reject });
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
const J = JSON.stringify;
/** In-page helpers: set a control's value the way React notices, pick an option by its text. */
const HELPERS = `
  window.__set = (el, v) => {
    const p = el instanceof HTMLSelectElement ? HTMLSelectElement.prototype : el instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
    Object.getOwnPropertyDescriptor(p, 'value').set.call(el, v);
    el.dispatchEvent(new Event(el instanceof HTMLSelectElement ? 'change' : 'input', { bubbles: true }));
  };
  window.__opt = (sel, text) => {
    const o = [...sel.options].find((x) => x.value === text || x.textContent.includes(text));
    if (!o) throw new Error('no option "' + text + '" in: ' + [...sel.options].map((x) => x.textContent).join(' | '));
    window.__set(sel, o.value);
  };
  window.__q = (s, n = 0) => { const el = document.querySelectorAll(s)[n]; if (!el) throw new Error('nothing matches ' + s + ' [' + n + ']'); return el; };
  window.confirm = () => true; window.alert = (m) => { window.__alerts = [...(window.__alerts || []), String(m)]; };
  true;`;
async function go(path) {
  await send('Page.navigate', { url: BASE + path });
  await waitFor(`document.readyState === 'complete' && location.pathname === ${J(path)}`, `load ${path}`);
  await sleep(700);
  await evaluate(HELPERS);
}
/** Sets the n-th element matching a CSS selector. */
const set = (sel, v, n = 0) => evaluate(`(() => { __set(__q(${J(sel)}, ${n}), ${J(String(v))}); return true; })()`);
/** Picks an option (by value or text) in the n-th <select> matching the selector. */
const choose = (sel, text, n = 0) => evaluate(`(() => { __opt(__q(${J(sel)}, ${n}), ${J(text)}); return true; })()`);
const clickText = (text, tag = 'button') => evaluate(`(() => {
  const el = [...document.querySelectorAll(${J(tag)})].find((b) => b.textContent.trim() === ${J(text)} && !b.disabled);
  if (!el) throw new Error('no enabled ${tag} "' + ${J(text)} + '"');
  el.click(); return true; })()`);
const has = (text) => `document.body.innerText.includes(${J(text)})`;
const rowAction = (rowText, label) => evaluate(`(() => {
  const row = [...document.querySelectorAll('tr')].find((tr) => tr.innerText.includes(${J(rowText)}));
  if (!row) throw new Error('no row ' + ${J(rowText)});
  const b = [...row.querySelectorAll('button')].find((x) => x.textContent.trim() === ${J(label)});
  if (!b) throw new Error('no button ' + ${J(label)} + ' in row ' + ${J(rowText)}); b.click(); return true; })()`);
/** Types into the input inside a <label> whose caption <span> reads `caption`. */
const setLabeled = (caption, v) => evaluate(`(() => {
  const l = [...document.querySelectorAll('label')].find((x) => x.querySelector('span')?.textContent.trim() === ${J(caption)});
  if (!l) throw new Error('no field ' + ${J(caption)});
  __set(l.querySelector('input,textarea,select'), ${J(String(v))}); return true; })()`);
/** Sets a control inside the <form> whose text contains `formText`. */
const setInForm = (formText, sel, v, n = 0) => evaluate(`(() => {
  const f = [...document.querySelectorAll('form')].find((x) => x.innerText.includes(${J(formText)}));
  if (!f) throw new Error('no form with ' + ${J(formText)});
  const el = f.querySelectorAll(${J(sel)})[${n}]; if (!el) throw new Error('no ' + ${J(sel)} + ' in form ' + ${J(formText)});
  if (el instanceof HTMLSelectElement) __opt(el, ${J(String(v))}); else __set(el, ${J(String(v))}); return true; })()`);
const submitFormWith = (text) => evaluate(`(() => {
  const f = [...document.querySelectorAll('form')].find((x) => x.innerText.includes(${J(text)}));
  if (!f) throw new Error('no form with ' + ${J(text)}); f.requestSubmit(); return true; })()`);
const api = (method, path, body) => evaluate(`(async () => {
  const r = await fetch(${J(path)}, { method: ${J(method)}, headers: { 'content-type': 'application/json',
    authorization: 'Bearer ' + localStorage.getItem('accessToken') }, body: ${body ? J(J(body)) : 'undefined'} });
  const t = await r.text(); return { status: r.status, body: t ? JSON.parse(t) : null }; })()`);
async function shot(name) {
  const { data } = await send('Page.captureScreenshot', { format: 'png' });
  writeFileSync(join(SHOTS, `${name}.png`), Buffer.from(data, 'base64'));
}
/** Settings is split into tabs (Company, Invoices & documents, Closing, …): opens one by its label. */
async function settingsTab(label) {
  await go('/settings');
  await evaluate(`(() => {
    const el = [...document.querySelectorAll('button, a, [role=tab]')].find((b) => b.textContent.trim() === ${J(label)});
    if (el) el.click(); return true; })()`);
  await sleep(500);
}
let step = 0;
/** Resume from a phase: node demo-data.mjs <base> <shots> <phase number>. */
const START = Number(process.argv[4] ?? 1);
async function phase(name, fn) {
  step++;
  if (step < START) { console.log(`skip  ${String(step).padStart(2)} ${name}`); return; }
  const t = Date.now();
  try {
    await fn();
    console.log(`OK    ${String(step).padStart(2)} ${name} (${((Date.now() - t) / 1000).toFixed(0)} s)`);
  } catch (e) {
    console.log(`FAIL  ${String(step).padStart(2)} ${name}: ${e.message}`);
    await shot(`FAIL-${step}`).catch(() => {});
    throw e;
  }
}

// ── The company's data ───────────────────────────────────────────────────────
// Everything here is fictitious: Lone Star Office Supply Co., Austin, Texas, April – October 2026.
const COMPANY = {
  name: 'Lone Star Office Supply Co.',
  address: '1820 S Lamar Blvd, Suite 200\nAustin, TX 78704\nUnited States',
  email: 'accounts@lonestaroffice.example',
  phone: '+1 (512) 555-0147',
  website: 'www.lonestaroffice.example',
  taxNumber: 'EIN 74-3829156',
  payment: 'ACH or wire transfer to Lone Star Office Supply Co.\nBusiness checking account 4827 3910 26\nPlease quote the invoice number as the reference.',
  footer: 'Thank you for your business. Questions about this invoice? Call (512) 555-0147.',
};

const ACCOUNTS = [
  ['1010', 'Business Checking Account', 'ASSET', 'current_asset'],
  ['2400', 'Telephone Payable', 'LIABILITY', 'current_liability'],
  ['1510', 'Office Equipment & Furniture', 'ASSET', 'fixed_asset'],
  ['1520', 'Delivery Truck', 'ASSET', 'fixed_asset'],
  ['3100', 'Common Stock', 'EQUITY', 'equity'],
  ['4100', 'Delivery Income', 'REVENUE', 'operating_revenue'],
  ['6200', 'Rent – Office & Warehouse', 'EXPENSE', 'operating_expense'],
  ['6300', 'Utilities', 'EXPENSE', 'operating_expense'],
  ['6400', 'Fuel & Vehicle', 'EXPENSE', 'operating_expense'],
  ['6500', 'Telephone & Internet', 'EXPENSE', 'operating_expense'],
  ['6600', 'Office Supplies', 'EXPENSE', 'operating_expense'],
  ['6700', 'Bank Fees', 'EXPENSE', 'operating_expense'],
  ['6800', 'Advertising & Marketing', 'EXPENSE', 'operating_expense'],
  ['6900', 'Repairs & Maintenance', 'EXPENSE', 'operating_expense'],
];

const CUSTOMERS = [
  ['Hill Country Dental Group', 'ap@hillcountrydental.example', '+1 (512) 555-0110', '4400 N Lamar Blvd, Austin, TX 78756'],
  ['Bluebonnet Realty Partners', 'billing@bluebonnetrealty.example', '+1 (512) 555-0123', '600 Congress Ave, Suite 1400, Austin, TX 78701'],
  ['Pecan Street Law Group', 'office@pecanstreetlaw.example', '+1 (512) 555-0136', '310 E 6th St, Austin, TX 78701'],
  ['Barton Creek Architects', 'accounts@bartoncreekarch.example', '+1 (512) 555-0152', '2700 Bee Caves Rd, Austin, TX 78746'],
  ['Capitol Coffee House', 'hello@capitolcoffee.example', '+1 (512) 555-0168', '1105 Guadalupe St, Austin, TX 78701'],
  ['Cedar Park Pediatrics', 'admin@cedarparkpeds.example', '+1 (512) 555-0174', '1401 Medical Pkwy, Cedar Park, TX 78613'],
  ['Lakeway Coworking', 'manager@lakewaycowork.example', '+1 (512) 555-0189', '1310 Ranch Rd 620 S, Lakeway, TX 78734'],
  ['Round Rock Insurance Agency', 'orders@rrinsurance.example', '+1 (512) 555-0195', '201 E Main St, Round Rock, TX 78664'],
];

const VENDORS = [
  ['Southwest Furniture Wholesale', 'sales@swfurniture.example', '+1 (214) 555-0131', '8800 Ambassador Row, Dallas, TX 75247'],
  ['Gulf Coast Paper Distributors', 'orders@gulfcoastpaper.example', '+1 (713) 555-0144', '5600 Clinton Dr, Houston, TX 77020'],
  ['Texas Packaging Supply', 'billing@texaspackaging.example', '+1 (210) 555-0158', '3500 SE Military Dr, San Antonio, TX 78223'],
  ['Hill Country Freight Lines', 'accounts@hcfreight.example', '+1 (512) 555-0163', '9500 McNeil Dr, Austin, TX 78750'],
  ['Austin Business Equipment', 'sales@austinbizequip.example', '+1 (512) 555-0177', '7301 Burnet Rd, Austin, TX 78757'],
  ['Lamar Commercial Properties LLC', 'rent@lamarcommercial.example', '+1 (512) 555-0182', '1000 S Lamar Blvd, Austin, TX 78704'],
];

// Price list (USD, before sales tax).
const P = {
  paper: ['Copy paper – case of 10 reams', 52],
  toner: ['Toner cartridge – laser, high yield', 89],
  chair: ['Ergonomic mesh office chair', 289],
  cabinet: ['Steel filing cabinet – 4 drawer', 215],
  desk: ['Electric standing desk – 60 in', 445],
  board: ['Magnetic whiteboard – 4 × 6 ft', 165],
  delivery: ['Delivery & installation', 75],
};
/** Orders are 6× the list below (office fit-outs); delivery stays one charge per order. */
const L = (k, qty) => ({ desc: P[k][0], qty: k === 'delivery' ? qty : qty * 6, price: P[k][1], account: k === 'delivery' ? '4100' : '4000' });

// Invoices. pay: [date, 'full' | 'half']; credit: [date, amount, reason].
const INVOICES = [
  { key: 'I1', date: '2026-04-06', cust: 'Hill Country Dental', terms: 'NET_30', lines: [L('chair', 12), L('cabinet', 4), L('delivery', 1)], pay: ['2026-05-04', 'full'] },
  { key: 'I2', po: 'BRP-PO-2604', date: '2026-04-14', cust: 'Bluebonnet Realty', terms: 'NET_30', lines: [L('paper', 60), L('toner', 24), L('desk', 6)], pay: ['2026-05-12', 'full'] },
  { key: 'I3', date: '2026-04-22', cust: 'Pecan Street Law', terms: 'NET_15', lines: [L('cabinet', 10), L('board', 3), L('paper', 25), L('delivery', 1)], pay: ['2026-05-08', 'full'] },
  { key: 'I4', po: 'BCA-2026-118', date: '2026-05-05', cust: 'Barton Creek Architects', terms: 'NET_30', lines: [L('desk', 14), L('chair', 14), L('delivery', 1)], pay: ['2026-06-03', 'full'] },
  { key: 'I5', date: '2026-05-18', cust: 'Capitol Coffee', terms: 'DUE_ON_RECEIPT', lines: [L('paper', 10), L('toner', 6)], pay: ['2026-05-18', 'full'] },
  { key: 'I6', po: 'HCD-0527', date: '2026-05-27', cust: 'Hill Country Dental', terms: 'NET_30', lines: [L('paper', 40), L('toner', 18), L('delivery', 1)], pay: ['2026-06-25', 'full'] },
  { key: 'I7', po: 'BRP-PO-2607', date: '2026-06-09', cust: 'Bluebonnet Realty', terms: 'NET_30', lines: [L('chair', 20), L('desk', 8), L('board', 4)], pay: ['2026-07-08', 'full'] },
  { key: 'I8', date: '2026-06-21', cust: 'Cedar Park Pediatrics', terms: 'NET_15', lines: [L('paper', 35), L('cabinet', 6)], pay: ['2026-07-06', 'full'] },
  { key: 'I9', date: '2026-07-03', cust: 'Lakeway Coworking', terms: 'NET_30', lines: [L('desk', 16), L('chair', 24), L('board', 6)], pay: ['2026-08-01', 'full'] },
  { key: 'I10', date: '2026-07-16', cust: 'Pecan Street Law', terms: 'NET_30', lines: [L('paper', 45), L('toner', 20), L('delivery', 1)], pay: ['2026-08-14', 'full'] },
  { key: 'I11', po: 'BCA-2026-162', date: '2026-07-29', cust: 'Barton Creek Architects', terms: 'NET_60', lines: [L('cabinet', 12), L('board', 5), L('paper', 30)], pay: ['2026-09-25', 'full'] },
  { key: 'I12', date: '2026-08-11', cust: 'Round Rock Insurance', terms: 'NET_30', lines: [L('chair', 18), L('cabinet', 8), L('delivery', 1)], pay: ['2026-09-09', 'full'] },
  { key: 'I13', po: 'BRP-PO-2611', date: '2026-08-25', cust: 'Bluebonnet Realty', terms: 'NET_30', lines: [L('paper', 80), L('toner', 30)], pay: ['2026-09-30', 'half'] },
  { key: 'I14', po: 'HCD-0908', date: '2026-09-08', cust: 'Hill Country Dental', terms: 'NET_30', lines: [L('desk', 6), L('chair', 10), L('delivery', 1)], pay: ['2026-10-09', 'full'] },
  { key: 'I15', date: '2026-09-22', cust: 'Capitol Coffee', terms: 'NET_15', lines: [L('paper', 12), L('toner', 8)],
    credit: ['2026-09-25', 150, 'Credit – 2 toner cartridges damaged in delivery'], pay: ['2026-10-05', 'full'] },
  { key: 'I16', date: '2026-10-06', cust: 'Lakeway Coworking', terms: 'NET_30', lines: [L('chair', 12), L('board', 4)] },
  { key: 'I17', po: 'BRP-PO-2614', date: '2026-10-14', cust: 'Bluebonnet Realty', terms: 'NET_30', lines: [L('desk', 10), L('cabinet', 10), L('paper', 50)] },
  { key: 'I18', date: '2026-10-19', cust: 'Cedar Park Pediatrics', terms: 'NET_15', lines: [L('paper', 25), L('delivery', 1)] },
];

// Bills: [key, vendor, date, due, lines [desc, qty, price, account, tax?], pay date?]. Bought for resale: no sales tax.
const BILLS = [
  ['B1', 'Southwest Furniture', '2026-04-03', '2026-05-03', [['Ergonomic mesh office chairs', 30, 168, '5000'], ['Steel filing cabinets – 4 drawer', 15, 124, '5000']], '2026-04-30'],
  ['B2', 'Gulf Coast Paper', '2026-04-10', '2026-05-10', [['Copy paper – cases of 10 reams', 120, 31, '5000'], ['Laser toner cartridges', 50, 54, '5000']], '2026-05-08'],
  ['B3', 'Texas Packaging', '2026-04-15', '2026-05-15', [['Shipping cartons – double wall', 400, 1.85, '5000'], ['Stretch wrap & tape', 1, 640, '5000']], '2026-05-14'],
  ['B4', 'Southwest Furniture', '2026-05-12', '2026-06-11', [['Electric standing desks – 60 in', 20, 262, '5000'], ['Ergonomic mesh office chairs', 25, 168, '5000']], '2026-06-09'],
  ['B5', 'Gulf Coast Paper', '2026-06-08', '2026-07-08', [['Copy paper – cases of 10 reams', 150, 31, '5000'], ['Magnetic whiteboards – 4 × 6 ft', 12, 96, '5000']], '2026-07-06'],
  ['B6', 'Southwest Furniture', '2026-07-07', '2026-08-06', [['Electric standing desks – 60 in', 22, 262, '5000'], ['Ergonomic mesh office chairs', 30, 170, '5000']], '2026-08-04'],
  ['B7', 'Texas Packaging', '2026-08-04', '2026-09-03', [['Shipping cartons – double wall', 3000, 1.85, '5000']], '2026-09-02'],
  ['B8', 'Gulf Coast Paper', '2026-08-19', '2026-09-18', [['Copy paper – cases of 10 reams', 140, 32, '5000'], ['Laser toner cartridges', 45, 55, '5000']], '2026-09-16'],
  ['B9', 'Southwest Furniture', '2026-09-10', '2026-10-10', [['Steel filing cabinets – 4 drawer', 20, 126, '5000'], ['Electric standing desks – 60 in', 10, 265, '5000']], '2026-10-08'],
  ['B10', 'Gulf Coast Paper', '2026-10-05', '2026-11-04', [['Copy paper – cases of 10 reams', 160, 32, '5000']]],
  ['B11', 'Hill Country Freight', '2026-10-12', '2026-11-11', [['Freight – Q3 inbound shipments', 1, 2180, '6400']]],
];

// Expenses paid on the spot: [date, description, amount, account, paid from]
const MONTHS = ['April', 'May', 'June', 'July', 'August', 'September', 'October'];
const EXPENSES = [];
// Telephone and internet: accrued at each month end, paid from Telephone Payable the next month.
const TELEPHONE = [['2026-04-30', 389.13], ['2026-05-31', 381.4], ['2026-06-30', 402.75], ['2026-07-31', 395], ['2026-08-31', 410.6], ['2026-09-30', 398.25]];
const power = [612, 655, 710, 795, 830, 760, 690];
const fuel = [480, 515, 540, 575, 610, 590, 555];
MONTHS.forEach((m, i) => {
  const mm = String(i + 4).padStart(2, '0');
  EXPENSES.push([`2026-${mm}-01`, `Office & warehouse rent – ${m} 2026`, 6800, '6200', '1010']);
  EXPENSES.push([`2026-${mm}-12`, `Electricity & water – ${m}`, power[i], '6300', '1010']);
  EXPENSES.push([`2026-${mm}-20`, `Delivery truck fuel – ${m}`, fuel[i], '6400', '1010']);
});
EXPENSES.push(['2026-05-07', 'Printer ink, labels & folders', 245, '6600', '1000']);
EXPENSES.push(['2026-08-13', 'Office stationery', 180, '6600', '1000']);
EXPENSES.push(['2026-06-30', 'Bank fees – Q1', 35, '6700', '1010']);
EXPENSES.push(['2026-09-30', 'Bank fees – Q2', 42, '6700', '1010']);
EXPENSES.push(['2026-07-10', 'Online search ads – July campaign', 1500, '6800', '1010']);
EXPENSES.push(['2026-09-18', 'Delivery truck service & new tires', 860, '6900', '1010']);
EXPENSES.sort((a, b) => a[0].localeCompare(b[0]));

/** 500 cartons returned on B7: 500 × $1.85 (no sales tax on goods bought for resale). */
const DEBIT_NOTE = 925;

// US payroll (Texas: no state income tax), salaried, paid monthly. k401: pre-tax 401(k) per paycheck.
const EMPLOYEES = [
  { id: 'LS-001', name: 'Jake Morrison', title: 'Operations Manager', dept: 'Operations', email: 'jake@lonestaroffice.example', phone: '+1 (512) 555-0201', join: '2023-02-01',
    salary: 72000, filing: 'MARRIED_JOINTLY', dependents: 2000, k401: 250 },
  { id: 'LS-002', name: 'Emily Chen', title: 'Accountant', dept: 'Finance', email: 'emily@lonestaroffice.example', phone: '+1 (512) 555-0202', join: '2023-06-15',
    salary: 62000, filing: 'SINGLE', dependents: 0, k401: 200 },
  { id: 'LS-003', name: 'Marcus Johnson', title: 'Sales Representative', dept: 'Sales', email: 'marcus@lonestaroffice.example', phone: '+1 (512) 555-0203', join: '2024-01-08',
    salary: 48000, filing: 'SINGLE', dependents: 0, k401: 0 },
  { id: 'LS-004', name: 'Sofia Ramirez', title: 'Warehouse Supervisor', dept: 'Operations', email: 'sofia@lonestaroffice.example', phone: '+1 (512) 555-0204', join: '2024-05-02',
    salary: 42000, filing: 'HEAD_OF_HOUSEHOLD', dependents: 2000, k401: 100 },
  { id: 'LS-005', name: 'Tyler Brooks', title: 'Delivery Driver', dept: 'Logistics', email: '', phone: '+1 (512) 555-0205', join: '2025-03-10',
    salary: 38000, filing: 'SINGLE', dependents: 0, k401: 0 },
];

// ── Run ──────────────────────────────────────────────────────────────────────
const ids = {};        // account code → id
const docs = {};       // INVOICES/BILLS key → { id, number }
const iso = (d) => d;

try {
  await connect();
  await send('Page.enable');
  await send('Runtime.enable');
  await send('Network.enable');
  await send('DOM.enable');

  if (START > 1) {
    await send('Page.navigate', { url: BASE + '/login' });
    await waitFor(`!!document.querySelector('input[type=password]')`, 'login form');
    await evaluate(HELPERS);
    await set('input[type=text]', ADMIN.username);
    await set('input[type=password]', ADMIN.password);
    await evaluate(`document.querySelector('button[type=submit]').click()`);
    await waitFor(`location.pathname === '/' && !!localStorage.getItem('accessToken')`, 'signed in');
    for (const a of (await api('GET', '/api/accounts')).body) ids[a.code] = a.id;
  }

  await phase('Create the administrator on the first-run screen', async () => {
    await send('Page.navigate', { url: BASE + '/login' });
    await waitFor(`location.pathname === '/setup' && document.querySelectorAll('input[type=password]').length === 2`, 'first-run form');
    await evaluate(HELPERS);
    await set('input[type=text]', ADMIN.username);
    await set('input[type=password]', ADMIN.password, 0);
    await set('input[type=password]', ADMIN.password, 1);
    await evaluate(`document.querySelector('button[type=submit]').click()`);
    // D49: the recovery key screen comes first.
    await waitFor(`document.body.innerText.includes('Save your recovery key')`, 'recovery key');
    await evaluate(`document.querySelector('input[type=checkbox]').click()`);
    await clickText('Continue');
    await waitFor(`location.pathname === '/company-setup' && !!document.querySelector('form[aria-label="Company setup"]')`, 'company setup');
  });

  await phase('Company setup wizard: every page filled in', async () => {
    await evaluate(HELPERS);
    const page = (t) => waitFor(`document.querySelector('form[aria-label="Company setup"] h1')?.innerText.includes(${J(t)})`, t);
    const next = () => clickText('Next');
    await page('called'); await set('input[aria-label="Company name"]', COMPANY.name); await next();
    await page('address'); await set('textarea[aria-label="Address"]', COMPANY.address); await next();
    await page('email'); await set('input[aria-label="Email"]', COMPANY.email); await next();
    await page('phone'); await set('input[aria-label="Phone"]', COMPANY.phone); await next();
    await page('website'); await set('input[aria-label="Website"]', COMPANY.website); await next();
    await page('country'); await choose('select[aria-label="Country"]', 'US');
    await waitFor(`/USD|\\$/.test(document.querySelector('[data-testid=money-example]').innerText)`, 'United States preset');
    await choose('select[aria-label="Financial year starts in"]', '4');
    await next();
    await page('registered for tax');
    if (await evaluate(`!!document.querySelector('input[aria-label="Tax number"]')`)) await set('input[aria-label="Tax number"]', COMPANY.taxNumber);
    await next();
    await page('logo');
    const { result } = await send('Runtime.evaluate', { expression: `document.querySelector('input[type=file]')` });
    await send('DOM.setFileInputFiles', { objectId: result.objectId, files: [LOGO] });
    await waitFor(`!!document.querySelector('img[alt="Company logo"]')`, 'logo preview');
    await next();
    await page('How do customers pay'); await set('textarea[aria-label="How customers pay you"]', COMPANY.payment); await next();
    await page('bottom of your invoices'); await set('input[aria-label="Footer note"]', COMPANY.footer); await next();
    // D73: backups and opening balances are set up in their own phases.
    await page('Keep your books safe'); await clickText('Skip');
    await page('already have a business'); await clickText('Skip');
    await page('check your details');
    await shot('01-company-review');
    await clickText('Finish');
    await waitFor(`location.pathname === '/'`, 'dashboard');
  });

  await phase('Users: an accountant and a read-only owner', async () => {
    const existing = (await api('GET', '/api/users')).body ?? [];
    const names = new Set((Array.isArray(existing) ? existing : existing.items ?? []).map((x) => x.username));
    for (const u of USERS) {
      if (names.has(u.username)) continue; // made by an earlier, interrupted run
      await go('/users');
      await clickText('+ New User');
      await waitFor(`!!document.querySelector('input[placeholder=Username]')`, 'user form');
      await set('input[placeholder=Username]', u.username);
      await set('input[placeholder="Password (min 8 chars)"]', u.password);
      // The form asks for the password twice ("Type it again").
      if (await evaluate(`!!document.querySelector('input[placeholder="Confirm password"]')`)) {
        await set('input[placeholder="Confirm password"]', u.password);
      }
      await choose('form select', u.role);
      await clickText('Save');
      await waitFor(has(u.username), `user ${u.username}`);
    }
  });

  await phase('Settings: invoice signatory', async () => {
    await settingsTab('Invoices & documents');
    await waitFor(`!!document.querySelector('input[placeholder="e.g. Jane Smith"]')`, 'signatory form');
    await set('input[placeholder="e.g. Jane Smith"]', 'Jake Morrison');
    await set('input[placeholder="e.g. Director"]', 'Operations Manager');
    await evaluate(`document.querySelector('input[placeholder="e.g. Jane Smith"]').closest('form').requestSubmit()`);
    await waitFor(has('Settings saved'), 'saved');
  });

  await phase('Chart of accounts: bank, fixed assets, capital and expense accounts', async () => {
    // The default chart carries Sri Lanka's payroll accounts (EPF, ETF, PAYE, stamp duty, staff welfare,
    // staff loans): a US company has no use for them, so they go before anything is posted.
    const sl = ['1300', '2210', '2220', '2230', '2240', '2250', '2260', '6110', '6120'];
    for (const a of (await api('GET', '/api/accounts')).body.filter((x) => sl.includes(x.code))) {
      const r = await api('DELETE', `/api/accounts/${a.id}`);
      if (r.status >= 400) console.log(`      … could not remove ${a.code} ${a.name}: ${JSON.stringify(r.body)}`);
    }
    await go('/accounts');
    for (const [code, name, type, subtype, currency] of ACCOUNTS) {
      await clickText('+ New Account');
      await waitFor(`!!document.querySelector('input[placeholder="Code (e.g. 1000)"]')`, 'account form');
      await set('input[placeholder="Code (e.g. 1000)"]', code);
      await set('input[placeholder="Account Name"]', name);
      await choose('select[aria-label="Account type"]', type);
      await set('input[placeholder^="Subtype"]', subtype);
      if (currency) await choose('select[aria-label="Account currency"]', currency);
      await clickText('Save Account');
      await waitFor(has(name), `account ${code}`);
    }
  });

  await phase('Taxes: Texas sales tax 8.25% (state 6.25% + local 2%), the default on sales', async () => {
    await go('/taxes');
    await clickText('+ New Tax Code');
    await set('input[placeholder="e.g. VAT 18%"]', 'Sales Tax 8.25%');
    await set('input[placeholder="e.g. 18"]', '8.25');
    await clickText('Add Tax Payable and Tax Receivable');
    await waitFor(`!!document.querySelector('select[aria-label="Sales tax account"]')?.value`, 'sales tax account');
    await clickText('Save Tax Code');
    await waitFor(`[...document.querySelectorAll('tr')].some((tr) => tr.innerText.includes('Sales Tax 8.25%'))`, 'sales tax row');
    await choose('select[aria-label="Default sales tax"]', 'Sales Tax 8.25%');
    await sleep(800);
    for (const a of (await api('GET', '/api/accounts')).body) ids[a.code] = a.id;
  });

  await phase('Ledger: opening balances as a journal entry', async () => {
    await go('/ledger');
    await clickText('+ New Journal Entry');
    await waitFor(`!!document.querySelector('form[aria-label="Journal entry"]')`, 'journal form');
    await set('input[aria-label="Entry date"]', '2026-04-01');
    await set('input[placeholder^="Description"]', 'Opening balances – start of fiscal year 2026–27');
    await set('input[placeholder^="Reference"]', 'OB-2026');
    const lines = [['1010', 185000, 0, 'Balance per bank statement Mar 31, 2026'], ['1000', 500, 0, 'Petty cash'],
      ['1200', 42000, 0, 'Inventory count Mar 31, 2026'], ['1510', 38500, 0, 'Computers, racking and furniture'],
      ['1520', 52000, 0, 'Delivery truck – TX plate DLV-4471'], ['3100', 0, 318000, 'Common stock issued']];
    for (let i = 0; i < lines.length; i++) {
      if (i >= 2) { await clickText('+ Add line'); await sleep(150); }
      const [code, dr, cr, note] = lines[i];
      await choose(`select[aria-label="Account for line ${i + 1}"]`, code + ' ');
      await set(`input[aria-label="Note for line ${i + 1}"]`, note);
      if (dr) await set(`input[aria-label="Debit for line ${i + 1}"]`, dr);
      if (cr) await set(`input[aria-label="Credit for line ${i + 1}"]`, cr);
    }
    await waitFor(`document.querySelector('[data-testid=je-difference]').innerText.includes('Balanced')`, 'balanced');
    await shot('02-opening-entry');
    await clickText('Post Entry');
    await waitFor(has('OB-2026'), 'entry listed');
  });

  await phase(`Customers (${CUSTOMERS.length})`, async () => {
    await go('/customers');
    for (const [name, email, phone, address] of CUSTOMERS) {
      await clickText('+ New Customer');
      await set('input[placeholder="Full Name"]', name);
      await set('input[placeholder="Email"]', email);
      await set('input[placeholder="Phone"]', phone);
      await set('input[placeholder="Address"]', address);
      await clickText('Save');
      await waitFor(has(name), name);
    }
  });

  await phase(`Vendors (${VENDORS.length})`, async () => {
    await go('/vendors');
    for (const [name, email, phone, address] of VENDORS) {
      await clickText('+ New Vendor');
      await set('input[placeholder="Vendor Name"]', name);
      await set('input[placeholder="Email"]', email);
      await set('input[placeholder="Phone"]', phone);
      await set('input[placeholder="Address"]', address);
      await clickText('Save');
      await waitFor(has(name), name);
    }
  });

  // Documents are entered in date order, month by month, as a bookkeeper would.
  const events = [];
  for (const inv of INVOICES) {
    events.push({ date: inv.date, kind: 'invoice', inv });
    if (inv.credit) events.push({ date: inv.credit[0], kind: 'credit', inv });
    if (inv.pay) events.push({ date: inv.pay[0], kind: 'receipt', inv });
  }
  for (const b of BILLS) {
    events.push({ date: b[2], kind: 'bill', b });
    if (b[5]) events.push({ date: b[5], kind: 'billpay', b });
  }
  for (const e of EXPENSES) events.push({ date: e[0], kind: 'expense', e });
  events.push({ date: '2026-08-06', kind: 'debit' });
  TELEPHONE.forEach(([date, amount], i) => {
    const month = MONTHS[i];
    events.push({ date, kind: 'telaccrue', amount, month });
    const [y, m] = date.split('-').map(Number);
    events.push({ date: `${y}-${String(m + 1).padStart(2, '0')}-12`, kind: 'telpay', amount, month });
  });
  const order = { invoice: 0, bill: 1, expense: 2, telaccrue: 2, credit: 3, debit: 3, receipt: 4, billpay: 5, telpay: 5, usdtransfer: 6 };
  events.sort((a, b) => a.date.localeCompare(b.date) || order[a.kind] - order[b.kind]);

  /** Posts a journal entry through the Ledger form. lines: [code, debit, credit, note, rate?]. */
  async function journal(date, description, reference, lines) {
    await go('/ledger');
    await clickText('+ New Journal Entry');
    await waitFor(`!!document.querySelector('form[aria-label="Journal entry"]')`, 'journal form');
    await set('input[aria-label="Entry date"]', date);
    await set('input[placeholder^="Description"]', description);
    await set('input[placeholder^="Reference"]', reference);
    for (let i = 0; i < lines.length; i++) {
      if (i >= 2) { await clickText('+ Add line'); await sleep(150); }
      const [code, dr, cr, note, rate] = lines[i];
      await choose(`select[aria-label="Account for line ${i + 1}"]`, code + ' ');
      if (note) await set(`input[aria-label="Note for line ${i + 1}"]`, note);
      if (dr) await set(`input[aria-label="Debit for line ${i + 1}"]`, dr);
      if (cr) await set(`input[aria-label="Credit for line ${i + 1}"]`, cr);
      if (rate) {
        await waitFor(`!!document.querySelector('input[aria-label="Rate for line ${i + 1}"]')`, 'rate field');
        await set(`input[aria-label="Rate for line ${i + 1}"]`, rate);
      }
    }
    await waitFor(`document.querySelector('[data-testid=je-difference]').innerText.includes('Balanced')`, `${reference} balanced`);
    await clickText('Post Entry');
    await waitFor(has(reference), `${reference} listed`);
  }

  async function payPayable(date, code, amount, from, note) {
    await go('/payments');
    await clickText('+ Record Payment');
    await choose('form select', 'ACCOUNT', 0);
    await waitFor(`!!document.querySelector('select[aria-label="Payable account"]')`, 'payable picker');
    await choose('select[aria-label="Payable account"]', code + ' ');
    await set('input[placeholder="Amount"]', amount);
    await choose('form select', from + ' ', 3);
    await set('form input[type=date]', date);
    await clickText('Record Payment');
    await waitFor(`![...document.querySelectorAll('button')].some((b) => b.textContent.trim() === 'Record Payment' && b.closest('form'))`, note);
  }

  async function enterInvoice(inv) {
    await go('/invoices');
    await clickText('+ New Invoice');
    await waitFor(`!!document.querySelector('.inv-sheet') && !!document.querySelector('select[aria-label="Tax for line 1"]')`, 'invoice sheet');
    const aside = '.inv-side select';
    await choose(aside, inv.cust, 0);
    await waitFor(`!!document.querySelector('input[placeholder="Name as it should print"]').value`, 'bill-to filled');
    await set('.inv-sheet input[type=date]', inv.date, 0);
    await choose(aside, inv.terms, 1);
    if (inv.po) await set('.inv-sheet input[placeholder="optional"]', inv.po);
    if (inv.usd) {
      await choose('select[aria-label="Currency"]', 'USD');
      await waitFor(`!!document.querySelector('input[aria-label="Exchange rate"]')`, 'rate field');
      await set('input[aria-label="Exchange rate"]', inv.usd);
    }
    for (let i = 0; i < inv.lines.length; i++) {
      if (i > 0) {
        await clickText('+ Add line');
        await waitFor(`document.querySelectorAll('input[placeholder="Item description"]').length > ${i}`, 'new line');
      }
      const l = inv.lines[i];
      await set('input[placeholder="Item description"]', l.desc, i);
      await set('.inv-sheet tbody input[type=number]', l.qty, i * 2);
      await set('.inv-sheet tbody input[type=number]', l.price, i * 2 + 1);
      // Revenue account sits just before the line's tax picker in the side panel.
      await evaluate(`(() => { const tax = __q('select[aria-label="Tax for line ${i + 1}"]');
        const sels = [...document.querySelectorAll('.inv-side select')]; __opt(sels[sels.indexOf(tax) - 1], ${J(l.account + ' ')});
        ${inv.usd ? "__set(tax, '');" : ''} return true; })()`);
    }
    await sleep(300);
    await clickText('Save Invoice');
    await waitFor(`document.querySelector('h1')?.textContent === 'Invoices'`, 'back to the list');
    const list = (await api('GET', '/api/invoices?limit=100')).body.data;
    const made = list.filter((x) => x.status === 'DRAFT').sort((a, b) => b.createdAt?.localeCompare?.(a.createdAt) ?? 0)[0];
    docs[inv.key] = { id: made.id, number: made.invoiceNumber };
    // Post it from the list, as the guide says.
    await go('/invoices');
    await evaluate(`(() => { const row = [...document.querySelectorAll('tr')].find((tr) => tr.innerText.includes(${J(made.invoiceNumber)}));
      const sel = [...row.querySelectorAll('select')].find((x) => x.title.startsWith('Change the status')); __set(sel, 'POSTED'); return true; })()`);
    await waitFor(`(async () => (await fetch('/api/invoices/${made.id}', { headers: { authorization: 'Bearer ' + localStorage.getItem('accessToken') } }).then((x) => x.json())).status === 'POSTED')()`, `${made.invoiceNumber} posted`);
  }

  async function receive(inv) {
    const d = docs[inv.key];
    const cur = (await api('GET', `/api/invoices/${d.id}`)).body;
    const owed = Math.round((Number(cur.totalAmount) - Number(cur.amountPaid) - Number(cur.amountCredited ?? 0)) * 100) / 100;
    const [date, how] = inv.pay;
    await go('/payments');
    await clickText('+ Record Payment');
    await waitFor(`[...(document.querySelectorAll('form select')[1]?.options ?? [])].some((o) => o.textContent.includes(${J(d.number)}))`, 'invoice option');
    await choose('form select', d.number, 1);
    await choose('form select', inv.into ?? '1010', 3);
    await set('form input[type=date]', date);
    if (how === 'wht') {
      await choose('select[aria-label="Withholding tax"]', 'WHT 5%');
      await waitFor(`Number(document.querySelector('input[placeholder="Amount withheld"]')?.value) > 0`, 'withholding suggested');
    } else {
      const amount = how === 'half' ? Math.round(owed * 50) / 100 : owed;
      await set('input[placeholder="Amount"]', amount);
      if (typeof how === 'number') {
        await waitFor(`!!document.querySelector('input[aria-label="Payment exchange rate"]')`, 'rate field');
        await set('input[aria-label="Payment exchange rate"]', how);
      }
    }
    await set('form input[placeholder^="Reference"]', `${d.number} remittance`).catch(() => {});
    await clickText('Record Payment');
    await waitFor(`!document.querySelector('form') || ![...document.querySelectorAll('button')].some((b) => b.textContent.trim() === 'Record Payment' && b.closest('form'))`, 'payment saved');
  }

  async function enterBill(b) {
    const [key, vendor, date, due, lines] = b;
    await go('/bills');
    await clickText('+ New Bill');
    await waitFor(`[...(document.querySelectorAll('form select')[0]?.options ?? [])].some((o) => o.textContent.includes(${J(vendor)}))`, 'vendor option');
    await choose('form select', vendor, 0);
    await set('form input[type=date]', date, 0);
    await set('form input[type=date]', due, 1);
    for (let i = 0; i < lines.length; i++) {
      if (i > 0) {
        await clickText('+ Add line');
        await waitFor(`document.querySelectorAll('form input[placeholder="Item description"]').length > ${i}`, 'bill line');
      }
      const [desc, baseQty, price, acct, vat] = lines[i];
      const qty = acct === '5000' ? Math.round(baseQty * 5.4) : baseQty;
      await set('form input[placeholder="Item description"]', desc, i);
      await set('form input[type=number]', qty, i * 2);
      await set('form input[type=number]', price, i * 2 + 1);
      // The bill line now starts with a Product picker, so find the account picker by its label.
      await evaluate(`(() => { const tax = __q('select[aria-label="Tax for line ${i + 1}"]');
        const acct = document.querySelector('select[aria-label="Account for line ${i + 1}"]') ?? [...tax.parentElement.querySelectorAll('select')][0];
        __opt(acct, ${J(acct + ' ')}); ${vat ? `__opt(tax, 'VAT 18%');` : `__set(tax, '');`} return true; })()`);
    }
    await clickText('Save Bill');
    await waitFor(`!document.querySelector('form input[placeholder="Item description"]')`, 'bill saved');
    const made = (await api('GET', '/api/bills')).body;
    const rows = Array.isArray(made) ? made : made.data;
    const bill = rows.filter((x) => x.status === 'DRAFT')[0];
    docs[key] = { id: bill.id, number: bill.billNumber };
    await go('/bills');
    await rowAction(bill.billNumber, 'Post');
    await waitFor(`[...document.querySelectorAll('tr')].some((tr) => tr.innerText.includes(${J(bill.billNumber)}) && tr.innerText.includes('POSTED'))`, 'bill posted');
  }

  async function payBill(b) {
    const d = docs[b[0]];
    const cur = (await api('GET', `/api/bills/${d.id}`)).body;
    // The bill list carries no paid / credited amounts (the reference's shape), so subtract our own debit note.
    const owed = Math.round((Number(cur.totalAmount) - (b[0] === 'B7' ? DEBIT_NOTE : 0)) * 100) / 100;
    await go('/payments');
    await clickText('+ Record Payment');
    await choose('form select', 'AP', 0);
    await waitFor(`[...(document.querySelectorAll('form select')[1]?.options ?? [])].some((o) => o.textContent.includes(${J(d.number)}))`, 'bill option');
    await choose('form select', d.number, 1);
    await choose('form select', '1010', 3);
    await set('form input[type=date]', b[5]);
    await set('input[placeholder="Amount"]', owed);
    await clickText('Record Payment');
    await waitFor(`![...document.querySelectorAll('button')].some((b) => b.textContent.trim() === 'Record Payment' && b.closest('form'))`, 'bill payment saved');
  }

  async function enterExpense(e) {
    const [date, desc, amount, acct, from] = e;
    await go('/expenses');
    await clickText('+ New Expense');
    await set('form input[type=date]', date);
    await set('input[placeholder="Description"]', desc);
    await set('input[placeholder="Amount"]', amount);
    await choose('form select', acct + ' ', 0);
    await choose('form select', from + ' ', 1);
    await clickText('Save');
    await waitFor(`[...document.querySelectorAll('tr')].some((tr) => tr.innerText.includes(${J(desc)}))`, 'expense row');
    await rowAction(desc, 'Post');
    await waitFor(`[...document.querySelectorAll('tr')].some((tr) => tr.innerText.includes(${J(desc)}) && tr.innerText.includes('POSTED'))`, 'expense posted');
  }

  async function creditNote(inv) {
    const [date, amount, reason] = inv.credit;
    const d = docs[inv.key];
    await go('/credit-notes');
    await clickText('+ New Credit Note');
    await waitFor(`[...(document.querySelectorAll('form select')[0]?.options ?? [])].some((o) => o.textContent.includes(${J(d.number)}))`, 'invoice option');
    await choose('form select', d.number, 0);
    await set('form input[type=date]', date);
    await set('input[placeholder=Amount]', amount);
    await set('input[placeholder^="e.g. 10%"]', reason);
    await clickText('Save Note');
    await waitFor(`[...document.querySelectorAll('tr')].some((tr) => tr.innerText.includes(${J(d.number)}))`, 'credit note row');
  }

  async function debitNote() {
    const d = docs.B7;
    await go('/debit-notes');
    await clickText('+ New Debit Note');
    await waitFor(`[...(document.querySelectorAll('form select')[0]?.options ?? [])].some((o) => o.textContent.includes(${J(d.number)}))`, 'bill option');
    await choose('form select', d.number, 0);
    await set('form input[type=date]', '2026-08-06');
    await set('input[placeholder=Amount]', DEBIT_NOTE);
    await set('input[placeholder^="e.g. 10%"]', '500 cartons returned – crushed in transit');
    await clickText('Save Note');
    await waitFor(`[...document.querySelectorAll('tr')].some((tr) => tr.innerText.includes(${J(d.number)}))`, 'debit note row');
  }

  await phase(`Day-to-day: ${INVOICES.length} invoices, ${BILLS.length} bills, ${EXPENSES.length} expenses, payments and notes in date order`, async () => {
    let month = '';
    for (const ev of events) {
      if (ev.date.slice(0, 7) !== month) { month = ev.date.slice(0, 7); console.log(`      … ${month}`); }
      if (ev.kind === 'invoice') await enterInvoice(ev.inv);
      else if (ev.kind === 'receipt') await receive(ev.inv);
      else if (ev.kind === 'credit') await creditNote(ev.inv);
      else if (ev.kind === 'bill') await enterBill(ev.b);
      else if (ev.kind === 'billpay') await payBill(ev.b);
      else if (ev.kind === 'expense') await enterExpense(ev.e);
      else if (ev.kind === 'debit') await debitNote();
      else if (ev.kind === 'telaccrue') {
        await journal(ev.date, `Business phone & fiber internet – ${ev.month} 2026`, `TEL-${ev.date.slice(0, 7)}`,
          [['6500', ev.amount, 0, 'Bill received, payable next month'], ['2400', 0, ev.amount, 'Telecom provider']]);
      } else if (ev.kind === 'telpay') {
        await payPayable(ev.date, '2400', ev.amount, '1010', `telephone ${ev.month} paid`);
      } else if (ev.kind === 'usdtransfer') {
        const lkr = Math.round(ev.usd * ev.rate * 100) / 100;
        await journal(ev.date, `USD ${ev.usd.toLocaleString('en-US')} converted to rupees at ${ev.rate}`, ev.ref,
          [['1010', lkr, 0, 'Received in the LKR current account'], ['1020', 0, ev.usd, `USD ${ev.usd.toLocaleString('en-US')} sold to the bank`, ev.rate]]);
      }
    }
  });

  await phase(`Employees (${EMPLOYEES.length}) with US pay details (Form W-4)`, async () => {
    // US payroll settings first, so the profile shows the US pay section.
    // A payroll account whose default code is already used by another account (here 2300, which the
    // sales tax code took) gets an account of its own, as the settings page asks.
    const roles = (await api('GET', '/api/payroll/us/settings')).body.accounts ?? [];
    const all = (await api('GET', '/api/accounts')).body;
    const used = new Set(all.map((a) => a.code));
    const accounts = {};
    for (const r of roles) {
      const taken = all.find((a) => a.code === r.defaultCode);
      if (!taken || taken.name === r.defaultName) continue;
      let code = Number(r.defaultCode) + 5;
      while (used.has(String(code))) code++;
      used.add(String(code));
      const made = await api('POST', '/api/accounts', { code: String(code), name: r.defaultName, type: r.type,
        subtype: r.type === 'LIABILITY' ? 'current_liability' : 'operating_expense', currency: null });
      if (made.status >= 400) throw new Error(`account for ${r.label}: ` + JSON.stringify(made.body));
      accounts[r.role] = made.body.id;
      console.log(`      … ${r.label}: account ${code} (code ${r.defaultCode} is ${taken.name})`);
    }
    const settings = await api('PATCH', '/api/payroll/us/settings', {
      country: 'US', payFrequency: 'MONTHLY', state: 'TX', sutaRate: 2.7, sutaWageBase: 9000, futaCreditReduction: 0,
      ein: '74-3829156', stateAccountNo: '99-482716-3', accounts,
    });
    if (settings.status >= 400) throw new Error('US payroll settings: ' + JSON.stringify(settings.body));
    for (const e of EMPLOYEES) {
      const made = await api('POST', '/api/employees', {
        employeeId: e.id, name: e.name, jobTitle: e.title, department: e.dept, email: e.email || null, phone: e.phone,
        joinDate: e.join, status: 'ACTIVE', notes: null, employmentType: 'PERMANENT', epfNumber: null, nationalId: null, resignedDate: null,
      });
      if (made.status >= 400) throw new Error(`employee ${e.name}: ` + JSON.stringify(made.body));
      const tax = await api('PUT', `/api/payroll/employees/${made.body.id}/us-tax`, {
        workState: 'TX', payType: 'SALARY', annualSalary: e.salary, hourlyRate: 0, w4Version: '2020', filingStatus: e.filing,
        multipleJobs: false, dependentsCredit: e.dependents, otherIncome: 0, deductions: 0, extraWithholding: 0, allowances: 0,
        exemptFromWithholding: false, pretax401k: e.k401, pretaxSection125: 0,
        stateFilingStatus: 'SINGLE', stateAllowances: 0, stateExtraWithholding: 0, stateExempt: false,
      });
      if (tax.status >= 400) throw new Error(`W-4 for ${e.name}: ` + JSON.stringify(tax.body));
    }
  });

  await phase('US payroll April – October: post, pay, Form 941 deposits', async () => {
    const months = [['04', 30], ['05', 31], ['06', 30], ['07', 31], ['08', 31], ['09', 30], ['10', 31]];
    for (const [mm, last] of months) {
      const end = `2026-${mm}-${last}`;
      const run = await api('POST', '/api/payroll/us/runs', { periodStart: `2026-${mm}-01`, periodEnd: end, payDate: end });
      if (run.status >= 400) throw new Error(`pay run ${mm}: ` + JSON.stringify(run.body));
      const id = run.body.id;
      if (mm === '10') { console.log('      … October left as a draft to review'); continue; }
      const step = async (what, path, body) => {
        const r = await api('POST', `/api/payroll/us/runs/${id}/${path}`, body);
        if (r.status >= 400) throw new Error(`${what} ${mm}: ` + JSON.stringify(r.body));
      };
      await step('post', 'post');
      await step('pay', 'pay', { paymentAccountId: ids['1010'], date: end });
      // Form 941 deposit (monthly depositor): by the 15th of the next month.
      const next = String(Number(mm) + 1).padStart(2, '0');
      await step('941 deposit', 'remit', { type: 'FORM_941', paymentAccountId: ids['1010'], date: `2026-${next}-15`, reference: `EFTPS ${MONTHS[Number(mm) - 4]} 2026` });
    }
    await go('/payroll/us');
    await sleep(800);
    await shot('03-us-payroll');
  });

  await phase('Texas sales tax paid to the Comptroller for each quarter (journal entries)', async () => {
    for (const [qEnd, paid, label] of [['2026-06-30', '2026-07-20', 'Q2 2026 (Apr – Jun)'], ['2026-09-30', '2026-10-20', 'Q3 2026 (Jul – Sep)']]) {
      const entries = (await api('GET', '/api/ledger/entries')).body;
      const owed = -entries.filter((e) => e.date <= qEnd).flatMap((e) => e.lines).filter((l) => l.account?.code === '2300')
        .reduce((t, l) => t + Math.round(Number(l.debit) * 100) - Math.round(Number(l.credit) * 100), 0);
      if (!(owed > 0)) continue;
      await journal(paid, `Texas sales & use tax return ${label} – paid to the Comptroller`, `STX-${qEnd.slice(0, 7)}`,
        [['2300', (owed / 100).toFixed(2), 0, 'Sales tax collected for the quarter'], ['1010', 0, (owed / 100).toFixed(2), 'Paid online (Webfile)']]);
    }
  });

  await phase('Bank reconciliation: April – September against the bank statements', async () => {
    const entries = (await api('GET', '/api/ledger/entries')).body;
    const bankUpTo = (end) => entries.filter((e) => e.date <= end).flatMap((e) => e.lines)
      .filter((l) => l.accountId === ids['1010'] || l.account?.code === '1010')
      .reduce((s, l) => s + Math.round(Number(l.debit) * 100) - Math.round(Number(l.credit) * 100), 0) / 100;
    for (const end of ['2026-04-30', '2026-05-31', '2026-06-30', '2026-07-31', '2026-08-31', '2026-09-30']) {
      await go('/reconcile');
      await waitFor(has('Start a reconciliation'), 'start form');
      await choose('select', '1010');
      await sleep(400);
      await set('form input[type=date]', end);
      await set('input[placeholder="Closing balance"]', bankUpTo(end).toFixed(2));
      await clickText('Start');
      await waitFor(`!!document.querySelector('[data-testid=rec-difference]')`, 'reconciliation open');
      await sleep(500);
      await evaluate(`(() => { for (const el of document.querySelectorAll('input[type=checkbox][aria-label^="Clear "]')) {
        const date = el.closest('tr')?.innerText ?? ''; if (!el.checked) el.click(); } return true; })()`);
      await sleep(600);
      const diff = await evaluate(`document.querySelector('[data-testid=rec-difference]').innerText`);
      if (!/0\.00/.test(diff) || /[1-9]/.test(diff.replace(/0\.00/, ''))) throw new Error(`difference for ${end}: ${diff}`);
      if (end === '2026-09-30') await shot('04-reconcile-september');
      await clickText('Finish');
      await waitFor(`[...document.querySelectorAll('tr')].some((tr) => tr.innerText.includes('COMPLETED'))`, 'completed');
    }
  });

  await phase('Lock the books up to 30 June 2026 (first quarter closed)', async () => {
    await settingsTab('Closing');
    await waitFor(has('not locked'), 'lock card');
    await evaluate(`(() => { const card = [...document.querySelectorAll('input[type=date]')].pop(); __set(card, '2026-06-30'); return true; })()`);
    await clickText('Lock');
    await waitFor(`document.querySelector('[data-testid=lock-status]')?.innerText.includes('30 June 2026')`, 'locked');
  });

  await phase('Check: trial balance balances, screenshots of the main screens', async () => {
    const tb = (await api('GET', '/api/ledger/trial-balance')).body;
    const dr = tb.reduce((s, r) => s + Math.round(r.debit * 100), 0);
    const cr = tb.reduce((s, r) => s + Math.round(r.credit * 100), 0);
    if (dr !== cr) throw new Error(`trial balance off: ${dr} vs ${cr}`);
    for (const [path, name] of [['/', '10-dashboard'], ['/invoices', '11-invoices'], ['/bills', '12-bills'], ['/expenses', '13-expenses'],
      ['/payments', '14-payments'], ['/reports/profit-loss', '15-profit-loss'], ['/reports/balance-sheet', '16-balance-sheet'],
      ['/reports/ar-aging', '17-ar-aging'], ['/employees', '18-employees'], ['/payroll/us', '19-payroll'], ['/reports/tax', '20-tax-return'],
      ['/ledger/account/' + ids['2400'], '22-telephone-payable']]) {
      await go(path);
      await sleep(1200);
      await shot(name);
    }
  });

  console.log(`\nDone. API answers >= 400 seen: ${problems.length ? problems.join(', ') : 'none'}`);
} catch (e) {
  console.log('\nStopped: ' + e.message);
  if (problems.length) console.log('API answers >= 400: ' + problems.join(', '));
  process.exitCode = 1;
} finally {
  try { ws?.close(); } catch { /* closing */ }
  edge.kill();
}
