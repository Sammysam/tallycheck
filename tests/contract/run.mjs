// Contract test: runs one scripted scenario against the reference NestJS API and against the
// new Rust API, normalises volatile values, and compares status codes and JSON bodies.
//
//   node tests/contract/run.mjs http://127.0.0.1:3900 http://127.0.0.1:3901
//
// Both servers must start from a fresh database with ADMIN_USERNAME=admin and
// ADMIN_PASSWORD=admin12345 (see README.md). No dependencies beyond Node 24.

import { createHash, createHmac } from 'node:crypto';

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

// Multipart upload, as the frontend's FormData posts do.
async function upload(base, method, path, { token, fields = {}, file } = {}) {
  const form = new FormData();
  for (const [k, v] of Object.entries(fields)) form.append(k, v);
  if (file) form.append('file', new Blob([file.bytes], { type: file.type }), file.name);
  let res;
  try {
    res = await fetch(base + path, { method, headers: token ? { authorization: `Bearer ${token}` } : {}, body: form });
  } catch (e) {
    // A server may answer an oversized upload and close the connection before the body is sent.
    return { status: 0, body: `connection closed during upload (${e.cause?.code ?? e.message})` };
  }
  const text = await res.text();
  let json;
  try { json = text ? JSON.parse(text) : null; } catch { json = text; }
  return { status: res.status, body: json };
}

// Binary download: compared by status, the headers the frontend relies on, size and hash.
async function download(base, path, token) {
  const res = await fetch(base + path, { headers: { authorization: `Bearer ${token}` } });
  const bytes = Buffer.from(await res.arrayBuffer());
  if (res.status >= 400) {
    let body; try { body = JSON.parse(bytes.toString()); } catch { body = bytes.toString(); }
    return { status: res.status, body };
  }
  return {
    status: res.status,
    body: {
      contentType: res.headers.get('content-type'),
      contentDisposition: res.headers.get('content-disposition'),
      cacheControl: res.headers.get('cache-control'),
      size: bytes.length,
      sha256: createHash('sha256').update(bytes).digest('hex'),
    },
  };
}

