// Side-by-side verification of the importer: the reference API on its PostgreSQL data and this
// app on the data imported from that same database must answer every read the same way.
//
//   node tests/contract/side_by_side.mjs http://127.0.0.1:3900 http://127.0.0.1:3902 <state.json>
//
// Unlike run.mjs, ids and timestamps are NOT normalised: the import keeps primary keys and
// converts timestamps, so they must match exactly. Only tokens, QR codes and the server-side
// "now" of fresh logins are excluded.

import { createHash, createHmac } from 'node:crypto';
import { readFileSync } from 'node:fs';

const [REF, NEW, STATE] = process.argv.slice(2);
const state = JSON.parse(readFileSync(STATE, 'utf8'));

function base32Decode(s) {
  const alphabet = 'ABCDEFGHIJKLMNOPQRSTUVWXYZ234567';
  let bits = '';
  for (const c of s.replace(/=+$/, '').toUpperCase()) bits += alphabet.indexOf(c).toString(2).padStart(5, '0');
  const bytes = [];
  for (let i = 0; i + 8 <= bits.length; i += 8) bytes.push(parseInt(bits.slice(i, i + 8), 2));
  return Buffer.from(bytes);
}
function totp(secret) {
  const buf = Buffer.alloc(8);
  buf.writeBigUInt64BE(BigInt(Math.floor(Date.now() / 30000)));
  const h = createHmac('sha1', base32Decode(secret)).update(buf).digest();
  const o = h[h.length - 1] & 0xf;
  return String((h.readUInt32BE(o) & 0x7fffffff) % 1_000_000).padStart(6, '0');
}

async function call(base, path, token, method = 'GET', body) {
  const res = await fetch(base + path, {
    method,
    headers: { ...(token ? { authorization: `Bearer ${token}` } : {}), ...(body ? { 'content-type': 'application/json' } : {}) },
    body: body ? JSON.stringify(body) : undefined,
  });
  const bytes = Buffer.from(await res.arrayBuffer());
  const type = res.headers.get('content-type') ?? '';
  if (type.includes('json')) return { status: res.status, body: bytes.length ? JSON.parse(bytes.toString()) : null };
  return {
    status: res.status,
    body: {
      contentType: type,
      contentDisposition: res.headers.get('content-disposition'),
      size: bytes.length,
      sha256: createHash('sha256').update(bytes).digest('hex'),
    },
  };
}

// The imported admin keeps the reference password hash (bcrypt) and 2FA secret.
async function login(base) {
  const r = await call(base, '/api/auth/login', null, 'POST', { username: 'admin', password: 'admin12345' });
  if (!r.body?.tempToken) throw new Error(`login failed on ${base}: ${JSON.stringify(r.body)}`);
  const v = await call(base, '/api/auth/2fa/verify', r.body.tempToken, 'POST', { code: totp(state.adminSecret) });
  if (!v.body?.accessToken) throw new Error(`2FA failed on ${base}: ${JSON.stringify(v.body)}`);
  return v.body.accessToken;
}

