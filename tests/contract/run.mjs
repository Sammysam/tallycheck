// Contract test: runs one scripted scenario against the reference NestJS API and against the
// new Rust API, normalises volatile values, and compares status codes and JSON bodies.
//
//   node tests/contract/run.mjs http://127.0.0.1:3900 http://127.0.0.1:3901
//
// Both servers must start from a fresh database with ADMIN_USERNAME=admin and
// ADMIN_PASSWORD=admin12345 (see README.md). No dependencies beyond Node 24.

import { createHmac } from 'node:crypto';

const [REF, NEW] = process.argv.slice(2);
if (!REF || !NEW) {
  console.error('usage: node run.mjs <reference-base-url> <new-base-url>');
  process.exit(2);
}

// ── TOTP (RFC 6238, SHA-1, 6 digits, 30 s) ───────────────────────────────────
function base32Decode(s) {
  const alphabet = 'ABCDEFGHIJKLMNOPQRSTUVWXYZ234567';
  let bits = '';
  for (const c of s.replace(/=+$/, '').toUpperCase()) bits += alphabet.indexOf(c).toString(2).padStart(5, '0');
  const bytes = [];
  for (let i = 0; i + 8 <= bits.length; i += 8) bytes.push(parseInt(bits.slice(i, i + 8), 2));
  return Buffer.from(bytes);
}
function totp(secret, offset = 0) {
  const counter = Math.floor(Date.now() / 30000) + offset;
  const buf = Buffer.alloc(8);
  buf.writeBigUInt64BE(BigInt(counter));
  const h = createHmac('sha1', base32Decode(secret)).update(buf).digest();
  const o = h[h.length - 1] & 0xf;
  const code = (h.readUInt32BE(o) & 0x7fffffff) % 1_000_000;
  return String(code).padStart(6, '0');
}
// A code that is certainly wrong for the current ±1 window.
function wrongCode(secret) {
  const valid = new Set([-1, 0, 1].map((d) => totp(secret, d)));
  for (let i = 0; ; i++) {
    const c = String(i).padStart(6, '0');
    if (!valid.has(c)) return c;
  }
}

// ── HTTP ─────────────────────────────────────────────────────────────────────
async function call(base, method, path, { token, body, rawAuth } = {}) {
  const headers = {};
  if (token) headers.authorization = `Bearer ${token}`;
  if (rawAuth !== undefined) headers.authorization = rawAuth;
  if (body !== undefined) headers['content-type'] = 'application/json';
  const res = await fetch(base + path, { method, headers, body: body === undefined ? undefined : JSON.stringify(body) });
  const text = await res.text();
  let json;
  try { json = text ? JSON.parse(text) : null; } catch { json = text; }
  return { status: res.status, body: json };
}

// ── Scenario ─────────────────────────────────────────────────────────────────
const FIXED_UUID = '7c9e6679-7425-40de-944b-e07fc1f90ae7';

