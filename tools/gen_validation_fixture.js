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
      const type = name === 'PaginationDto' ? 'query' : 'body';
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
