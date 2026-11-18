// App Store screenshots: signs in to a running TallyCheck server (normally the demo company,
// docs/demo/README.md) in a headless Chrome / Edge and captures the main screens at exactly
// 2880 x 1800 (1440 x 900 at 2x), the Mac App Store's largest screenshot size.
//
//   node tests/ui/appstore-shots.mjs http://127.0.0.1:47811 <output-dir> [username] [password]

import { spawn } from 'node:child_process';
import { existsSync, mkdirSync, mkdtempSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const BASE = process.argv[2] ?? 'http://127.0.0.1:47811';
const OUT = process.argv[3] ?? join(tmpdir(), 'appstore-shots');
const USER = process.argv[4] ?? 'admin';
const PASS = process.argv[5] ?? 'LoneStarDemo2026'; // the US demo's password (demo-data-us.mjs)
// SHOT_SIZE=1320x780 makes 2640 x 1560 shots for tools/appstore_frame.py (window on a desktop).
const [WIDTH, HEIGHT] = (process.env.SHOT_SIZE ?? '1440x900').split('x').map(Number);
const SCALE = 2;
const PORT = 9335;
mkdirSync(OUT, { recursive: true });

const SCREENS = [
  ['/', '1-dashboard'],
  ['/invoices', '2-invoices'],
  ['/reports/profit-loss', '3-profit-and-loss'],
  ['/reports/balance-sheet', '4-balance-sheet'],
  ['/payroll/us', '5-payroll'],
  ['/ledger', '6-ledger'],
  ['/reports/ar-aging', '7-ar-aging'],
  ['/employees', '8-employees'],
];

const BROWSER = process.env.DEMO_BROWSER ?? [
  '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',
  '/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge',
  '/Applications/Chromium.app/Contents/MacOS/Chromium',
  'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe',
].find((p) => existsSync(p));
if (!BROWSER) throw new Error('No Chrome or Edge found; set DEMO_BROWSER to its path.');

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const profile = mkdtempSync(join(tmpdir(), 'shots-'));
const browser = spawn(BROWSER, ['--headless=new', `--remote-debugging-port=${PORT}`, `--user-data-dir=${profile}`,
  '--no-first-run', '--disable-extensions', '--hide-scrollbars', `--window-size=${WIDTH},${HEIGHT}`, 'about:blank'],
  { stdio: 'ignore' });

let ws;
let nextId = 1;
const pending = new Map();
async function connect() {
  for (let i = 0; i < 50; i++) {
    try {
      const page = (await (await fetch(`http://127.0.0.1:${PORT}/json`)).json()).find((t) => t.type === 'page');
      if (page) {
        ws = new WebSocket(page.webSocketDebuggerUrl);
        await new Promise((res, rej) => { ws.onopen = res; ws.onerror = rej; });
        ws.onmessage = (ev) => {
          const m = JSON.parse(ev.data);
          if (m.id && pending.has(m.id)) {
            const { ok, no } = pending.get(m.id);
            pending.delete(m.id);
            m.error ? no(new Error(m.error.message)) : ok(m.result);
          }
        };
        return;
      }
    } catch { /* browser still starting */ }
    await sleep(200);
  }
  throw new Error('could not connect to the browser');
}
const send = (method, params = {}) => new Promise((ok, no) => {
  const id = nextId++;
  pending.set(id, { ok, no });
  ws.send(JSON.stringify({ id, method, params }));
});
const evaluate = async (expression) => {
  const r = await send('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true });
  if (r.exceptionDetails) throw new Error(r.exceptionDetails.text);
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
const setValue = (sel, v) => evaluate(`(() => { const el = document.querySelector(${J(sel)});
  Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set.call(el, ${J(v)});
  el.dispatchEvent(new Event('input', { bubbles: true })); return true; })()`);

try {
  await connect();
  await send('Page.enable');
  await send('Runtime.enable');
  await send('Emulation.setDeviceMetricsOverride', { width: WIDTH, height: HEIGHT, deviceScaleFactor: SCALE, mobile: false });

  await send('Page.navigate', { url: BASE + '/login' });
  await waitFor(`!!document.querySelector('input[type=password]')`, 'sign-in page');
  await sleep(500);
  await setValue('input[type=text]', USER);
  await setValue('input[type=password]', PASS);
  await evaluate(`document.querySelector('button[type=submit]').click()`);
  await waitFor(`location.pathname === '/' && !!localStorage.getItem('accessToken')`, 'signed in');

  for (const [path, name] of SCREENS) {
    await send('Page.navigate', { url: BASE + path });
    await waitFor(`document.readyState === 'complete' && location.pathname === ${J(path)}`, `load ${path}`);
    await sleep(2000); // charts and tables finish loading
    const { data } = await send('Page.captureScreenshot', { format: 'png', captureBeyondViewport: false });
    writeFileSync(join(OUT, `${name}.png`), Buffer.from(data, 'base64'));
    console.log(`saved ${name}.png`);
  }
  console.log(`\nDone: ${SCREENS.length} screenshots, ${WIDTH * SCALE} x ${HEIGHT * SCALE}, in ${OUT}`);
} catch (e) {
  console.log('Stopped: ' + e.message);
  try {
    const where = await evaluate(`location.pathname + ' | ' + document.body.innerText.replace(/\\s+/g, ' ').slice(0, 300)`);
    console.log('Page: ' + where);
    const { data } = await send('Page.captureScreenshot', { format: 'png' });
    writeFileSync(join(OUT, 'FAILED.png'), Buffer.from(data, 'base64'));
    console.log('Saved what the page showed: ' + join(OUT, 'FAILED.png'));
  } catch { /* browser gone */ }
  process.exitCode = 1;
} finally {
  try { ws?.close(); } catch { /* closing */ }
  browser.kill();
}