async function scenario(base) {
  const out = [];
  const s = {};
  const step = async (label, method, path, opts) => {
    const r = await call(base, method, path, opts);
    out.push({ label, method, path, ...r });
    return r;
  };

  await step('login.unknown-user', 'POST', '/api/auth/login', { body: { username: 'ghost', password: 'whatever1' } });
  await step('login.wrong-password', 'POST', '/api/auth/login', { body: { username: 'admin', password: 'wrong-password' } });
  let r = await step('login.ok', 'POST', '/api/auth/login', { body: { username: 'admin', password: 'admin12345' } });
  s.temp = r.body.tempToken;
  await step('me.with-temp-token', 'GET', '/api/auth/me', { token: s.temp });
  await step('me.no-token', 'GET', '/api/auth/me');
  await step('me.garbage-token', 'GET', '/api/auth/me', { token: 'abc.def.ghi' });
  await step('2fa.setup.no-header', 'GET', '/api/auth/2fa/setup');
  await step('2fa.setup.bad-token', 'GET', '/api/auth/2fa/setup', { token: 'nope' });
  r = await step('2fa.setup', 'GET', '/api/auth/2fa/setup', { token: s.temp });
  s.secret = r.body.secret;
  await step('2fa.enable.wrong-code', 'POST', '/api/auth/2fa/enable', { token: s.temp, body: { code: wrongCode(s.secret) } });
  r = await step('2fa.enable.ok', 'POST', '/api/auth/2fa/enable', { token: s.temp, body: { code: totp(s.secret) } });
  s.admin = r.body.accessToken;
  await step('me', 'GET', '/api/auth/me', { token: s.admin });
  r = await step('login.second-time', 'POST', '/api/auth/login', { body: { username: 'admin', password: 'admin12345' } });
  s.temp2 = r.body.tempToken;
  await step('2fa.verify.wrong-code', 'POST', '/api/auth/2fa/verify', { token: s.temp2, body: { code: wrongCode(s.secret) } });
  await step('2fa.verify.ok', 'POST', '/api/auth/2fa/verify', { token: s.temp2, body: { code: totp(s.secret) } });
  await step('2fa.setup.after-enabled', 'GET', '/api/auth/2fa/setup', { token: s.temp2 });

  // Users
  r = await step('roles.list', 'GET', '/api/roles', { token: s.admin });
  const roleId = (name) => r.body.find((x) => x.name === name)?.id;
  s.viewerRole = roleId('Viewer');
  s.accountantRole = roleId('Accountant');
  await step('roles.permissions', 'GET', '/api/roles/permissions', { token: s.admin });
  await step('users.list.initial', 'GET', '/api/users', { token: s.admin });
  await step('users.create.empty', 'POST', '/api/users', { token: s.admin, body: {} });
  await step('users.create.short', 'POST', '/api/users', { token: s.admin, body: { username: 'ab', password: 'short' } });
  await step('users.create.bad-chars', 'POST', '/api/users', { token: s.admin, body: { username: 'bad name!', password: 'longenough' } });
  await step('users.create.bad-role-id', 'POST', '/api/users', { token: s.admin, body: { username: 'clerk', password: 'longenough', roleId: 'nope' } });
  await step('users.create.unknown-role', 'POST', '/api/users', { token: s.admin, body: { username: 'clerk', password: 'longenough', roleId: FIXED_UUID } });
  r = await step('users.create.ok', 'POST', '/api/users', { token: s.admin, body: { username: 'clerk', password: 'longenough', extra: 'stripped' } });
  s.clerk = r.body.id;
  await step('users.create.duplicate', 'POST', '/api/users', { token: s.admin, body: { username: 'clerk', password: 'longenough' } });
  r = await step('users.create.viewer', 'POST', '/api/users', { token: s.admin, body: { username: 'viewer', password: 'viewerpass1', roleId: s.viewerRole } });
  s.viewerId = r.body.id;
  await step('users.update.rename', 'PATCH', `/api/users/${s.clerk}`, { token: s.admin, body: { username: 'clerk2' } });
  await step('users.update.rename-conflict', 'PATCH', `/api/users/${s.clerk}`, { token: s.admin, body: { username: 'viewer' } });
  await step('users.update.invalid', 'PATCH', `/api/users/${s.clerk}`, { token: s.admin, body: { isActive: 'yes', username: 'x' } });
  await step('users.update.unknown-id', 'PATCH', `/api/users/${FIXED_UUID}`, { token: s.admin, body: {} });
  const adminId = (await call(base, 'GET', '/api/auth/me', { token: s.admin })).body.id;
  await step('users.update.last-admin', 'PATCH', `/api/users/${adminId}`, { token: s.admin, body: { isActive: false } });
  // D9 is exercised in isolation: from no role (reference saves it but answers with the stale
  // role), then from one role to another (reference silently keeps the old role).
  // A throwaway user so the divergence does not leak into later steps.
  const mover = (await call(base, 'POST', '/api/users', { token: s.admin, body: { username: 'mover', password: 'longenough' } })).body.id;
  await step('users.update.role', 'PATCH', `/api/users/${mover}`, { token: s.admin, body: { roleId: s.accountantRole } });
  await step('users.update.role-change', 'PATCH', `/api/users/${mover}`, { token: s.admin, body: { roleId: s.viewerRole } });
  await step('users.list.after-role-change', 'GET', '/api/users', { token: s.admin });
  await call(base, 'PATCH', `/api/users/${mover}`, { token: s.admin, body: { isActive: false } });
  await step('users.reset-password.short', 'POST', `/api/users/${s.clerk}/reset-password`, { token: s.admin, body: { password: 'short' } });
  await step('users.reset-password', 'POST', `/api/users/${s.clerk}/reset-password`, { token: s.admin, body: { password: 'brandnewpass' } });
  await step('users.reset-2fa', 'POST', `/api/users/${s.clerk}/reset-2fa`, { token: s.admin });


  // Roles
  await step('roles.create.empty', 'POST', '/api/roles', { token: s.admin, body: {} });
  await step('roles.create.bad-permission', 'POST', '/api/roles', { token: s.admin, body: { name: 'Clerk', permissions: ['nope', 'nope'] } });
  r = await step('roles.create.ok', 'POST', '/api/roles', { token: s.admin, body: { name: 'Clerk', permissions: ['invoices', 'bills'] } });
  s.clerkRole = r.body.id;
  r = await step('roles.create.spare', 'POST', '/api/roles', { token: s.admin, body: { name: 'Spare', permissions: [] } });
  s.spareRole = r.body.id;
  await step('roles.create.duplicate', 'POST', '/api/roles', { token: s.admin, body: { name: 'Clerk', permissions: [] } });
  const adminRole = (await call(base, 'GET', '/api/roles', { token: s.admin })).body.find((x) => x.name === 'Admin').id;
  await step('roles.update.admin', 'PATCH', `/api/roles/${adminRole}`, { token: s.admin, body: { name: 'Boss' } });
  await step('roles.update.ok', 'PATCH', `/api/roles/${s.clerkRole}`, { token: s.admin, body: { permissions: ['invoices'] } });
  await step('roles.update.conflict', 'PATCH', `/api/roles/${s.clerkRole}`, { token: s.admin, body: { name: 'Viewer' } });
  await step('roles.delete.admin', 'DELETE', `/api/roles/${adminRole}`, { token: s.admin });
  await step('users.create.with-role', 'POST', '/api/users', { token: s.admin, body: { username: 'assignee', password: 'longenough', roleId: s.clerkRole } });
  await step('roles.delete.assigned', 'DELETE', `/api/roles/${s.clerkRole}`, { token: s.admin });
  await step('roles.delete.ok', 'DELETE', `/api/roles/${s.spareRole}`, { token: s.admin });
  await step('roles.delete.unknown', 'DELETE', `/api/roles/${FIXED_UUID}`, { token: s.admin });
  await step('roles.list.final', 'GET', '/api/roles', { token: s.admin });

  // Settings
  await step('settings.get', 'GET', '/api/settings', { token: s.admin });
  await step('settings.patch.ok', 'PATCH', '/api/settings', { token: s.admin, body: { signatoryName: '  Jane Doe  ', junk: 1 } });
  await step('settings.patch.invalid', 'PATCH', '/api/settings', { token: s.admin, body: { signatoryTitle: 5 } });

  // Routing and guards
  await step('route.unknown', 'GET', '/api/nope?x=1');
  await step('route.later-wave', 'GET', '/api/does-not-exist/abc');
  await step('route.wrong-method', 'DELETE', '/api/users');
  await step('guard.no-auth-before-validation', 'POST', '/api/users', { body: {} });

  // Viewer: limited permissions
  r = await call(base, 'POST', '/api/auth/login', { body: { username: 'viewer', password: 'viewerpass1' } });
  const vt = r.body.tempToken;
  const vs = (await call(base, 'GET', '/api/auth/2fa/setup', { token: vt })).body.secret;
  s.viewer = (await call(base, 'POST', '/api/auth/2fa/enable', { token: vt, body: { code: totp(vs) } })).body.accessToken;
  await step('viewer.users', 'GET', '/api/users', { token: s.viewer });
  await step('viewer.create-user-invalid', 'POST', '/api/users', { token: s.viewer, body: {} });
  await step('viewer.settings.get', 'GET', '/api/settings', { token: s.viewer });
  await step('viewer.settings.patch', 'PATCH', '/api/settings', { token: s.viewer, body: { signatoryName: 'x' } });
  await step('viewer.audit', 'GET', '/api/audit-logs', { token: s.viewer });
  await step('viewer.me', 'GET', '/api/auth/me', { token: s.viewer });

  // Deactivated viewer
  await call(base, 'PATCH', `/api/users/${s.viewerId}`, { token: s.admin, body: { isActive: false } });
  await step('inactive.users', 'GET', '/api/users', { token: s.viewer });
  await step('inactive.me', 'GET', '/api/auth/me', { token: s.viewer });
  await step('inactive.login', 'POST', '/api/auth/login', { body: { username: 'viewer', password: 'viewerpass1' } });

  // Audit trail last, so it covers everything above.
  await step('audit.list', 'GET', '/api/audit-logs?limit=100', { token: s.admin });
  await step('audit.invalid-pagination', 'GET', '/api/audit-logs?page=abc&limit=101', { token: s.admin });
  await step('audit.users', 'GET', '/api/audit-logs/users', { token: s.admin });
  return out;
}

