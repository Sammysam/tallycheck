// D63 UI test: US payroll in several states in headless Microsoft Edge (Chrome DevTools Protocol, no npm
// dependencies) against a running accounting-server.
//
//   node tests/ui/us-states.mjs http://127.0.0.1:47800 <screenshot-dir>
//
// Start the server on a fresh, empty data folder with ADMIN_USERNAME=admin and
// ADMIN_PASSWORD=admin12345.

import { spawn } from 'node:child_process';
import { mkdirSync, writeFileSync, mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const BASE = process.argv[2] ?? 'http://127.0.0.1:47800';
const SHOTS = process.argv[3] ?? join(tmpdir(), 'accounting-ui-us-states');
mkdirSync(SHOTS, { recursive: true });
const EDGE = 'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe';
const PORT = 9337;
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

// ── Browser plumbing ──────────────────────────────────────────────────────
const profile = mkdtempSync(join(tmpdir(), 'edge-us-states-'));
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

let employeeId; let runId; let cashId;
try {
  await connect();
  await send('Page.enable');
  await send('Runtime.enable');
  await send('Network.enable');

  await check('sign in; a US company with example state rules', async () => {
    await go('/login');
    await type('input[type=text]', 'admin');
    await type('input[type=password]', 'admin12345');
    await evaluate(`document.querySelector('button[type=submit]').click()`);
    await waitFor(`location.pathname !== '/login'`, 'signed in');
    const ok = (r) => { if (r.status >= 300) throw new Error(JSON.stringify(r)); return r; };
    ok(await api('PATCH', '/settings', { 'company.name': 'Sample Trading', 'region.country': 'US', 'region.currency': 'USD', 'region.locale': 'en-US' }));
    ok(await api('POST', '/accounts/seed'));
    ok(await api('PATCH', '/payroll/us/settings', { country: 'US', state: 'TX', sutaRate: 2.7, sutaWageBase: 9000, payFrequency: 'BIWEEKLY' }));
    for (const [st, rate] of [['NJ', 5], ['PA', 3.07]]) {
      ok(await api('PUT', `/payroll/us/state-taxes/${st}`, { method: 'FLAT', flatRate: rate, notes: 'Example rule for tests' }));
    }
    const e = ok(await api('POST', '/employees', { employeeId: 'US-NJ', name: 'Pat Example' }));
    employeeId = e.body.id;
    ok(await api('PUT', `/payroll/employees/${employeeId}/us-tax`, { workState: 'NJ', payType: 'SALARY', annualSalary: 52000 }));
    const accounts = (await api('GET', '/accounts')).body;
    cashId = accounts.find((a) => a.code === '1000').id;
  });

  await check('register NJ and add the NJ / PA reciprocity agreement', async () => {
    await go('/payroll/us');
    await waitFor(`[...document.querySelectorAll('button')].some((b) => b.textContent.trim() === 'Payroll settings' || b.textContent.trim() === 'Hide settings')`, 'page');
    if (await evaluate(`[...document.querySelectorAll('button')].some((b) => b.textContent.trim() === 'Payroll settings')`)) await clickText('Payroll settings');
    await waitFor(`document.body.innerText.includes('States you are registered in')`, 'states panel');
    await waitFor(`document.body.innerText.includes('which is not registered here yet') || document.body.innerText.includes('which are not registered here yet')`, 'unregistered hint');
    await setValue('select[aria-label="Register a state"]', 'NJ');
    await clickText('Add');
    await waitFor(`!!document.querySelector('form[aria-label="NJ registration"]')`, 'NJ form');
    // D68: a new registration starts from the state's 2026 figures.
    await waitFor(`document.querySelector('input[aria-label="SUTA rate"]').value === '2.8' && document.querySelector('input[aria-label="SUTA wage base"]').value === '44800' && document.body.innerText.includes('2026 for NJ: wage base')`, 'NJ defaults filled in');
    await setValue('input[aria-label="SUTA rate"]', '3');
    await setValue('input[aria-label="SUTA wage base"]', '43300');
    await setValue('input[aria-label="Withholding account no."]', 'NJ-1');
    // D67: New Jersey's disability and family leave programs, with the employer TDI rate.
    await waitFor(`document.body.innerText.includes('Disability and paid leave in NJ') && document.body.innerText.includes('New Jersey FLI')`, 'NJ leave programs');
    await setValue('input[aria-label="NJ_TDI rate"]', '0.8');
    await clickText('Save NJ');
    await waitFor(`document.body.innerText.includes('SUTA 3% on the first')`, 'NJ saved');
    await setValue('select[aria-label="Home state"]', 'PA');
    await setValue('select[aria-label="Work state"]', 'NJ');
    await clickText('Add agreement');
    await waitFor(`document.body.innerText.includes('pays PA only')`, 'agreement listed');
    await shot('01-states');
  });

  await check('the employee lives in PA: the preview taxes PA only and unemployment goes to NJ', async () => {
    await go(`/employees/${employeeId}`);
    await waitFor(`!!document.querySelector('select[aria-label="Lives in state"]')`, 'US tax section');
    await setValue('select[aria-label="Lives in state"]', 'PA');
    await clickText('Save US pay & W-4');
    await waitFor(`document.body.innerText.includes('Saved. New and refreshed pay run lines')`, 'saved');
    await clickText('Work it out');
    await waitFor(`document.body.innerText.includes('State income tax PA (home state)')`, 'PA line in preview');
    const t = await pageText();
    if (!t.includes('State unemployment (NJ)')) throw new Error('unemployment state not shown');
    if (t.includes('State income tax NJ')) throw new Error('NJ taxed despite reciprocity');
    await shot('02-employee');
  });

  await check('a pay run shows the PA tax and pays it to PA', async () => {
    const r = await api('POST', '/payroll/us/runs', { periodStart: '2026-10-19', periodEnd: '2026-11-01', payDate: '2026-11-06' });
    if (r.status !== 201) throw new Error(JSON.stringify(r));
    runId = r.body.id;
    const p = await api('POST', `/payroll/us/runs/${runId}/post`);
    if (p.status !== 201) throw new Error(JSON.stringify(p));
    await go(`/payroll/us/${runId}`);
    await waitFor(`document.body.innerText.includes('PA 61.40')`, 'per-state tax under the total');
    await waitFor(`document.body.innerText.includes('New Jersey TDI') && document.body.innerText.includes('employer 16.00')`, 'NJ TDI with the employer rate on the line');
    await waitFor(`document.body.innerText.includes('State income tax — PA')`, 'taxes table per state');
    await evaluate(`(() => { const s = [...document.querySelectorAll('select')].find((x) => [...x.options].some((o) => o.value === 'STATE_INCOME_TAX'));
      Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, 'value').set.call(s, 'STATE_INCOME_TAX'); s.dispatchEvent(new Event('change', { bubbles: true })); return true; })()`);
    await waitFor(`!!document.querySelector('select[aria-label="State for this payment"]')`, 'state choice');
    await evaluate(`(() => { const form = [...document.querySelectorAll('form')].find((f) => f.innerText.includes('Record payment')); const s = [...form.querySelectorAll('select')].find((x) => [...x.options].some((o) => o.value === ${JSON.stringify(cashId)}));
      Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, 'value').set.call(s, ${JSON.stringify(cashId)}); s.dispatchEvent(new Event('change', { bubbles: true })); return true; })()`);
    await clickText('Record payment');
    await waitFor(`document.body.innerText.includes('State income tax — PA') && document.body.innerText.toLowerCase().includes('tax payments recorded')`, 'payment recorded');
    const run = (await api('GET', `/payroll/us/runs/${runId}`)).body;
    const pa = run.taxes.find((t) => t.type === 'STATE_INCOME_TAX' && t.state === 'PA');
    if (!pa || pa.outstanding !== 0) throw new Error(JSON.stringify(run.taxes));
    if (!run.remittances.some((x) => x.state === 'PA')) throw new Error('remittance without state');
    await shot('03-run');
  });
  await check('built-in 2026 rules show their source; a company rule replaces one', async () => {
    await go('/payroll/us');
    if (await evaluate(`[...document.querySelectorAll('button')].some((b) => b.textContent.trim() === 'Payroll settings')`)) await clickText('Payroll settings');
    await waitFor(`document.body.innerText.includes('Kentucky 2026 Withholding Tax Formula')`, 'KY built in with its source');
    const t = await pageText();
    if (!t.includes('NC-30')) throw new Error('NC source missing');
    if (!t.includes('Your own rule — replaces the built-in one')) throw new Error('PA override not marked');
    await go(`/employees/${employeeId}`);
    await waitFor(`!!document.querySelector('select[aria-label="Lives in state"]')`, 'US tax section');
    await evaluate(`(() => { const s = [...document.querySelectorAll('select')].find((x) => x.required && [...x.options].some((o) => o.value === 'IL'));
      Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, 'value').set.call(s, 'IL'); s.dispatchEvent(new Event('change', { bubbles: true })); return true; })()`);
    await waitFor(`document.body.innerText.includes('IL-W-4 line 2 allowances')`, 'Illinois form labels');
    // D65: West Virginia picks its table by the IT-104 one-earner box, not filing status.
    await evaluate(`(() => { const s = [...document.querySelectorAll('select')].find((x) => x.required && [...x.options].some((o) => o.value === 'WV'));
      Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, 'value').set.call(s, 'WV'); s.dispatchEvent(new Event('change', { bubbles: true })); return true; })()`);
    await waitFor(`[...document.querySelectorAll('select[aria-label="State filing status"] option')].some((o) => o.textContent.includes('One-earner table'))`, 'West Virginia table choice');
    // D66: Connecticut asks for the CT-W4 withholding code.
    await evaluate(`(() => { const s = [...document.querySelectorAll('select')].find((x) => x.required && [...x.options].some((o) => o.value === 'CT'));
      Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, 'value').set.call(s, 'CT'); s.dispatchEvent(new Event('change', { bubbles: true })); return true; })()`);
    await waitFor(`[...document.querySelectorAll('select[aria-label="State form code"] option')].some((o) => o.textContent.includes('no withholding'))`, 'Connecticut withholding codes');
    await shot('04-built-in');
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