const JWT = /^[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+$/;
function clean(v, key) {
  if (Array.isArray(v)) return v.map((x) => clean(x));
  if (v && typeof v === 'object') return Object.fromEntries(Object.keys(v).sort().map((k) => [k, clean(v[k], k)]));
  if (typeof v === 'string' && JWT.test(v)) return '<jwt>';
  return v;
}
// PostgreSQL returns lines in no fixed order (the reference has no ORDER BY).
function sortLines(v) {
  if (Array.isArray(v)) return v.map(sortLines);
  if (!v || typeof v !== 'object') return v;
  const out = Object.fromEntries(Object.entries(v).map(([k, x]) => [k, sortLines(x)]));
  for (const k of ['lines', 'items']) {
    if (Array.isArray(out[k])) out[k] = [...out[k]].sort((x, y) => (JSON.stringify(x) < JSON.stringify(y) ? -1 : 1));
  }
  return out;
}
// Each server records its own fresh login; the audit trail is compared on mutations only.
function mutationRows(body) {
  if (!body || !Array.isArray(body.data)) return body;
  const data = body.data.filter((e) => e.path !== '/api/auth/login');
  return { ...body, data, total: data.length };
}

// Signing in with the imported bcrypt password upgrades it to Argon2id (CLAUDE.md §7), which
// touches that one user's updatedAt on this side only. Only that field of that user is ignored.
function signedInUser(body) {
  const strip = (u) => (u && typeof u === 'object' && u.username === 'admin' ? { ...u, updatedAt: '<signed in>' } : u);
  if (Array.isArray(body)) return body.map(strip);
  return strip(body);
}

// Approved deviations that are visible on historical data.
const EXPECTED = [
  [/^\/api\/ledger\/trial-balance/, 'D1: reversed entries and their reversals both count'],
  [/^\/api\/ledger\/balance\//, 'D1: reversed entries and their reversals both count'],
  [/^\/api\/reports\/(profit-loss|balance-sheet)/, 'D1: reports come from all ledger lines'],
  [/^\/api\/reports\/ap-aging/, 'D13: AP ageing uses the accumulated amount paid'],
];

const [rt, nt] = await Promise.all([login(REF), login(NEW)]);
const list = async (path) => (await call(REF, path, rt)).body;
const paths = new Set([
  '/api/auth/me', '/api/users', '/api/roles', '/api/roles/permissions', '/api/settings', '/api/audit-logs/users',
  '/api/accounts', '/api/ledger/entries', '/api/ledger/trial-balance', '/api/customers', '/api/vendors', '/api/banking',
  '/api/bills', '/api/expenses', '/api/payments', '/api/employees', '/api/payroll/runs', '/api/payroll/rates',
  '/api/payroll/company', '/api/reports/profit-loss', '/api/reports/balance-sheet', '/api/reports/ar-aging',
  '/api/reports/ap-aging', '/api/reports/profit-loss?dateFrom=2000-01-01&dateTo=2100-12-31',
]);
const ids = (rows) => (Array.isArray(rows) ? rows.map((r) => r.id) : []);
for (const id of ids(await list('/api/accounts'))) { paths.add(`/api/accounts/${id}`); paths.add(`/api/ledger/balance/${id}`); }
for (const id of ids(await list('/api/ledger/entries'))) paths.add(`/api/ledger/entries/${id}`);
for (const r of ['customers', 'vendors', 'banking', 'bills', 'expenses', 'payments']) {
  for (const id of ids(await list(`/api/${r}`))) paths.add(`/api/${r}/${id}`);
}
for (let page = 1; ; page++) {
  const p = await list(`/api/invoices?limit=100&page=${page}`);
  paths.add(`/api/invoices?limit=100&page=${page}`);
  for (const id of ids(p.data)) paths.add(`/api/invoices/${id}`);
  if (page >= p.totalPages) break;
}
for (let page = 1; ; page++) {
  const p = await list(`/api/audit-logs?limit=100&page=${page}`);
  paths.add(`/api/audit-logs?limit=100&page=${page}`);
  if (page >= p.totalPages) break;
}
for (const e of await list('/api/employees')) {
  paths.add(`/api/employees/${e.id}`);
  paths.add(`/api/employees/${e.id}/documents`);
  paths.add(`/api/payroll/employees/${e.id}`);
  if (e.photoKey) paths.add(`/api/employees/${e.id}/photo`);
  for (const d of await list(`/api/employees/${e.id}/documents`)) {
    paths.add(`/api/employees/${e.id}/documents/${d.id}/download`);
    paths.add(`/api/employees/${e.id}/documents/${d.id}/download?inline=1`);
  }
}
for (const id of ids(await list('/api/payroll/runs'))) paths.add(`/api/payroll/runs/${id}`);

/** Leaf paths where two JSON values differ. */
function differences(a, b, at, out = []) {
  if (JSON.stringify(a) === JSON.stringify(b)) return out;
  if (a && b && typeof a === 'object' && typeof b === 'object') {
    for (const k of new Set([...Object.keys(a), ...Object.keys(b)])) differences(a[k], b[k], `${at}.${k}`, out);
  } else {
    out.push(`${at}: reference ${JSON.stringify(a)?.slice(0, 160)} | imported ${JSON.stringify(b)?.slice(0, 160)}`);
  }
  return out;
}

let same = 0;
let deviations = 0;
let failures = 0;
for (const path of paths) {
  const [a, b] = await Promise.all([call(REF, path, rt), call(NEW, path, nt)]);
  const prep = (x) => JSON.stringify({ status: x.status, body: sortLines(clean(signedInUser(path.startsWith('/api/audit-logs?') ? mutationRows(x.body) : x.body))) });
  const [l, r] = [prep(a), prep(b)];
  if (l === r) {
    same++;
    continue;
  }
  const known = EXPECTED.find(([re]) => re.test(path));
  if (known) {
    deviations++;
    console.log(`DEVN  ${path} — ${known[1]}`);
  } else {
    failures++;
    console.log(`FAIL  ${path}`);
    for (const d of differences(JSON.parse(l), JSON.parse(r), '').slice(0, 12)) console.log(`        ${d}`);
  }
}
console.log(`\nside by side: ${paths.size} reads, ${same} identical, ${deviations} approved deviations, ${failures} mismatches`);
process.exit(failures ? 1 : 0);
