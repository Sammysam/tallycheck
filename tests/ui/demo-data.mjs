// Demo company: types a realistic, made-up business into the app THROUGH THE SCREENS (the same
// forms a person uses), into an empty data folder. Reads back through the API only to learn the
// numbers the app gave to new documents.
//
//   node tests/ui/demo-data.mjs http://127.0.0.1:47811 <screenshot-dir>
//
// The server must point at an EMPTY data folder (see docs/demo/README.md). Everything here is
// fictitious: Lotus Spice & Tea Traders (Pvt) Ltd, Colombo, April – October 2026.

import { spawn } from 'node:child_process';
import { mkdirSync, writeFileSync, mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';

const BASE = process.argv[2] ?? 'http://127.0.0.1:47811';
const SHOTS = process.argv[3] ?? join(tmpdir(), 'accounting-demo');
mkdirSync(SHOTS, { recursive: true });
const LOGO = resolve('docs/demo/lotus-logo.png');
const EDGE = 'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe';
const PORT = 9334;
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

// Sign-in details for the demo copy only; also listed in docs/demo/README.md.
const ADMIN = { username: 'admin', password: 'LotusDemo2026' };
const USERS = [
  { username: 'dilini.accounts', password: 'LotusAccounts26', role: 'Accountant' },
  { username: 'owner.view', password: 'LotusViewer26', role: 'Viewer' },
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
const COMPANY = {
  name: 'Lotus Spice & Tea Traders (Pvt) Ltd',
  address: 'No. 48, Sir Chittampalam A. Gardiner Mawatha\nColombo 02\nSri Lanka',
  email: 'accounts@lotusspicetea.lk',
  phone: '+94 11 245 7788',
  website: 'www.lotusspicetea.lk',
  taxNumber: 'VAT 114587236-7000',
  payment: 'Bank transfer to Lotus Spice & Tea Traders (Pvt) Ltd\nCommercial Bank – Union Place branch\nAccount no. 1100 4587 2361\nPlease quote the invoice number as the reference.',
  footer: 'Thank you for your business. Goods once sold are not returnable unless damaged on delivery.',
};

const ACCOUNTS = [
  ['1010', 'Commercial Bank – Current Account', 'ASSET', 'current_asset'],
  ['1510', 'Office Equipment & Furniture', 'ASSET', 'fixed_asset'],
  ['1520', 'Delivery Van', 'ASSET', 'fixed_asset'],
  ['3100', 'Share Capital', 'EQUITY', 'equity'],
  ['4100', 'Delivery & Handling Income', 'REVENUE', 'operating_revenue'],
  ['6200', 'Rent – Office & Warehouse', 'EXPENSE', 'operating_expense'],
  ['6300', 'Electricity & Water', 'EXPENSE', 'operating_expense'],
  ['6400', 'Fuel & Transport', 'EXPENSE', 'operating_expense'],
  ['6500', 'Telephone & Internet', 'EXPENSE', 'operating_expense'],
  ['6600', 'Office Supplies & Stationery', 'EXPENSE', 'operating_expense'],
  ['6700', 'Bank Charges', 'EXPENSE', 'operating_expense'],
  ['6800', 'Advertising & Marketing', 'EXPENSE', 'operating_expense'],
  ['6900', 'Repairs & Maintenance', 'EXPENSE', 'operating_expense'],
];

const CUSTOMERS = [
  ['Hillside Hotels (Pvt) Ltd', 'purchasing@hillsidehotels.lk', '+94 81 222 4410', '12 Peradeniya Road, Kandy'],
  ['Colombo City Supermarkets (Pvt) Ltd', 'ap@colombocitysuper.lk', '+94 11 250 6600', '210 Galle Road, Colombo 04'],
  ['Blue Lagoon Resort', 'stores@bluelagoonresort.lk', '+94 34 227 5120', 'Beach Road, Bentota'],
  ['Green Leaf Café', 'hello@greenleafcafe.lk', '+94 77 431 2290', '5 Ward Place, Colombo 07'],
  ['Island Wellness Spa', 'admin@islandwellness.lk', '+94 91 224 8830', '33 Lighthouse Street, Galle Fort'],
  ['Harbour View Restaurant', 'manager@harbourview.lk', '+94 31 222 7714', '2 Lewis Place, Negombo'],
  ['Sunrise Catering Services', 'orders@sunrisecatering.lk', '+94 11 282 9045', '87 High Level Road, Nugegoda'],
  ['Pacific Gourmet Imports LLC', 'payables@pacificgourmet.us', '+1 415 555 0182', '1450 Mission Street, San Francisco, CA 94103, USA'],
];

const VENDORS = [
  ['Uva Highlands Tea Estates', 'sales@uvahighlands.lk', '+94 57 222 3150', 'Badulla Road, Bandarawela'],
  ['Matale Spice Growers Co-operative', 'coop@matalespice.lk', '+94 66 223 1408', 'Kandy Road, Matale'],
  ['Ceylon Packaging Solutions (Pvt) Ltd', 'billing@ceylonpack.lk', '+94 11 291 5530', 'Industrial Zone, Biyagama'],
  ['Rapid Freight Lanka (Pvt) Ltd', 'accounts@rapidfreight.lk', '+94 11 243 8800', '15 Port Access Road, Colombo 15'],
  ['Lanka Office Mart', 'sales@lankaofficemart.lk', '+94 11 269 4471', '400 Union Place, Colombo 02'],
  ['Prime Properties (Pvt) Ltd', 'rent@primeproperties.lk', '+94 11 230 1188', '7 Alfred House Gardens, Colombo 03'],
];

// Price list (LKR, before VAT).
const P = {
  tea: ['Ceylon Black Tea BOPF – 1 kg pack', 2450],
  green: ['Ceylon Green Tea – 100 tea bags', 1250],
  cinnamon: ['Cinnamon Alba Sticks – 500 g', 3900],
  pepper: ['Black Pepper Whole – 1 kg', 3200],
  cardamom: ['Green Cardamom – 250 g', 2850],
  cloves: ['Cloves – 500 g', 2100],
  delivery: ['Delivery & handling', 3500],
};
/** Order sizes are 2.5× the list below (wholesale cases); delivery stays one charge per order. */
const L = (k, qty) => ({ desc: P[k][0], qty: k === 'delivery' ? qty : Math.round(qty * 2.5), price: P[k][1], account: k === 'delivery' ? '4100' : '4000' });
const USDL = (desc, qty, price) => ({ desc, qty: qty * 2, price, account: '4000' });

// Invoices. pay: [date, 'full' | 'half' | 'wht' | rate for USD]; credit: [date, amount, reason].
const INVOICES = [
  { key: 'I1', date: '2026-04-06', cust: 'Hillside Hotels', terms: 'NET_30', lines: [L('tea', 40), L('cinnamon', 10), L('delivery', 1)], pay: ['2026-05-04', 'full'] },
  { key: 'I2', po: 'CCS-PO-26041', date: '2026-04-14', cust: 'Colombo City Supermarkets', terms: 'NET_30', lines: [L('tea', 120), L('green', 80), L('pepper', 30)], pay: ['2026-05-12', 'full'] },
  { key: 'I3', date: '2026-04-22', cust: 'Blue Lagoon Resort', terms: 'NET_15', lines: [L('cardamom', 12), L('cloves', 10), L('tea', 25), L('delivery', 1)], pay: ['2026-05-08', 'full'] },
  { key: 'I4', po: 'PGI-2026-118', date: '2026-05-05', cust: 'Pacific Gourmet Imports', terms: 'NET_30', usd: 298,
    lines: [USDL('Ceylon Cinnamon Alba sticks – export carton (10 kg)', 20, 145), USDL('Black Pepper whole – export sack (25 kg)', 6, 280)], pay: ['2026-06-10', 305] },
  { key: 'I5', date: '2026-05-18', cust: 'Green Leaf Café', terms: 'DUE_ON_RECEIPT', lines: [L('tea', 15), L('green', 20)], pay: ['2026-05-18', 'full'] },
  { key: 'I6', po: 'HH/PUR/0527', date: '2026-05-27', cust: 'Hillside Hotels', terms: 'NET_30', lines: [L('tea', 50), L('pepper', 12), L('delivery', 1)], pay: ['2026-06-25', 'full'] },
  { key: 'I7', po: 'CCS-PO-26077', date: '2026-06-09', cust: 'Colombo City Supermarkets', terms: 'NET_30', lines: [L('tea', 150), L('cinnamon', 40), L('cloves', 25)], pay: ['2026-07-08', 'full'] },
  { key: 'I8', date: '2026-06-21', cust: 'Island Wellness Spa', terms: 'NET_15', lines: [L('green', 60), L('cardamom', 8)], pay: ['2026-07-06', 'wht'] },
  { key: 'I9', date: '2026-07-03', cust: 'Harbour View Restaurant', terms: 'NET_30', lines: [L('pepper', 20), L('cardamom', 10), L('cloves', 12)], pay: ['2026-08-01', 'full'] },
  { key: 'I10', date: '2026-07-16', cust: 'Blue Lagoon Resort', terms: 'NET_30', lines: [L('tea', 40), L('cinnamon', 15), L('delivery', 1)], pay: ['2026-08-14', 'full'] },
  { key: 'I11', po: 'PGI-2026-162', date: '2026-07-29', cust: 'Pacific Gourmet Imports', terms: 'NET_60', usd: 302,
    lines: [USDL('Ceylon Cinnamon Alba sticks – export carton (10 kg)', 25, 145), USDL('Green Cardamom – export carton (5 kg)', 8, 98)], pay: ['2026-10-02', 299.5] },
  { key: 'I12', date: '2026-08-11', cust: 'Sunrise Catering Services', terms: 'NET_30', lines: [L('pepper', 25), L('cloves', 20), L('tea', 30)], pay: ['2026-09-09', 'full'] },
  { key: 'I13', po: 'CCS-PO-26112', date: '2026-08-25', cust: 'Colombo City Supermarkets', terms: 'NET_30', lines: [L('tea', 180), L('green', 100)], pay: ['2026-09-30', 'half'] },
  { key: 'I14', po: 'HH/PUR/0908', date: '2026-09-08', cust: 'Hillside Hotels', terms: 'NET_30', lines: [L('tea', 60), L('cinnamon', 20), L('cardamom', 10), L('delivery', 1)], pay: ['2026-10-09', 'full'] },
  { key: 'I15', date: '2026-09-22', cust: 'Green Leaf Café', terms: 'NET_15', lines: [L('tea', 20), L('green', 25)],
    credit: ['2026-09-25', 5000, 'Discount – 4 packs of green tea damaged in delivery'], pay: ['2026-10-05', 'full'] },
  { key: 'I16', date: '2026-10-06', cust: 'Harbour View Restaurant', terms: 'NET_30', lines: [L('pepper', 15), L('cloves', 10)] },
  { key: 'I17', po: 'CCS-PO-26139', date: '2026-10-14', cust: 'Colombo City Supermarkets', terms: 'NET_30', lines: [L('tea', 160), L('pepper', 40), L('cinnamon', 30)] },
  { key: 'I18', date: '2026-10-19', cust: 'Island Wellness Spa', terms: 'NET_15', lines: [L('green', 40), L('delivery', 1)] },
];

// Bills: [key, vendor, date, due, lines [desc, qty, price, account, vat?], pay date?]
const BILLS = [
  ['B1', 'Uva Highlands', '2026-04-03', '2026-05-03', [['Black tea BOPF – bulk (kg)', 300, 1350, '5000', true]], '2026-04-30'],
  ['B2', 'Matale Spice', '2026-04-10', '2026-05-10', [['Cinnamon Alba sticks (500 g packs)', 100, 2300, '5000'], ['Black pepper whole (kg)', 80, 1900, '5000'], ['Green cardamom (250 g packs)', 40, 1650, '5000']], '2026-05-08'],
  ['B3', 'Ceylon Packaging', '2026-04-15', '2026-05-15', [['Printed tea pouches – 1 kg', 5000, 18, '5000', true], ['Shipping cartons', 400, 95, '5000', true]], '2026-05-14'],
  ['B4', 'Uva Highlands', '2026-05-12', '2026-06-11', [['Black tea BOPF – bulk (kg)', 350, 1350, '5000', true], ['Green tea – bulk (kg)', 60, 1500, '5000', true]], '2026-06-09'],
  ['B5', 'Matale Spice', '2026-06-08', '2026-07-08', [['Cinnamon Alba sticks (500 g packs)', 150, 2300, '5000'], ['Cloves (500 g packs)', 60, 1200, '5000']], '2026-07-06'],
  ['B6', 'Uva Highlands', '2026-07-07', '2026-08-06', [['Black tea BOPF – bulk (kg)', 400, 1380, '5000', true]], '2026-08-04'],
  ['B7', 'Ceylon Packaging', '2026-08-04', '2026-09-03', [['Printed tea pouches – 1 kg', 6000, 18, '5000', true]], '2026-09-02'],
  ['B8', 'Matale Spice', '2026-08-19', '2026-09-18', [['Black pepper whole (kg)', 120, 1950, '5000'], ['Green cardamom (250 g packs)', 50, 1700, '5000']], '2026-09-16'],
  ['B9', 'Uva Highlands', '2026-09-10', '2026-10-10', [['Black tea BOPF – bulk (kg)', 420, 1400, '5000', true]], '2026-10-08'],
  ['B10', 'Matale Spice', '2026-10-05', '2026-11-04', [['Cinnamon Alba sticks (500 g packs)', 160, 2350, '5000']]],
  ['B11', 'Rapid Freight', '2026-10-12', '2026-11-11', [['Export freight & port handling – Q3 shipments', 1, 68500, '6400', true]]],
];

// Expenses paid on the spot: [date, description, amount, account, paid from]
const MONTHS = ['April', 'May', 'June', 'July', 'August', 'September', 'October'];
const EXPENSES = [];
const power = [18400, 19650, 21200, 22800, 24900, 23100, 21750];
const fuel = [32500, 34800, 36250, 38900, 41200, 39650, 37400];
MONTHS.forEach((m, i) => {
  const mm = String(i + 4).padStart(2, '0');
  EXPENSES.push([`2026-${mm}-01`, `Office & warehouse rent – ${m} 2026`, 175000, '6200', '1010']);
  EXPENSES.push([`2026-${mm}-12`, `CEB electricity & water – ${m}`, power[i], '6300', '1010']);
  EXPENSES.push([`2026-${mm}-15`, `SLT fibre broadband & phones – ${m}`, 12500, '6500', '1010']);
  EXPENSES.push([`2026-${mm}-20`, `Delivery van fuel – ${m}`, fuel[i], '6400', '1010']);
});
EXPENSES.push(['2026-05-07', 'Printer toner, paper & files', 8750, '6600', '1000']);
EXPENSES.push(['2026-08-13', 'Stationery & invoice books', 6200, '6600', '1000']);
EXPENSES.push(['2026-06-30', 'Bank charges – Q1', 1850, '6700', '1010']);
EXPENSES.push(['2026-09-30', 'Bank charges – Q2', 2150, '6700', '1010']);
EXPENSES.push(['2026-07-10', 'Facebook & Instagram ads – July campaign', 45000, '6800', '1010']);
EXPENSES.push(['2026-09-18', 'Delivery van service & new tyres', 27500, '6900', '1010']);
EXPENSES.sort((a, b) => a[0].localeCompare(b[0]));

/** 500 misprinted pouches returned on B7: 500 × 18 + 18% VAT. */
const DEBIT_NOTE = 10620;

const EMPLOYEES = [
  { id: 'LS-001', name: 'Kasun Jayasinghe', title: 'Operations Manager', dept: 'Operations', email: 'kasun@lotusspicetea.lk', phone: '+94 77 512 3401', join: '2023-02-01', nic: '198812345678',
    epf: '101', basic: 185000, bra2005: 1000, bra2016: 2500, vehicle: 25000, bank: ['Commercial Bank', 'Union Place', '8001 2345 6701'] },
  { id: 'LS-002', name: 'Dilini Fernando', title: 'Accountant', dept: 'Finance', email: 'dilini@lotusspicetea.lk', phone: '+94 71 388 2290', join: '2023-06-15', nic: '199245678912',
    epf: '102', basic: 145000, bra2005: 1000, bra2016: 2500, bank: ['Sampath Bank', 'Borella', '1029 5566 3412'] },
  { id: 'LS-003', name: 'Ruwan Kumara', title: 'Sales Executive', dept: 'Sales', email: 'ruwan@lotusspicetea.lk', phone: '+94 76 904 1188', join: '2024-01-08', nic: '199534561234',
    epf: '103', basic: 95000, bra2005: 1000, bra2016: 2500, vehicle: 15000, bank: ['Hatton National Bank', 'Nugegoda', '0072 1100 4581'] },
  { id: 'LS-004', name: 'Shanika Perera', title: 'Warehouse Supervisor', dept: 'Operations', email: 'shanika@lotusspicetea.lk', phone: '+94 75 220 6743', join: '2024-05-02', nic: '199687654321',
    epf: '104', basic: 78000, bra2005: 1000, bra2016: 2500, welfare: 300, bank: ['Bank of Ceylon', 'Pettah', '7788 1234'] },
  { id: 'LS-005', name: 'Nuwan Silva', title: 'Delivery Driver', dept: 'Logistics', email: '', phone: '+94 78 663 5120', join: '2025-03-10', nic: '199912349876',
    epf: '105', basic: 58000, bra2005: 1000, bra2016: 2500, welfare: 300, loan: 60000, instalment: 5000, bank: ['People\u2019s Bank', 'Maradana', '1340 0011 9876'] },
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
    await page('country'); await choose('select[aria-label="Country"]', 'LK');
    await waitFor(`document.querySelector('[data-testid=money-example]').innerText.includes('LKR')`, 'Sri Lanka preset');
    await choose('select[aria-label="Financial year starts in"]', '4');
    await next();
    await page('registered for tax'); await set('input[aria-label="Tax number"]', COMPANY.taxNumber); await next();
    await page('logo');
    const { result } = await send('Runtime.evaluate', { expression: `document.querySelector('input[type=file]')` });
    await send('DOM.setFileInputFiles', { objectId: result.objectId, files: [LOGO] });
    await waitFor(`!!document.querySelector('img[alt="Company logo"]')`, 'logo preview');
    await next();
    await page('How do customers pay'); await set('textarea[aria-label="How customers pay you"]', COMPANY.payment); await next();
    await page('bottom of your invoices'); await set('input[aria-label="Footer note"]', COMPANY.footer); await next();
    await page('check your details');
    await shot('01-company-review');
    await clickText('Finish');
    await waitFor(`location.pathname === '/'`, 'dashboard');
  });

  await phase('Users: an accountant and a read-only owner', async () => {
    for (const u of USERS) {
      await go('/users');
      await clickText('+ New User');
      await waitFor(`!!document.querySelector('input[placeholder=Username]')`, 'user form');
      await set('input[placeholder=Username]', u.username);
      await set('input[placeholder="Password (min 8 chars)"]', u.password);
      await choose('form select', u.role);
      await clickText('Save');
      await waitFor(has(u.username), `user ${u.username}`);
    }
  });

  await phase('Settings: invoice signatory', async () => {
    await go('/settings');
    await waitFor(`!!document.querySelector('input[placeholder="e.g. Jane Smith"]')`, 'signatory form');
    await set('input[placeholder="e.g. Jane Smith"]', 'Kasun Jayasinghe');
    await set('input[placeholder="e.g. Director"]', 'Operations Manager');
    await evaluate(`document.querySelector('input[placeholder="e.g. Jane Smith"]').closest('form').requestSubmit()`);
    await waitFor(has('Settings saved'), 'saved');
  });

  await phase('Chart of accounts: bank, fixed assets, capital and expense accounts', async () => {
    await go('/accounts');
    for (const [code, name, type, subtype] of ACCOUNTS) {
      await clickText('+ New Account');
      await waitFor(`!!document.querySelector('input[placeholder="Code (e.g. 1000)"]')`, 'account form');
      await set('input[placeholder="Code (e.g. 1000)"]', code);
      await set('input[placeholder="Account Name"]', name);
      await choose('form select', type);
      await set('input[placeholder^="Subtype"]', subtype);
      await clickText('Save Account');
      await waitFor(has(name), `account ${code}`);
    }
  });

  await phase('Taxes: VAT 18% and WHT 5%, VAT as the default on sales', async () => {
    await go('/taxes');
    await clickText('+ New Tax Code');
    await set('input[placeholder="e.g. VAT 18%"]', 'VAT 18%');
    await set('input[placeholder="e.g. 18"]', '18');
    await clickText('Add Tax Payable and Tax Receivable');
    await waitFor(`!!document.querySelector('select[aria-label="Sales tax account"]')?.value && !!document.querySelector('select[aria-label="Purchase tax account"]')?.value`, 'VAT accounts');
    await clickText('Save Tax Code');
    await waitFor(`[...document.querySelectorAll('tr')].some((tr) => tr.innerText.includes('VAT 18%'))`, 'VAT row');
    await clickText('+ New Tax Code');
    await set('input[placeholder="e.g. VAT 18%"]', 'WHT 5%');
    await choose('select[aria-label="Kind"]', 'WITHHOLDING');
    await set('input[placeholder="e.g. 18"]', '5');
    await clickText('Add Withholding Tax Receivable and Payable');
    await waitFor(`!!document.querySelector('select[aria-label="Sales tax account"]')?.value && !!document.querySelector('select[aria-label="Purchase tax account"]')?.value`, 'WHT accounts');
    await clickText('Save Tax Code');
    await waitFor(`[...document.querySelectorAll('tr')].some((tr) => tr.innerText.includes('WHT 5%'))`, 'WHT row');
    await choose('select[aria-label="Default sales tax"]', 'VAT 18%');
    await sleep(800);
    for (const a of (await api('GET', '/api/accounts')).body) ids[a.code] = a.id;
  });

  await phase('Ledger: opening balances as a journal entry', async () => {
    await go('/ledger');
    await clickText('+ New Journal Entry');
    await waitFor(`!!document.querySelector('form[aria-label="Journal entry"]')`, 'journal form');
    await set('input[aria-label="Entry date"]', '2026-04-01');
    await set('input[placeholder^="Description"]', 'Opening balances – start of the 2026/27 financial year');
    await set('input[placeholder^="Reference"]', 'OB-2026');
    const lines = [['1010', 2850000, 0, 'Balance per bank statement 31 Mar 2026'], ['1000', 50000, 0, 'Petty cash float'],
      ['1200', 640000, 0, 'Stock count 31 Mar 2026'], ['1510', 685000, 0, 'Computers, racks and furniture'],
      ['1520', 2400000, 0, 'Delivery van – WP CAB-4471'], ['1300', 60000, 0, 'Staff loan – N. Silva (paid out Feb 2026)'],
      ['3100', 0, 6685000, 'Issued share capital']];
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
  const order = { invoice: 0, bill: 1, expense: 2, credit: 3, debit: 3, receipt: 4, billpay: 5 };
  events.sort((a, b) => a.date.localeCompare(b.date) || order[a.kind] - order[b.kind]);

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
    await choose('form select', '1010', 3);
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
      const qty = acct === '5000' ? baseQty * 2 : baseQty;
      await set('form input[placeholder="Item description"]', desc, i);
      await set('form input[type=number]', qty, i * 2);
      await set('form input[type=number]', price, i * 2 + 1);
      await evaluate(`(() => { const tax = __q('select[aria-label="Tax for line ${i + 1}"]'); const row = tax.parentElement;
        const acct = [...row.querySelectorAll('select')][0]; __opt(acct, ${J(acct + ' ')}); ${vat ? `__opt(tax, 'VAT 18%');` : `__set(tax, '');`} return true; })()`);
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
    await set('input[placeholder^="e.g. 10%"]', '500 printed pouches returned – misprinted batch (incl. VAT)');
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
    }
  });

  await phase(`Employees (${EMPLOYEES.length}) with salary and bank details`, async () => {
    for (const e of EMPLOYEES) {
      await go('/employees');
      await clickText('+ New Employee');
      await set('input[placeholder="e.g. CEN-042"]', e.id);
      await set('input[placeholder="Full name"]', e.name);
      await set('input[placeholder="Job title"]', e.title);
      await set('input[placeholder="Department"]', e.dept);
      if (e.email) await set('form input[placeholder="Email"]', e.email);
      await set('form input[placeholder="Phone"]', e.phone);
      await setLabeled('Join date', e.join);
      await set('input[placeholder="e.g. 96"]', e.epf);
      await set('input[placeholder="NIC number"]', e.nic);
      await clickText('Save & open profile');
      await waitFor(`/^\\/employees\\/[0-9a-f-]{36}$/.test(location.pathname)`, 'profile');
      await sleep(600);
      await evaluate(HELPERS);
      await clickText('+ Set salary');
      await waitFor(`[...document.querySelectorAll('form')].some((f) => f.innerText.includes('Effective from'))`, 'salary form');
      await setInForm('Effective from', 'input[type=date]', '2026-04-01');
      await setLabeled('Basic *', e.basic);
      await setLabeled('BRA 2005', e.bra2005);
      await setLabeled('BRA 2016', e.bra2016);
      if (e.vehicle) await setLabeled('Vehicle allowance', e.vehicle);
      if (e.welfare) await setLabeled('Welfare deduction / month', e.welfare);
      if (e.instalment) await setLabeled('Loan instalment / month', e.instalment);
      await submitFormWith('Effective from');
      await waitFor(has('+ Add increment'), 'salary saved');
      await clickText('Bank details');
      await waitFor(`[...document.querySelectorAll('form')].some((f) => f.innerText.includes('Account number'))`, 'bank form');
      await setLabeled('Bank', e.bank[0]);
      await setLabeled('Branch', e.bank[1]);
      await setLabeled('Account number', e.bank[2]);
      if (e.loan) await setLabeled('Staff loan balance (Rs.)', e.loan);
      await submitFormWith('Account number');
      await waitFor(has(e.bank[0]), 'bank saved');
    }
  });

  await phase('Payroll April – October: post, pay salaries, remit EPF / ETF / PAYE', async () => {
    const months = ['04', '05', '06', '07', '08', '09', '10'];
    const lastDay = { '04': 30, '05': 29, '06': 30, '07': 31, '08': 28, '09': 30, '10': 23 };
    for (const mm of months) {
      await go('/payroll');
      await setInForm('Month', 'input[type=month]', `2026-${mm}`);
      await waitFor(has('Start payroll for'), 'start button');
      await submitFormWith('Start payroll for');
      await waitFor(`/^\\/payroll\\/[0-9a-f-]{36}$/.test(location.pathname)`, 'run page');
      const runId = await evaluate('location.pathname.split("/").pop()');
      await sleep(600);
      await evaluate(HELPERS);
      if (mm === '10') { console.log('      … October left as a draft to review'); continue; }
      await clickText('Post to ledger');
      await waitFor(`[...document.querySelectorAll('form')].some((f) => f.innerText.includes('Record payment of'))`, 'posted');
      await setInForm('Record payment of', 'select', '1010');
      await setInForm('Record payment of', 'input[type=date]', `2026-${mm}-${lastDay[mm]}`);
      await submitFormWith('Record payment of');
      await waitFor(`![...document.querySelectorAll('form')].some((f) => f.innerText.includes('Record payment of'))`, 'salaries paid');
      // Statutory payments are made the following month; September's are still to pay.
      if (mm === '09') continue;
      const next = String(Number(mm) + 1).padStart(2, '0');
      const run = (await api('GET', '/api/payroll/runs/' + runId)).body;
      for (const [fund, v] of Object.entries(run.owed ?? {})) {
        if (!(v.outstanding > 0)) continue;
        await setInForm('Record remittance', 'select', fund, 0);
        await setInForm('Record remittance', 'select', '1010', 1);
        await setInForm('Record remittance', 'input[type=date]', `2026-${next}-14`);
        await setInForm('Record remittance', 'input[placeholder^="Reference"]', `${fund} ${MONTHS[Number(mm) - 4]} 2026`).catch(() => {});
        const before = (await api('GET', '/api/payroll/runs/' + runId)).body.remittances.length;
        await submitFormWith('Record remittance');
        await waitFor(`(async () => (await fetch('/api/payroll/runs/${runId}', { headers: { authorization: 'Bearer ' + localStorage.getItem('accessToken') } }).then((x) => x.json())).remittances.length > ${before})()`, `${fund} remitted`);
      }
      if (mm === '08') await shot('03-payroll-august');
    }
  });

  await phase('VAT paid to the Inland Revenue for each quarter (journal entries)', async () => {
    for (const [qEnd, paid, label] of [['2026-06-30', '2026-07-20', 'Q1 (Apr – Jun 2026)'], ['2026-09-30', '2026-10-20', 'Q2 (Jul – Sep 2026)']]) {
      const entries = (await api('GET', '/api/ledger/entries')).body;
      const bal = (code) => entries.filter((e) => e.date <= qEnd).flatMap((e) => e.lines).filter((l) => l.account?.code === code)
        .reduce((t, l) => t + Math.round(Number(l.debit) * 100) - Math.round(Number(l.credit) * 100), 0);
      const payable = -bal('2300');  // VAT charged on sales, not yet settled
      const reclaim = bal('1400');   // VAT paid on purchases, not yet set off
      await go('/ledger');
      await clickText('+ New Journal Entry');
      await waitFor(`!!document.querySelector('form[aria-label="Journal entry"]')`, 'journal form');
      await set('input[aria-label="Entry date"]', paid);
      await set('input[placeholder^="Description"]', `VAT return ${label} – paid to Inland Revenue`);
      await set('input[placeholder^="Reference"]', `VAT-${qEnd.slice(0, 7)}`);
      await clickText('+ Add line'); await sleep(150);
      const rows = [['2300', payable, 0, 'Output VAT for the quarter'], ['1400', 0, reclaim, 'Input VAT set off'], ['1010', 0, payable - reclaim, 'Paid by bank transfer']];
      for (let i = 0; i < rows.length; i++) {
        const [code, dr, cr, note] = rows[i];
        await choose(`select[aria-label="Account for line ${i + 1}"]`, code + ' ');
        await set(`input[aria-label="Note for line ${i + 1}"]`, note);
        if (dr) await set(`input[aria-label="Debit for line ${i + 1}"]`, (dr / 100).toFixed(2));
        if (cr) await set(`input[aria-label="Credit for line ${i + 1}"]`, (cr / 100).toFixed(2));
      }
      await waitFor(`document.querySelector('[data-testid=je-difference]').innerText.includes('Balanced')`, 'VAT entry balanced');
      await clickText('Post Entry');
      await waitFor(has(`VAT-${qEnd.slice(0, 7)}`), 'VAT entry listed');
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
    await go('/settings');
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
      ['/reports/ar-aging', '17-ar-aging'], ['/employees', '18-employees'], ['/payroll', '19-payroll'], ['/reports/tax', '20-tax-return']]) {
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
