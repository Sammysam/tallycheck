// Runs the reference's real DTO classes through Nest's ValidationPipe (whitelist + transform,
// exactly as main.ts configures it) and records the outcome for each input.
// Usage: node gen_validation_fixture.js <reference backend dir> <output json>
const path = require('path');
const fs = require('fs');
const ref = process.argv[2];
const req = (m) => require(path.join(ref, 'node_modules', m));
// In-memory TypeScript transpilation (nothing is written to the reference tree).
const ts = req('typescript');
require.extensions['.ts'] = (module, filename) => {
  const out = ts.transpileModule(fs.readFileSync(filename, 'utf8'), {
    fileName: filename,
    compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2021, experimentalDecorators: true, emitDecoratorMetadata: true, esModuleInterop: true },
  });
  module._compile(out.outputText, filename);
};
module.paths.unshift(path.join(ref, 'node_modules'));
require('module').Module._initPaths && 0;
process.env.NODE_PATH = path.join(ref, 'node_modules'); require('module').Module._initPaths();
req('reflect-metadata');
const { ValidationPipe } = req('@nestjs/common');
const dto = (p, name) => require(path.join(ref, 'src', p))[name];
const pipe = new ValidationPipe({ whitelist: true, transform: true });

const cases = {
  CreateUserDto: [dto('modules/users/dto/create-user.dto.ts', 'CreateUserDto'), [
    {}, { username: 'ab', password: 'short' }, { username: 'bad name!', password: 'longenough' },
    { username: 'alice', password: 'longenough' }, { username: 'alice', password: 'longenough', roleId: 'nope' },
    { username: 'alice', password: 'longenough', roleId: '7c9e6679-7425-40de-944b-e07fc1f90ae7', extra: 1 },
    { username: 123, password: 12345678 }, { username: null, password: null }, { username: 'alice', password: 'longenough', roleId: null },
    { username: 'alice', password: 'longenough', roleId: '' }, { username: 'a.b_c-d', password: '😀😀😀😀😀😀😀😀' },
    { username: '', password: '' }, { username: 'alice', password: 'longenough', roleId: '11111111-1111-1111-1111-111111111111' },
  ]],
  UpdateUserDto: [dto('modules/users/dto/update-user.dto.ts', 'UpdateUserDto'), [
    {}, { username: 'x' }, { isActive: 'true' }, { isActive: false }, { roleId: 'x', isActive: 1 }, { username: null, roleId: null, isActive: null },
    { username: 'good.name', other: 'stripped' },
  ]],
  ResetPasswordDto: [dto('modules/users/dto/reset-password.dto.ts', 'ResetPasswordDto'), [
    {}, { password: 'short' }, { password: 'longenough' }, { password: 5 },
  ]],
  CreateRoleDto: [dto('modules/roles/dto/create-role.dto.ts', 'CreateRoleDto'), [
    {}, { name: '', permissions: [] }, { name: 'Clerk', permissions: ['invoices', 'invoices'] }, { name: 'Clerk', permissions: ['nope'] },
    { name: 'Clerk', permissions: 'invoices' }, { name: 'Clerk', permissions: ['invoices', 'bills'] }, { name: 5, permissions: [1] },
    { name: 'Clerk', permissions: ['nope', 'nope'] }, { name: 'Clerk', permissions: null }, { name: 'Clerk', permissions: [] },
  ]],
  UpdateRoleDto: [dto('modules/roles/dto/update-role.dto.ts', 'UpdateRoleDto'), [
    {}, { name: '' }, { permissions: ['x'] }, { name: 'A', permissions: ['audit'] }, { name: null }, { permissions: null },
  ]],
  UpdateSettingsDto: [dto('modules/settings/dto/update-settings.dto.ts', 'UpdateSettingsDto'), [
    {}, { signatoryName: 'x'.repeat(121) }, { signatoryName: 5, signatoryTitle: 'T' }, { signatoryName: 'Ok', junk: 1 },
    { signatoryName: null }, { signatoryTitle: 'x'.repeat(120) },
  ]],
  CreateAccountDto: [dto('modules/accounts/dto/create-account.dto.ts', 'CreateAccountDto'), [
    {}, { code: '', name: '', type: 'INCOME' }, { code: 1000, name: 5, type: 'ASSET' }, { code: '1000', name: 'Cash', type: 'ASSET' },
    { code: '1000', name: 'Cash', type: 'ASSET', subtype: 'current_asset', description: 'Petty', parentId: 'nope', extra: 1 },
    { code: '1000', name: 'Cash', type: 'asset' }, { code: '1000', name: 'Cash', type: 'ASSET', subtype: null, description: null, parentId: null },
    { code: '1000', name: 'Cash', type: 'ASSET', subtype: 5, parentId: '7c9e6679-7425-40de-944b-e07fc1f90ae7' },
  ]],
  UpdateAccountDto: [dto('modules/accounts/dto/update-account.dto.ts', 'UpdateAccountDto'), [
    {}, { name: '' }, { type: 'X' }, { name: 'Bank', isActive: false }, { code: null }, { code: '1001', parentId: 'x' },
  ]],
  CreateJournalEntryDto: [dto('modules/ledger/dto/create-journal-entry.dto.ts', 'CreateJournalEntryDto'), [
    {}, { date: '2026-02-30', description: '', lines: [] },
    { date: '2026-04-01', description: 'x', lines: 'nope' },
    { date: '2026-04-01', description: 'x', lines: [1, 2] },
    { date: '2026-04-01', description: 'x', lines: [{}] },
    { date: '2026-04-01', description: 'x', lines: [{}, {}] },
    { date: '2026-04-01', description: 'x', lines: [{ accountId: 'bad', debit: '10', credit: -1 }, { accountId: '7c9e6679-7425-40de-944b-e07fc1f90ae7', debit: 1.005, credit: 0, description: 5 }] },
    { date: '2026-04-01', description: 'x', reference: 5, sourceType: null, lines: [{ accountId: '7c9e6679-7425-40de-944b-e07fc1f90ae7', debit: 10.5, credit: 0, junk: 1 }, { accountId: '7c9e6679-7425-40de-944b-e07fc1f90ae7', debit: 0, credit: 10.5 }] },
    { date: '2026-04-01', description: 'ok', reference: 'R', sourceType: 'MANUAL', sourceId: 'S', extra: true, lines: [{ accountId: '7c9e6679-7425-40de-944b-e07fc1f90ae7', debit: 100, credit: 0, description: 'd' }, { accountId: '7c9e6679-7425-40de-944b-e07fc1f90ae7', debit: 0, credit: 100 }] },
    { date: '2026-04-01', description: 'x', lines: [{ accountId: '7c9e6679-7425-40de-944b-e07fc1f90ae7', debit: null, credit: 0 }, null] },
    { date: '2026-04-01', description: 'x', lines: { a: 1 } },
  ]],
  LedgerFilterDto: [dto('modules/ledger/dto/ledger-filter.dto.ts', 'LedgerFilterDto'), [
    {}, { dateFrom: '2026-04-01', dateTo: '2026-04-30', sourceType: 'MANUAL' }, { dateFrom: 'x' }, { dateTo: '2026-13-01', sourceType: ['a', 'b'] }, { junk: '1' },
  ]],
  PaginationDto: [dto('common/dto/pagination.dto.ts', 'PaginationDto'), [
    {}, { page: '2', limit: '50' }, { page: '0' }, { limit: '101' }, { page: 'abc' }, { page: '1.5' }, { page: '' }, { limit: '0' },
    { page: ['1', '2'] }, { page: '-1', limit: 'x' }, { other: 'y' }, { page: ' 3 ' }, { page: '1e1' },
  ]],
};

(async () => {
  const out = {};
  for (const [name, [cls, inputs]] of Object.entries(cases)) {
    out[name] = [];
    for (const input of inputs) {
      const type = ['PaginationDto', 'LedgerFilterDto'].includes(name) ? 'query' : 'body';
      try {
        const value = await pipe.transform(JSON.parse(JSON.stringify(input)), { type, metatype: cls });
        out[name].push({ input, ok: JSON.parse(JSON.stringify(value)) });
      } catch (e) {
        out[name].push({ input, status: e.getStatus(), message: e.getResponse().message });
      }
    }
  }
  require('fs').writeFileSync(process.argv[3], JSON.stringify(out, null, 1));
  console.log(Object.entries(out).map(([k, v]) => `${k}:${v.length}`).join(' '));
})().catch((e) => { console.error(e); process.exit(1); });