const PNG = Buffer.from('89504e470d0a1a0a0000000d49484452000000010000000108060000001f15c4890000000d49444154789c6360000002000154a24f5d0000000049454e44ae426082', 'hex');
const PDF = Buffer.from('%PDF-1.4\n% test document for the contract suite\n1 0 obj << >> endobj\ntrailer << >>\n%%EOF\n');
const PDF2 = Buffer.from('%PDF-1.4\n% second version\n%%EOF\n');

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

  // ── W1: chart of accounts ──────────────────────────────────────────────────
  r = await step('accounts.list.initial', 'GET', '/api/accounts', { token: s.admin });
  s.acct = Object.fromEntries((Array.isArray(r.body) ? r.body : []).map((a) => [a.code, a.id]));
  await step('accounts.create.empty', 'POST', '/api/accounts', { token: s.admin, body: {} });
  await step('accounts.create.bad-type', 'POST', '/api/accounts', { token: s.admin, body: { code: '1000', name: 'Cash', type: 'INCOME' } });
  const mk = async (label, body) => {
    const res = await step(label, 'POST', '/api/accounts', { token: s.admin, body });
    s.acct[body.code] = res.body?.id;
    return res;
  };
  await mk('accounts.create.cash', { code: '1000', name: 'Cash', type: 'ASSET', subtype: 'current_asset' });
  await mk('accounts.create.ar', { code: '1100', name: 'Accounts Receivable', type: 'ASSET', subtype: 'current_asset', description: 'Trade debtors', junk: 1 });
  await mk('accounts.create.equity', { code: '3000', name: "Owner's Equity", type: 'EQUITY', subtype: 'equity' });
  await mk('accounts.create.revenue', { code: '4000', name: 'Revenue', type: 'REVENUE', subtype: 'operating_revenue' });
  await mk('accounts.create.expense', { code: '6000', name: 'Operating Expenses', type: 'EXPENSE', subtype: 'operating_expense' });
  await mk('accounts.create.child', { code: '1010', name: 'Petty Cash', type: 'ASSET', parentId: s.acct['1000'] });
  await mk('accounts.create.spare', { code: '9999', name: 'Spare', type: 'EXPENSE' });
  await step('accounts.create.duplicate', 'POST', '/api/accounts', { token: s.admin, body: { code: '1000', name: 'Again', type: 'ASSET' } });
  await step('accounts.get', 'GET', `/api/accounts/${s.acct['1100']}`, { token: s.admin });
  await step('accounts.get.unknown', 'GET', `/api/accounts/${FIXED_UUID}`, { token: s.admin });
  await step('accounts.update', 'PATCH', `/api/accounts/${s.acct['1010']}`, { token: s.admin, body: { name: 'Petty Cash Float', isActive: false } });
  await step('accounts.update.invalid', 'PATCH', `/api/accounts/${s.acct['1010']}`, { token: s.admin, body: { type: 'X' } });
  await step('accounts.update.code-conflict', 'PATCH', `/api/accounts/${s.acct['1010']}`, { token: s.admin, body: { code: '1000' } });
  await step('accounts.delete.unused', 'DELETE', `/api/accounts/${s.acct['9999']}`, { token: s.admin });
  await step('accounts.list', 'GET', '/api/accounts', { token: s.admin });

  // ── W1: journal entries ────────────────────────────────────────────────────
  const line = (code, debit, credit, description) => ({ accountId: s.acct[code], debit, credit, ...(description ? { description } : {}) });
  const je = (body) => ({ token: s.admin, body });
  await step('ledger.create.empty', 'POST', '/api/ledger/entries', je({}));
  await step('ledger.create.one-line', 'POST', '/api/ledger/entries', je({ date: '2026-04-01', description: 'x', lines: [line('1000', 10, 0)] }));
  await step('ledger.create.bad-lines', 'POST', '/api/ledger/entries', je({ date: '2026-04-01', description: 'x', lines: [{ accountId: 'bad', debit: '10', credit: -1 }, { accountId: s.acct['1000'], debit: 1.005, credit: 0 }] }));
  await step('ledger.create.unbalanced', 'POST', '/api/ledger/entries', je({ date: '2026-04-01', description: 'x', lines: [line('1000', 100.1, 0), line('3000', 0, 100)] }));
  await step('ledger.create.float-sum-balanced', 'POST', '/api/ledger/entries', je({ date: '2026-04-02', description: 'Float check', lines: [line('1000', 0.1, 0), line('1000', 0.2, 0), line('3000', 0, 0.3)] }));
  r = await step('ledger.create.opening', 'POST', '/api/ledger/entries', je({ date: '2026-04-01', description: 'Opening balance', reference: 'OPEN-1', sourceType: 'MANUAL', sourceId: 'manual-1', lines: [line('1000', 1000.5, 0, 'Cash in hand'), line('3000', 0, 1000.5)] }));
  s.je1 = r.body?.id;
  r = await step('ledger.create.expense', 'POST', '/api/ledger/entries', je({ date: '2026-04-15', description: 'Office supplies', lines: [line('6000', 250, 0), line('1000', 0, 250)] }));
  s.je2 = r.body?.id;
  r = await step('ledger.create.three-lines', 'POST', '/api/ledger/entries', je({ date: '2026-04-20', description: 'Sale', sourceType: 'MANUAL', lines: [line('1000', 100, 0, 'cash part'), line('1100', 50.25, 0, 'on account'), line('4000', 0, 150.25, 'sales')] }));
  s.je3 = r.body?.id;
  await step('ledger.entries', 'GET', '/api/ledger/entries', { token: s.admin });
  await step('ledger.entries.by-source', 'GET', '/api/ledger/entries?sourceType=MANUAL', { token: s.admin });
  await step('ledger.entries.by-dates', 'GET', '/api/ledger/entries?dateFrom=2026-04-10&dateTo=2026-04-30', { token: s.admin });
  await step('ledger.entries.only-from', 'GET', '/api/ledger/entries?dateFrom=2026-04-10', { token: s.admin });
  await step('ledger.entries.bad-date', 'GET', '/api/ledger/entries?dateFrom=2026-02-30', { token: s.admin });
  await step('ledger.entry', 'GET', `/api/ledger/entries/${s.je3}`, { token: s.admin });
  await step('ledger.entry.unknown', 'GET', `/api/ledger/entries/${FIXED_UUID}`, { token: s.admin });
  await step('ledger.reverse', 'POST', `/api/ledger/entries/${s.je2}/reverse`, { token: s.admin });
  await step('ledger.reverse.again', 'POST', `/api/ledger/entries/${s.je2}/reverse`, { token: s.admin });
  await step('ledger.reverse.unknown', 'POST', `/api/ledger/entries/${FIXED_UUID}/reverse`, { token: s.admin });
  await step('ledger.entry.reversed', 'GET', `/api/ledger/entries/${s.je2}`, { token: s.admin });
  await step('ledger.trial-balance', 'GET', '/api/ledger/trial-balance', { token: s.admin });
  await step('ledger.trial-balance.as-of', 'GET', '/api/ledger/trial-balance?asOf=2026-04-10', { token: s.admin });
  await step('ledger.balance.cash', 'GET', `/api/ledger/balance/${s.acct['1000']}`, { token: s.admin });
  await step('ledger.balance.equity', 'GET', `/api/ledger/balance/${s.acct['3000']}`, { token: s.admin });
  await step('ledger.balance.range', 'GET', `/api/ledger/balance/${s.acct['1000']}?dateFrom=2026-04-10&dateTo=2026-04-30`, { token: s.admin });
  await step('ledger.balance.unknown-account', 'GET', `/api/ledger/balance/${FIXED_UUID}`, { token: s.admin });
  await step('accounts.delete.used', 'DELETE', `/api/accounts/${s.acct['1000']}`, { token: s.admin });
  await step('accounts.delete.unknown', 'DELETE', `/api/accounts/${FIXED_UUID}`, { token: s.admin });
  await step('viewer.accounts', 'GET', '/api/accounts', { token: s.viewer });
  await step('viewer.accounts.create', 'POST', '/api/accounts', { token: s.viewer, body: { code: '7000', name: 'X', type: 'EXPENSE' } });
  await step('viewer.ledger.entries', 'GET', '/api/ledger/entries?sourceType=NONE', { token: s.viewer });

  // ── W2: customers, vendors, banking ────────────────────────────────────────
  const A = (body) => ({ token: s.admin, body });
  await mk('accounts.create.ap', { code: '2000', name: 'Accounts Payable', type: 'LIABILITY', subtype: 'current_liability' });
  await step('customers.create.empty', 'POST', '/api/customers', A({}));
  await step('customers.create.bad-email', 'POST', '/api/customers', A({ name: 'Acme', email: 'not-an-email' }));
  r = await step('customers.create.acme', 'POST', '/api/customers', A({ name: 'Acme Ltd', email: 'ap@acme.lk', phone: '011', address: '1 Main St', taxId: 'VAT1', junk: 1 }));
  s.acme = r.body?.id;
  r = await step('customers.create.beta', 'POST', '/api/customers', A({ name: 'Beta Traders' }));
  s.beta = r.body?.id;
  await step('customers.list', 'GET', '/api/customers', { token: s.admin });
  await step('customers.get', 'GET', `/api/customers/${s.acme}`, { token: s.admin });
  await step('customers.get.unknown', 'GET', `/api/customers/${FIXED_UUID}`, { token: s.admin });
  await step('customers.update', 'PATCH', `/api/customers/${s.beta}`, A({ phone: '077', isActive: false }));
  await step('customers.delete.unused', 'DELETE', `/api/customers/${s.beta}`, { token: s.admin });
  await step('viewer.customers', 'GET', '/api/customers', { token: s.viewer });
  await step('viewer.customers.create', 'POST', '/api/customers', { token: s.viewer, body: { name: 'X' } });

  await step('vendors.create.empty', 'POST', '/api/vendors', A({}));
  r = await step('vendors.create', 'POST', '/api/vendors', A({ name: 'Paper Co', email: 'billing@paper.lk' }));
  s.vendor = r.body?.id;
  await step('vendors.list', 'GET', '/api/vendors', { token: s.admin });
  await step('vendors.get', 'GET', `/api/vendors/${s.vendor}`, { token: s.admin });
  await step('vendors.update', 'PATCH', `/api/vendors/${s.vendor}`, A({ address: 'Colombo' }));
  await step('viewer.vendors', 'GET', '/api/vendors', { token: s.viewer });

  await step('banking.create.invalid', 'POST', '/api/banking', A({ name: 'BOC', accountId: 'x' }));
  r = await step('banking.create', 'POST', '/api/banking', A({ name: 'BOC Current', bankName: 'Bank of Ceylon', accountNumber: '0071234', accountId: s.acct['1000'] }));
  s.bank = r.body?.id;
  r = await step('banking.create.second', 'POST', '/api/banking', A({ name: 'Petty', accountId: s.acct['1000'] }));
  s.bank2 = r.body?.id;
  await step('banking.get', 'GET', `/api/banking/${s.bank}`, { token: s.admin });
  await step('banking.update', 'PATCH', `/api/banking/${s.bank}`, A({ accountNumber: '0079999' }));
  await step('banking.deactivate', 'DELETE', `/api/banking/${s.bank2}`, { token: s.admin });
  await step('banking.list', 'GET', '/api/banking', { token: s.admin });
  await step('banking.get.unknown', 'GET', `/api/banking/${FIXED_UUID}`, { token: s.admin });

  // ── W2: invoices ───────────────────────────────────────────────────────────
  const rev = s.acct['4000'];
  await step('invoices.create.empty', 'POST', '/api/invoices', A({}));
  await step('invoices.create.no-billto', 'POST', '/api/invoices', A({ date: '2026-04-01', lines: [{ description: 'x' }] }));
  r = await step('invoices.create.acme', 'POST', '/api/invoices', A({
    customerId: s.acme, date: '2026-04-01', paymentTerms: 'NET_30', poNumber: 'PO-77', notes: 'Thanks',
    paymentOptions: [' Bank transfer ', '', 'Cash'], signatoryName: '  ', junk: 1,
    lines: [{ description: 'Design', quantity: 2.5, unitPrice: 1000.25, accountId: rev }, { description: 'Free consult' }],
  }));
  s.inv1 = r.body?.id;
  r = await step('invoices.create.walkin', 'POST', '/api/invoices', A({
    billToName: 'Walk-in Client', billToAddress: 'Galle', date: '2026-04-05', dueDate: '2026-04-20',
    lines: [{ description: 'Consult', quantity: 1, unitPrice: 500, accountId: rev }],
  }));
  s.inv2 = r.body?.id;
  await step('invoices.get', 'GET', `/api/invoices/${s.inv1}`, { token: s.admin });
  await step('invoices.get.unknown', 'GET', `/api/invoices/${FIXED_UUID}`, { token: s.admin });
  await step('invoices.list', 'GET', '/api/invoices', { token: s.admin });
  await step('invoices.list.search', 'GET', '/api/invoices?q=walk', { token: s.admin });
  await step('invoices.list.search-customer', 'GET', '/api/invoices?q=ACME', { token: s.admin });
  await step('invoices.list.filters', 'GET', `/api/invoices?status=DRAFT&customerId=${s.acme}&dateFrom=2026-04-01&dateTo=2026-04-03&limit=5`, { token: s.admin });
  await step('invoices.list.bad-query', 'GET', '/api/invoices?status=OPEN&limit=500', { token: s.admin });
  await step('invoices.update.draft', 'PATCH', `/api/invoices/${s.inv1}`, A({ paymentTerms: 'NET_15', notes: 'Updated', lines: [{ description: 'Design', quantity: 2.5, unitPrice: 1000.25, accountId: rev }, { description: 'Free consult' }, { description: 'Hosting', quantity: 12, unitPrice: 15.5, accountId: rev }] }));
  await step('invoices.post.blank-account', 'POST', `/api/invoices/${s.inv1}/post`, { token: s.admin });
  await step('invoices.update.accounts', 'PATCH', `/api/invoices/${s.inv1}`, A({ lines: [{ description: 'Design', quantity: 2.5, unitPrice: 1000.25, accountId: rev }, { description: 'Hosting', quantity: 12, unitPrice: 15.5, accountId: rev }] }));
  await step('invoices.post', 'POST', `/api/invoices/${s.inv1}/post`, { token: s.admin });
  await step('invoices.post.again', 'POST', `/api/invoices/${s.inv1}/post`, { token: s.admin });
  await step('invoices.update.posted-money', 'PATCH', `/api/invoices/${s.inv1}`, A({ notes: 'no' }));
  await step('invoices.update.posted-po', 'PATCH', `/api/invoices/${s.inv1}`, A({ poNumber: ' PO-78 ' }));
  await step('invoices.delete.posted', 'DELETE', `/api/invoices/${s.inv1}`, { token: s.admin });
  await step('invoices.post.walkin', 'POST', `/api/invoices/${s.inv2}/post`, { token: s.admin });
  await step('invoices.unpost', 'POST', `/api/invoices/${s.inv2}/unpost`, { token: s.admin });
  await step('invoices.unpost.draft', 'POST', `/api/invoices/${s.inv2}/unpost`, { token: s.admin });
  await step('invoices.status.posted', 'PATCH', `/api/invoices/${s.inv2}/status`, A({ status: 'POSTED' }));
  await step('invoices.status.paid', 'PATCH', `/api/invoices/${s.inv2}/status`, A({ status: 'PAID' }));
  await step('invoices.status.invalid', 'PATCH', `/api/invoices/${s.inv2}/status`, A({ status: 'OPEN' }));
  await step('invoices.status.void', 'PATCH', `/api/invoices/${s.inv2}/status`, A({ status: 'VOID' }));
  await step('invoices.status.from-void', 'PATCH', `/api/invoices/${s.inv2}/status`, A({ status: 'DRAFT' }));
  await step('invoices.void.again', 'POST', `/api/invoices/${s.inv2}/void`, { token: s.admin });
  await step('invoices.delete.void', 'DELETE', `/api/invoices/${s.inv2}`, { token: s.admin });
  await step('invoices.update.void', 'PATCH', `/api/invoices/${s.inv2}`, A({ poNumber: 'x' }));
  r = await step('invoices.duplicate', 'POST', `/api/invoices/${s.inv2}/duplicate`, { token: s.admin });
  s.dup = r.body?.id;

  // ── W2: payments against invoices ──────────────────────────────────────────
  const cash = s.acct['1000'];
  const pay = (body) => ({ token: s.admin, body: { paymentMethod: 'BANK_TRANSFER', paymentAccountId: cash, date: '2026-04-25', ...body } });
  await step('payments.ar.no-invoice', 'POST', '/api/payments', pay({ type: 'AR', amount: 10 }));
  await step('payments.ar.draft', 'POST', '/api/payments', pay({ type: 'AR', invoiceId: s.dup, amount: 10 }));
  await step('payments.ar.void', 'POST', '/api/payments', pay({ type: 'AR', invoiceId: s.inv2, amount: 10 }));
  await step('payments.ar.partial', 'POST', '/api/payments', pay({ type: 'AR', invoiceId: s.inv1, customerId: s.acme, amount: 1000, notes: 'part' }));
  await step('invoices.get.part-paid', 'GET', `/api/invoices/${s.inv1}`, { token: s.admin });
  await step('payments.ar.too-much', 'POST', '/api/payments', pay({ type: 'AR', invoiceId: s.inv1, amount: 99999 }));
  await step('invoices.void.part-paid', 'POST', `/api/invoices/${s.inv1}/void`, { token: s.admin });
  await step('invoices.unpost.part-paid', 'POST', `/api/invoices/${s.inv1}/unpost`, { token: s.admin });
  await step('payments.ar.rest', 'POST', '/api/payments', pay({ type: 'AR', invoiceId: s.inv1, amount: 1686.63, paymentMethod: 'CHECK' }));
  await step('invoices.get.paid', 'GET', `/api/invoices/${s.inv1}`, { token: s.admin });
  await step('payments.ar.already-paid', 'POST', '/api/payments', pay({ type: 'AR', invoiceId: s.inv1, amount: 1 }));
  await step('invoices.void.paid', 'POST', `/api/invoices/${s.inv1}/void`, { token: s.admin });
  await step('invoices.status.from-paid', 'PATCH', `/api/invoices/${s.inv1}/status`, A({ status: 'VOID' }));
  await step('invoices.delete.draft', 'DELETE', `/api/invoices/${s.dup}`, { token: s.admin });

  // ── W2: bills and AP payments ──────────────────────────────────────────────
  const exp = s.acct['6000'];
  await step('bills.create.empty', 'POST', '/api/bills', A({}));
  r = await step('bills.create', 'POST', '/api/bills', A({ vendorId: s.vendor, date: '2026-04-03', dueDate: '2026-05-03', notes: 'April paper', lines: [{ description: 'Paper', quantity: 3, unitPrice: 12.5, accountId: exp }, { description: 'Ink', quantity: 1, unitPrice: 20, accountId: exp }] }));
  s.bill1 = r.body?.id;
  await step('bills.get', 'GET', `/api/bills/${s.bill1}`, { token: s.admin });
  await step('bills.get.unknown', 'GET', `/api/bills/${FIXED_UUID}`, { token: s.admin });
  await step('bills.post', 'POST', `/api/bills/${s.bill1}/post`, { token: s.admin });
  await step('bills.post.again', 'POST', `/api/bills/${s.bill1}/post`, { token: s.admin });
  await step('payments.ap.no-bill', 'POST', '/api/payments', pay({ type: 'AP', amount: 10 }));
  await step('payments.ap.full', 'POST', '/api/payments', pay({ type: 'AP', billId: s.bill1, vendorId: s.vendor, amount: 57.5 }));
  await step('bills.get.paid', 'GET', `/api/bills/${s.bill1}`, { token: s.admin });
  await step('bills.void.paid', 'POST', `/api/bills/${s.bill1}/void`, { token: s.admin });
  r = await call(base, 'POST', '/api/bills', A({ vendorId: s.vendor, date: '2026-04-04', lines: [{ description: 'Draft only', quantity: 1, unitPrice: 5, accountId: exp }] }));
  await step('bills.void.draft', 'POST', `/api/bills/${r.body?.id}/void`, { token: s.admin });
  await step('payments.ap.void-bill', 'POST', '/api/payments', pay({ type: 'AP', billId: r.body?.id, amount: 5 }));
  r = await call(base, 'POST', '/api/bills', A({ vendorId: s.vendor, date: '2026-04-06', lines: [{ description: 'Toner', quantity: 2, unitPrice: 40, accountId: exp }] }));
  await call(base, 'POST', `/api/bills/${r.body?.id}/post`, { token: s.admin });
  await step('bills.void.posted', 'POST', `/api/bills/${r.body?.id}/void`, { token: s.admin });
  await step('bills.list', 'GET', '/api/bills?limit=50', { token: s.admin });

  // ── W2: expenses ───────────────────────────────────────────────────────────
  await step('expenses.create.empty', 'POST', '/api/expenses', A({}));
  r = await step('expenses.create', 'POST', '/api/expenses', A({ date: '2026-04-08', description: 'Taxi', amount: 1500.75, expenseAccountId: exp, paymentAccountId: cash, receiptUrl: 'receipt-1' }));
  s.exp1 = r.body?.id;
  await step('expenses.get', 'GET', `/api/expenses/${s.exp1}`, { token: s.admin });
  await step('expenses.post', 'POST', `/api/expenses/${s.exp1}/post`, { token: s.admin });
  await step('expenses.post.again', 'POST', `/api/expenses/${s.exp1}/post`, { token: s.admin });
  await step('expenses.void', 'POST', `/api/expenses/${s.exp1}/void`, { token: s.admin });
  await step('expenses.void.again', 'POST', `/api/expenses/${s.exp1}/void`, { token: s.admin });
  r = await call(base, 'POST', '/api/expenses', A({ date: '2026-04-09', description: 'Lunch', amount: 20, expenseAccountId: exp, paymentAccountId: cash }));
  await step('expenses.void.draft', 'POST', `/api/expenses/${r.body?.id}/void`, { token: s.admin });
  await step('expenses.list', 'GET', '/api/expenses', { token: s.admin });
  await step('expenses.get.unknown', 'GET', `/api/expenses/${FIXED_UUID}`, { token: s.admin });

  await step('payments.list', 'GET', '/api/payments?limit=50', { token: s.admin });
  await step('payments.get.unknown', 'GET', `/api/payments/${FIXED_UUID}`, { token: s.admin });
  await step('customers.delete.used', 'DELETE', `/api/customers/${s.acme}`, { token: s.admin });
  await step('ledger.trial-balance.after-w2', 'GET', '/api/ledger/trial-balance', { token: s.admin });
  await step('ledger.entries.invoices', 'GET', '/api/ledger/entries?sourceType=INVOICE', { token: s.admin });

  // ── W3: reports ────────────────────────────────────────────────────────────
  // Open items for ageing: a part-paid invoice and an unpaid bill (no voids, no part-paid bills).
  r = await call(base, 'POST', '/api/invoices', A({ customerId: s.acme, date: '2026-04-16', dueDate: '2026-04-30', lines: [{ description: 'Retainer', unitPrice: 800, accountId: rev }] }));
  s.inv3 = r.body?.id;
  await call(base, 'POST', `/api/invoices/${s.inv3}/post`, { token: s.admin });
  await call(base, 'POST', '/api/payments', pay({ type: 'AR', invoiceId: s.inv3, amount: 300 }));
  r = await call(base, 'POST', '/api/bills', A({ vendorId: s.vendor, date: '2026-04-17', dueDate: '2026-06-01', lines: [{ description: 'Cleaning', quantity: 1, unitPrice: 120, accountId: exp }] }));
  await call(base, 'POST', `/api/bills/${r.body?.id}/post`, { token: s.admin });
  // Ranges without any voided document: identical on both sides.
  await step('reports.profit-loss.clean-range', 'GET', '/api/reports/profit-loss?dateFrom=2026-04-16&dateTo=2026-04-30', { token: s.admin });
  await step('reports.profit-loss.open-start', 'GET', '/api/reports/profit-loss?dateFrom=&dateTo=2026-04-04', { token: s.admin });
  await step('reports.balance-sheet.clean', 'GET', '/api/reports/balance-sheet?asOf=2026-04-04', { token: s.admin });
  await step('reports.ar-aging.as-of', 'GET', '/api/reports/ar-aging?asOf=2026-05-15', { token: s.admin });
  await step('reports.ar-aging', 'GET', '/api/reports/ar-aging', { token: s.admin });
  await step('reports.ap-aging.as-of', 'GET', '/api/reports/ap-aging?asOf=2026-05-15', { token: s.admin });
  await step('reports.ap-aging', 'GET', '/api/reports/ap-aging', { token: s.admin });
  // Whole year: the voided documents above make D1 visible.
  await step('reports.profit-loss.year', 'GET', '/api/reports/profit-loss', { token: s.admin });
  await step('reports.balance-sheet.today', 'GET', '/api/reports/balance-sheet', { token: s.admin });
  await step('reports.profit-loss.bad-date', 'GET', '/api/reports/profit-loss?dateFrom=2026-13-01', { token: s.admin });
  await step('viewer.reports', 'GET', '/api/reports/ar-aging', { token: s.viewer });
  await step('viewer.reports.anonymous', 'GET', '/api/reports/ar-aging');

  // ── W4: employees ──────────────────────────────────────────────────────────
  const E = (body) => ({ token: s.admin, body });
  await step('employees.create.empty', 'POST', '/api/employees', E({}));
  r = await step('employees.create', 'POST', '/api/employees', E({ employeeId: ' CEN-001 ', name: ' Asha Perera ', jobTitle: 'Engineer', department: 'Ops',
    email: ' asha@cenango.lk ', phone: '', joinDate: '2024-01-15', notes: '  ', epfNumber: ' 101 ', nationalId: '199012345678', junk: 1 }));
  s.emp1 = r.body?.id;
  await step('employees.create.duplicate-case', 'POST', '/api/employees', E({ employeeId: 'cen-001', name: 'Other' }));
  r = await step('employees.create.contract', 'POST', '/api/employees', E({ employeeId: 'CEN-002', name: 'Nimal Silva', employmentType: 'CONTRACT', status: 'ACTIVE' }));
  s.emp2 = r.body?.id;
  await step('employees.list', 'GET', '/api/employees', { token: s.admin });
  await step('employees.list.search', 'GET', '/api/employees?q=%20ops%20', { token: s.admin });
  await step('employees.list.status', 'GET', '/api/employees?status=ON_LEAVE', { token: s.admin });
  await step('employees.list.bad-status', 'GET', '/api/employees?status=GONE', { token: s.admin });
  await step('employees.get', 'GET', `/api/employees/${s.emp1}`, { token: s.admin });
  await step('employees.get.not-uuid', 'GET', '/api/employees/not-a-uuid', { token: s.admin });
  await step('employees.get.unknown', 'GET', `/api/employees/${FIXED_UUID}`, { token: s.admin });
  await step('employees.update', 'PATCH', `/api/employees/${s.emp2}`, E({ status: 'ON_LEAVE', jobTitle: '', bankName: 'ignored' }));
  await step('employees.update.id-clash', 'PATCH', `/api/employees/${s.emp2}`, E({ employeeId: 'Cen-001' }));
  await step('employees.update.invalid', 'PATCH', `/api/employees/${s.emp2}`, E({ name: '  ', email: 'nope' }));

  // Photo
  const shoot = (label, path, opts) => { const t = upload(base, 'POST', path, { token: s.admin, ...opts }).then((x) => { out.push({ label, method: 'POST', path, ...x }); return x; }); return t; };
  await step('employees.photo.none', 'GET', `/api/employees/${s.emp1}/photo`, { token: s.admin });
  await shoot('employees.photo.upload', `/api/employees/${s.emp1}/photo`, { file: { name: 'Face.PNG', type: 'image/png', bytes: PNG } });
  out.push({ label: 'employees.photo.download', method: 'GET', path: '/photo', ...(await download(base, `/api/employees/${s.emp1}/photo`, s.admin)) });
  await shoot('employees.photo.wrong-type', `/api/employees/${s.emp1}/photo`, { file: { name: 'notes.txt', type: 'text/plain', bytes: Buffer.from('hello') } });
  await shoot('employees.photo.no-file', `/api/employees/${s.emp1}/photo`, {});
  await shoot('employees.photo.too-large', `/api/employees/${s.emp1}/photo`, { file: { name: 'big.jpg', type: 'image/jpeg', bytes: Buffer.alloc(5 * 1024 * 1024 + 1) } });
  await step('employees.photo.remove', 'DELETE', `/api/employees/${s.emp2}/photo`, { token: s.admin });

  // Documents
  const docPath = `/api/employees/${s.emp1}/documents`;
  await shoot('documents.add.no-title', docPath, { file: { name: 'cv.pdf', type: 'application/pdf', bytes: PDF } });
  await shoot('documents.add.no-file', docPath, { fields: { title: 'Degree' } });
  await shoot('documents.add.wrong-type', docPath, { fields: { title: 'Zip' }, file: { name: 'a.zip', type: 'application/zip', bytes: Buffer.from('PK') } });
  await shoot('documents.add.too-large', docPath, { fields: { title: 'Huge' }, file: { name: 'h.pdf', type: 'application/pdf', bytes: Buffer.alloc(10 * 1024 * 1024 + 1) } });
  r = await shoot('documents.add', docPath, { fields: { title: ' Degree certificate ', category: 'CERTIFICATE', expiryDate: '2030-12-31', notes: ' BSc ' }, file: { name: 'Degree.pdf', type: 'application/pdf', bytes: PDF } });
  s.doc1 = r.body?.id;
  await step('documents.list', 'GET', docPath, { token: s.admin });
  out.push({ label: 'documents.download', method: 'GET', path: '/download', ...(await download(base, `${docPath}/${s.doc1}/download`, s.admin)) });
  out.push({ label: 'documents.download.inline', method: 'GET', path: '/download?inline=1', ...(await download(base, `${docPath}/${s.doc1}/download?inline=1`, s.admin)) });
  r = await shoot('documents.replace', `${docPath}/${s.doc1}/replace`, { fields: { notes: 'Re-issued' }, file: { name: 'Degree v2.pdf', type: 'application/pdf', bytes: PDF2 } });
  s.doc2 = r.body?.id;
  await shoot('documents.replace.old-version', `${docPath}/${s.doc1}/replace`, { file: { name: 'x.pdf', type: 'application/pdf', bytes: PDF2 } });
  await step('documents.update', 'PATCH', `${docPath}/${s.doc2}`, E({ title: 'Degree (certified)', category: 'OTHER' }));
  await step('employees.get.with-documents', 'GET', `/api/employees/${s.emp1}`, { token: s.admin });
  await step('documents.delete.current', 'DELETE', `${docPath}/${s.doc2}`, { token: s.admin });
  await step('documents.list.after-delete', 'GET', docPath, { token: s.admin });
  await step('documents.delete.unknown', 'DELETE', `${docPath}/${FIXED_UUID}`, { token: s.admin });

  // Salaries, increments and bank details (payroll permission)
  const P = (body) => ({ token: s.admin, body });
  const pe = `/api/payroll/employees/${s.emp1}`;
  await step('payroll.employee.empty', 'GET', pe, { token: s.admin });
  await step('salaries.create.invalid', 'POST', `${pe}/salaries`, P({ effectiveFrom: '', basic: 'abc' }));
  r = await step('salaries.create', 'POST', `${pe}/salaries`, P({ effectiveFrom: '2026-04-01', basic: '40500', bra2005: 1000, bra2016: 2500, vehicleAllowance: 15000, otherAllowance: 24850, reason: ' Joined ' }));
  s.sal1 = r.body?.id;
  await step('salaries.create.clash', 'POST', `${pe}/salaries`, P({ effectiveFrom: '2026-04-01', basic: 1 }));
  r = await step('salaries.create.increment', 'POST', `${pe}/salaries`, P({ effectiveFrom: '2027-01-01', basic: 45000, bra2005: 1000, bra2016: 2500, vehicleAllowance: 15000, otherAllowance: 24850, reason: 'Annual increment 2027' }));
  s.sal2 = r.body?.id;
  await step('salaries.update', 'PATCH', `${pe}/salaries/${s.sal2}`, P({ loanInstallment: 2500, notes: 'approved' }));
  await step('salaries.create.contract-default', 'POST', `/api/payroll/employees/${s.emp2}/salaries`, P({ effectiveFrom: '2026-04-01', basic: 30000 }));
  await step('payroll.bank', 'PATCH', `${pe}/bank`, P({ bankName: ' Bank of Ceylon ', bankBranch: '', bankAccountNo: '0071234', staffLoanBalance: '25000.5' }));
  await step('payroll.employee', 'GET', pe, { token: s.admin });
  await step('payroll.employee.as-of', 'GET', `${pe}?asOf=2027-02-01`, { token: s.admin });
  await step('payroll.employee.not-uuid', 'GET', '/api/payroll/employees/abc', { token: s.admin });
  await step('salaries.delete', 'DELETE', `${pe}/salaries/${s.sal2}`, { token: s.admin });
  await step('salaries.delete.unknown', 'DELETE', `${pe}/salaries/${FIXED_UUID}`, { token: s.admin });
  await step('employees.get.with-bank', 'GET', `/api/employees/${s.emp1}`, { token: s.admin });

  // An HR user with `employees` but not `payroll` never sees bank details or salaries.
  r = await call(base, 'POST', '/api/roles', A({ name: 'HR', permissions: ['employees'] }));
  await call(base, 'POST', '/api/users', A({ username: 'hr1', password: 'hrpass123', roleId: r.body?.id }));
  r = await call(base, 'POST', '/api/auth/login', { body: { username: 'hr1', password: 'hrpass123' } });
  const ht = r.body.tempToken;
  const hs = (await call(base, 'GET', '/api/auth/2fa/setup', { token: ht })).body.secret;
  s.hr = (await call(base, 'POST', '/api/auth/2fa/enable', { token: ht, body: { code: totp(hs) } })).body.accessToken;
  await step('hr.employee.redacted', 'GET', `/api/employees/${s.emp1}`, { token: s.hr });
  await step('hr.employees.list.redacted', 'GET', '/api/employees', { token: s.hr });
  await step('hr.payroll.forbidden', 'GET', pe, { token: s.hr });
  await step('viewer.employees.forbidden', 'GET', '/api/employees', { token: s.viewer });
  await step('employees.delete', 'DELETE', `/api/employees/${s.emp2}`, { token: s.admin });
  await step('employees.list.after-delete', 'GET', '/api/employees', { token: s.admin });

  // ── W2: own-invoices-only scope ────────────────────────────────────────────
  r = await call(base, 'POST', '/api/roles', A({ name: 'Sales', permissions: ['invoices'] }));
  await call(base, 'POST', '/api/users', A({ username: 'sales1', password: 'salespass1', roleId: r.body?.id }));
  r = await call(base, 'POST', '/api/auth/login', { body: { username: 'sales1', password: 'salespass1' } });
  const st = r.body.tempToken;
  const ss = (await call(base, 'GET', '/api/auth/2fa/setup', { token: st })).body.secret;
  s.sales = (await call(base, 'POST', '/api/auth/2fa/enable', { token: st, body: { code: totp(ss) } })).body.accessToken;
  r = await step('scope.create', 'POST', '/api/invoices', { token: s.sales, body: { billToName: 'Sales Lead', date: '2026-04-10', lines: [{ description: 'Quote', unitPrice: 75, accountId: rev }] } });
  await step('scope.list.own', 'GET', '/api/invoices', { token: s.sales });
  await step('scope.get.other', 'GET', `/api/invoices/${s.inv1}`, { token: s.sales });
  await step('scope.post.other', 'POST', `/api/invoices/${s.inv1}/post`, { token: s.sales });
  await step('scope.list.admin', 'GET', '/api/invoices?limit=100', { token: s.admin });
  await step('scope.payments', 'GET', '/api/payments', { token: s.sales });

  // D4 in isolation: 0.29 x 0.50 is 0.15 exactly; the reference's float toFixed gives 0.14.
  r = await step('invoices.create.rounding', 'POST', '/api/invoices', A({ billToName: 'Rounding', date: '2026-04-11', lines: [{ description: 'Edge', quantity: 0.29, unitPrice: 0.5 }] }));
  await step('invoices.delete.rounding', 'DELETE', `/api/invoices/${r.body?.id}`, { token: s.admin });

  // Deactivated viewer
  await call(base, 'PATCH', `/api/users/${s.viewerId}`, { token: s.admin, body: { isActive: false } });
  await step('inactive.users', 'GET', '/api/users', { token: s.viewer });
  await step('inactive.me', 'GET', '/api/auth/me', { token: s.viewer });
  await step('inactive.login', 'POST', '/api/auth/login', { body: { username: 'viewer', password: 'viewerpass1' } });


  // Audit trail last, so it covers everything above.
  await step('audit.list', 'GET', '/api/audit-logs?limit=100', { token: s.admin });
  await step('audit.invalid-pagination', 'GET', '/api/audit-logs?page=abc&limit=101', { token: s.admin });
  await step('audit.users', 'GET', '/api/audit-logs/users', { token: s.admin });
  // ── Approved W2 fixes D11–D15, isolated at the end so they cannot affect shared steps ──
  r = await call(base, 'POST', '/api/bills', A({ vendorId: s.vendor, date: '2026-04-13', lines: [{ description: 'Desk', quantity: 1, unitPrice: 57.5, accountId: exp }] }));
  const twoPay = r.body?.id;
  await call(base, 'POST', `/api/bills/${twoPay}/post`, { token: s.admin });
  await step('payments.ap.first-part', 'POST', '/api/payments', pay({ type: 'AP', billId: twoPay, amount: 10 }));
  await step('payments.ap.second-part', 'POST', '/api/payments', pay({ type: 'AP', billId: twoPay, amount: 47.5 }));
  await step('bills.get.two-payments', 'GET', `/api/bills/${twoPay}`, { token: s.admin });
  r = await call(base, 'POST', '/api/bills', A({ vendorId: s.vendor, date: '2026-04-14', lines: [{ description: 'Chair', quantity: 1, unitPrice: 30, accountId: exp }] }));
  const partVoid = r.body?.id;
  await call(base, 'POST', `/api/bills/${partVoid}/post`, { token: s.admin });
  await call(base, 'POST', '/api/payments', pay({ type: 'AP', billId: partVoid, amount: 10 }));
  await step('bills.void.part-paid', 'POST', `/api/bills/${partVoid}/void`, { token: s.admin });
  r = await call(base, 'POST', '/api/bills', A({ vendorId: s.vendor, date: '2026-04-15', lines: [{ description: 'Unposted', quantity: 1, unitPrice: 9, accountId: exp }] }));
  await step('payments.ap.draft-bill', 'POST', '/api/payments', pay({ type: 'AP', billId: r.body?.id, amount: 9 }));
  // These two create records only on the new server, so they come after everything shared.
  await step('customers.create.empty-email', 'POST', '/api/customers', A({ name: 'No Email Ltd', email: '' }));
  await step('bills.create.empty-due-date', 'POST', '/api/bills', A({ vendorId: s.vendor, date: '2026-04-12', dueDate: '', lines: [{ description: 'Pens', quantity: 1, unitPrice: 4, accountId: exp }] }));

  // D10 last: its extra accounts would shift the UUID numbering of everything after it.
  await step('accounts.seed', 'POST', '/api/accounts/seed', { token: s.admin });
  await step('accounts.list.after-seed', 'GET', '/api/accounts', { token: s.admin });
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
  'accounts.update.code-conflict': 'The reference crashed (500) on the duplicate code; here it is a 409 like create',
  'ledger.trial-balance': 'D1: the reversed expense and its reversal both count (reference: cash +250 too high, expense -250)',
  'ledger.balance.cash': 'D1: reversed entry and reversal cancel (correct cash 1100.80; reference 1350.80)',
  'ledger.balance.range': 'D1: April shows the expense; its reversal is dated in October, outside the range',
  'ledger.trial-balance.after-w2': 'D1: reversed invoice/bill/expense entries and their reversals both count',
  'invoices.create.rounding': 'D4: 0.29 x 0.50 = 0.145 rounds half-up to 0.15; the reference float toFixed gives 0.14',
  'bills.void.paid': 'The reference source file garbles the dash in this message (â€”); the intended text is sent',
  'customers.create.empty-email': 'D11: an empty email means no email (the reference rejected "")',
  'bills.create.empty-due-date': 'D12: an empty due date means no due date (the reference rejected "")',
  'bills.get.two-payments': 'D13: two part-payments covering the bill make it PAID (reference: stays POSTED)',
  'bills.void.part-paid': 'D15: a part-paid bill cannot be voided (reference voided it, leaving the payment against nothing)',
  'payments.ap.draft-bill': 'D14: a draft bill must be posted before it can be paid (reference marked it PAID unposted)',
  'reports.balance-sheet.clean': 'D4: the reference sums floats (totalAssets 3687.4300000000003); exact cents give 3687.43',
  'reports.profit-loss.year': 'D1: reversed documents and their reversals both count (reference double-counts reversals)',
  'reports.balance-sheet.today': 'D1: as above; the reference sheet is distorted by every void',
  'reports.profit-loss.bad-date': 'The reference passed the bad date to SQL and crashed (500); here it is a 400',
  'accounts.list.after-seed': 'D10: seeding adds the missing default accounts (1200, 2100, 5000)',
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