// ── Normalisation ────────────────────────────────────────────────────────────
const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;
const ISO = /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(\.\d+)?Z$/;
const JWT = /^[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+$/;

function normaliser() {
  const ids = new Map();
  const norm = (v, key) => {
    if (Array.isArray(v)) return v.map((x) => norm(x));
    if (v && typeof v === 'object') {
      return Object.fromEntries(Object.keys(v).sort().map((k) => [k, norm(v[k], k)]));
    }
    if (typeof v !== 'string') return v;
    if (key === 'secret') return '<base32-secret>';
    if (v.startsWith('data:image/png;base64,')) return '<png-data-url>';
    if (ISO.test(v)) return '<timestamp>';
    if (JWT.test(v)) return '<jwt>';
    if (UUID.test(v)) {
      if (v === FIXED_UUID) return '<fixed-uuid>';
      if (!ids.has(v)) ids.set(v, `<uuid#${ids.size + 1}>`);
      return ids.get(v);
    }
    // ids embedded in messages / paths
    return v.replace(/[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}/gi, (m) => {
      if (m === FIXED_UUID) return '<fixed-uuid>';
      if (!ids.has(m)) ids.set(m, `<uuid#${ids.size + 1}>`);
      return ids.get(m);
    });
  };
  return norm;
}

// Approved deviations (CLAUDE.md §9). A difference on these labels is reported, not failed.
const EXPECTED = {
  '2fa.setup.after-enabled': 'D2: 2FA cannot be re-enrolled with a password alone',
  'inactive.me': 'D2: inactive users are refused on every protected route',
  'inactive.login': 'D2: inactive users cannot log in',
  'users.update.role':
    'D9: the reference returns the stale eager role (roleId null) and silently drops a change away from an existing role',
  'users.update.role-change': 'D9: the reference answers with (and keeps) the old role',
  'users.list.after-role-change': 'D9: on the reference the user keeps the old role; here the change was saved',
  'audit.users': 'D2: failed logins are audited (adds rows for known users)',
};

// Login rows legitimately differ (D2: failed and inactive logins), so the audit trail is
// compared on the mutation rows written by the interceptor only.
function mutationRows(body) {
  if (!body || !Array.isArray(body.data)) return body;
  const data = body.data.filter((e) => e.path !== '/api/auth/login');
  return { ...body, data, total: data.length };
}

const stable = (v) => JSON.stringify(v);

const [ref, neu] = await Promise.all([scenario(REF), scenario(NEW)]);
const nr = normaliser();
const nn = normaliser();
let failures = 0;
let deviations = 0;
for (let i = 0; i < ref.length; i++) {
  const a = ref[i];
  const b = neu[i];
  const audit = a.label === 'audit.list';
  const left = { status: a.status, body: nr(audit ? mutationRows(a.body) : a.body) };
  const right = { status: b.status, body: nn(audit ? mutationRows(b.body) : b.body) };
  const same = stable(left) === stable(right);
  if (same) {
    console.log(`PASS  ${a.label}`);
  } else if (EXPECTED[a.label]) {
    deviations++;
    console.log(`DEVN  ${a.label} — ${EXPECTED[a.label]}`);
    console.log(`        reference: ${a.status} ${stable(left.body).slice(0, 300)}`);
    console.log(`        new:       ${b.status} ${stable(right.body).slice(0, 300)}`);
  } else {
    failures++;
    console.log(`FAIL  ${a.label}  ${a.method} ${a.path}`);
    console.log(`        reference: ${a.status} ${stable(left.body)}`);
    console.log(`        new:       ${b.status} ${stable(right.body)}`);
  }
}
console.log(`\n${ref.length} steps: ${ref.length - failures - deviations} identical, ${deviations} approved deviations, ${failures} mismatches`);
process.exit(failures ? 1 : 0);