// PostgreSQL returns journal lines in no particular order (the reference has no ORDER BY), so
// lines are compared as a set: sorted by their amounts, description and account code.
function sortLines(v) {
  if (Array.isArray(v)) return v.map(sortLines);
  if (!v || typeof v !== 'object') return v;
  const out = Object.fromEntries(Object.entries(v).map(([k, x]) => [k, sortLines(x)]));
  if (Array.isArray(out.items) && out.items.every((i) => i && typeof i === 'object' && 'number' in i)) {
    out.items = [...out.items].sort((x, y) => (x.number < y.number ? -1 : x.number > y.number ? 1 : 0));
  }
  if (Array.isArray(out.lines)) {
    const key = (l) => [l.debit, l.credit, l.description ?? '', l.account?.code ?? ''].join('|');
    out.lines = [...out.lines].sort((x, y) => (key(x) < key(y) ? -1 : key(x) > key(y) ? 1 : 0));
  }
  return out;
}

const [ref, neu] = await Promise.all([scenario(REF), scenario(NEW)]);
const nr = normaliser();
const nn = normaliser();
let failures = 0;
let deviations = 0;
for (let i = 0; i < ref.length; i++) {
  const a = ref[i];
  const b = neu[i];
  const audit = a.label === 'audit.list';
  const left = { status: a.status, body: nr(sortLines(audit ? mutationRows(a.body) : a.body)) };
  const right = { status: b.status, body: nn(sortLines(audit ? mutationRows(b.body) : b.body)) };
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
