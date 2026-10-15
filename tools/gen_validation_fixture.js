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
  CreateCustomerDto: [dto('modules/customers/dto/create-customer.dto.ts', 'CreateCustomerDto'), [
    {}, { name: '' }, { name: 'Acme', email: 'not-an-email' }, { name: 'Acme', email: 'a@b.co', phone: '011', address: 'x', taxId: 'T', junk: 1 },
    { name: 5, email: 5, phone: 5, address: 5, taxId: 5 }, { name: 'Acme', email: null, phone: null }, { name: 'Acme', email: '' },
  ]],
  UpdateCustomerDto: [dto('modules/customers/dto/update-customer.dto.ts', 'UpdateCustomerDto'), [
    {}, { name: '' }, { email: 'x@y' }, { name: 'New', isActive: false },
  ]],
  CreateVendorDto: [dto('modules/vendors/dto/create-vendor.dto.ts', 'CreateVendorDto'), [
    {}, { name: 'Supplier', email: 'billing@supplier.lk' }, { name: 'S', email: 'bad@' },
  ]],
  CreateBankAccountDto: [dto('modules/banking/dto/create-bank-account.dto.ts', 'CreateBankAccountDto'), [
    {}, { name: 'BOC', accountId: 'x' }, { name: 'BOC', bankName: 'Bank of Ceylon', accountNumber: '123', accountId: '7c9e6679-7425-40de-944b-e07fc1f90ae7', currentBalance: 5 },
  ]],
  UpdateBankAccountDto: [dto('modules/banking/dto/update-bank-account.dto.ts', 'UpdateBankAccountDto'), [
    {}, { name: '' }, { accountId: 'x', bankName: 5 },
  ]],
  CreateInvoiceDto: [dto('modules/invoices/dto/create-invoice.dto.ts', 'CreateInvoiceDto'), [
    {}, { date: '2026-04-01', lines: [] }, { date: '2026-04-01', lines: [{}] },
    { date: '2026-04-01', lines: [{ description: 'Work', quantity: 0, unitPrice: -1, accountId: 'x' }] },
    { date: '2026-04-01', lines: [{ description: 'Work', quantity: 1.005, unitPrice: 10.001 }] },
    { customerId: 'x', date: 'bad', dueDate: 'bad', paymentTerms: 'NET_90', poNumber: 5, notes: 5, billToName: 'x'.repeat(161), billToAddress: 'y'.repeat(501), paymentOptions: 'cash', signatoryName: 'z'.repeat(121), signatoryTitle: 5, lines: [{ description: 'a' }] },
    { date: '2026-04-01', paymentOptions: ['Bank', 5, 'w'.repeat(301)], lines: [{ description: 'a' }] },
    { customerId: '7c9e6679-7425-40de-944b-e07fc1f90ae7', date: '2026-04-01', dueDate: '2026-05-01', paymentTerms: 'NET_30', poNumber: 'PO1', notes: 'n', billToName: 'B', billToAddress: 'A', paymentOptions: ['Bank', 'Cash'], signatoryName: 'S', signatoryTitle: 'T', junk: 1, lines: [{ description: 'Design', quantity: 2.5, unitPrice: 1000.25, accountId: '7c9e6679-7425-40de-944b-e07fc1f90ae7', junk: 2 }, { description: 'Free' }] },
    { date: '2026-04-01', lines: 'x' }, { date: '2026-04-01', lines: [null] },
  ]],
  UpdateInvoiceDto: [dto('modules/invoices/dto/update-invoice.dto.ts', 'UpdateInvoiceDto'), [
    {}, { poNumber: 'PO-9' }, { lines: [] }, { lines: [{ description: '' }] }, { paymentTerms: null, dueDate: null, customerId: null }, { status: 'PAID' },
  ]],
  ChangeInvoiceStatusDto: [dto('modules/invoices/dto/change-status.dto.ts', 'ChangeInvoiceStatusDto'), [
    {}, { status: 'POSTED' }, { status: 'posted' },
  ]],
  CreateBillDto: [dto('modules/bills/dto/create-bill.dto.ts', 'CreateBillDto'), [
    {}, { vendorId: 'x', date: '2026-04-01', lines: [] },
    { vendorId: '7c9e6679-7425-40de-944b-e07fc1f90ae7', date: '2026-04-01', lines: [{ description: 'Paper' }] },
    { vendorId: '7c9e6679-7425-40de-944b-e07fc1f90ae7', date: '2026-04-01', dueDate: '2026-05-01', notes: 'n', lines: [{ description: 'Paper', quantity: 3, unitPrice: 12.5, accountId: '7c9e6679-7425-40de-944b-e07fc1f90ae7' }] },
    { vendorId: '7c9e6679-7425-40de-944b-e07fc1f90ae7', date: '2026-04-01', lines: [{ description: 'P', quantity: 0.0001, unitPrice: 1, accountId: '7c9e6679-7425-40de-944b-e07fc1f90ae7' }] },
  ]],
  CreateExpenseDto: [dto('modules/expenses/dto/create-expense.dto.ts', 'CreateExpenseDto'), [
    {}, { date: '2026-04-01', description: 'Taxi', amount: 0, expenseAccountId: '7c9e6679-7425-40de-944b-e07fc1f90ae7', paymentAccountId: '7c9e6679-7425-40de-944b-e07fc1f90ae7' },
    { date: '2026-04-01', description: 'Taxi', amount: 1500.75, expenseAccountId: '7c9e6679-7425-40de-944b-e07fc1f90ae7', paymentAccountId: '7c9e6679-7425-40de-944b-e07fc1f90ae7', receiptUrl: 'r', junk: 1 },
    { date: '2026-04-01', description: 'Taxi', amount: '15', expenseAccountId: 'x', paymentAccountId: 'y', receiptUrl: 5 },
  ]],
  CreatePaymentDto: [dto('modules/payments/dto/create-payment.dto.ts', 'CreatePaymentDto'), [
    {}, { type: 'AR', date: '2026-04-01', amount: 0.001, paymentMethod: 'CARD', paymentAccountId: 'x' },
    { type: 'AR', invoiceId: '7c9e6679-7425-40de-944b-e07fc1f90ae7', customerId: '7c9e6679-7425-40de-944b-e07fc1f90ae7', date: '2026-04-01', amount: 100, paymentMethod: 'BANK_TRANSFER', paymentAccountId: '7c9e6679-7425-40de-944b-e07fc1f90ae7', notes: 'n', junk: 1 },
    { type: 'AP', billId: 'x', vendorId: 'y', date: '2026-04-01', amount: 1, paymentMethod: 'CASH', paymentAccountId: '7c9e6679-7425-40de-944b-e07fc1f90ae7' },
  ]],
  QueryInvoicesDto: [dto('modules/invoices/dto/query-invoices.dto.ts', 'QueryInvoicesDto'), [
    {}, { q: ' INV ', status: 'POSTED', customerId: '7c9e6679-7425-40de-944b-e07fc1f90ae7', dateFrom: '2026-04-01', dateTo: '2026-04-30', page: '2', limit: '10' },
    { q: 'x'.repeat(121), status: 'OPEN', customerId: 'x', dateFrom: 'x' }, { page: '0' }, { status: 'OPEN', limit: '500', page: 'x' },
  ]],
  CreateEmployeeDto: [dto('modules/employees/dto/create-employee.dto.ts', 'CreateEmployeeDto'), [
    {}, { employeeId: '   ', name: '  ' }, { employeeId: ' CEN-001 ', name: ' Asha Perera ', junk: 1 },
    { employeeId: 'CEN-001', name: 'A', jobTitle: '  ', department: ' Ops ', email: ' a@b.lk ', phone: '', joinDate: ' 2024-01-15 ', notes: '  ', epfNumber: ' 12 ', nationalId: '', resignedDate: '', userId: '' },
    { employeeId: 'x'.repeat(51), name: 'y'.repeat(201), jobTitle: 5, email: 'bad', phone: 'p'.repeat(51), joinDate: '2024-02-30', status: 'GONE', employmentType: 'TEMP', userId: 'nope' },
    { employeeId: 'CEN-2', name: 'B', status: 'ON_LEAVE', employmentType: 'CONTRACT', email: null, joinDate: null },
    { employeeId: 5, name: 6 },
  ]],
  UpdateEmployeeDto: [dto('modules/employees/dto/update-employee.dto.ts', 'UpdateEmployeeDto'), [
    {}, { name: '  ' }, { jobTitle: '' }, { employeeId: ' cen-9 ', status: 'LEFT', resignedDate: '2026-03-31' }, { bankName: 'x', staffLoanBalance: 5 },
  ]],
  QueryEmployeesDto: [dto('modules/employees/dto/query-employees.dto.ts', 'QueryEmployeesDto'), [
    {}, { q: ' asha ', status: 'ACTIVE' }, { status: 'x' }, { q: ['a', 'b'] },
  ]],
  UploadDocumentDto: [dto('modules/employees/dto/upload-document.dto.ts', 'UploadDocumentDto'), [
    {}, { title: '  ' }, { title: ' Degree ', category: 'CERTIFICATE', expiryDate: ' 2030-01-01 ', notes: ' n ' },
    { title: 'x'.repeat(201), category: 'PASSPORT', expiryDate: 'soon', notes: 5 }, { title: 'T', expiryDate: '', notes: '' },
  ]],
  UpdateBankDto: [dto('modules/payroll/dto/update-bank.dto.ts', 'UpdateBankDto'), [
    {}, { bankName: ' BOC ', bankBranch: '', bankAccountNo: ' 0071 ', staffLoanBalance: '25000.5' },
    { bankName: 'x'.repeat(101), bankAccountNo: 'y'.repeat(51), staffLoanBalance: -1 }, { staffLoanBalance: 'abc' }, { staffLoanBalance: 1.005 },
    { bankName: null, staffLoanBalance: null },
  ]],
  CreateSalaryDto: [dto('modules/payroll/dto/create-salary.dto.ts', 'CreateSalaryDto'), [
    {}, { effectiveFrom: '', basic: '' }, { effectiveFrom: '2026-04-01', basic: '40500', bra2005: 1000, bra2016: '2500', vehicleAllowance: 15000, otherAllowance: 24850, junk: 1 },
    { effectiveFrom: '2026-04-01', basic: 40500, epfApplicable: 'yes', welfareDeduction: -1, loanInstallment: 1.234, reason: ' Joined ', notes: '  ' },
    { effectiveFrom: '2026-04-01', basic: 100, epfApplicable: false, reason: 'r'.repeat(201) }, { effectiveFrom: '2026-04-01', basic: null },
  ]],
  UpdateSalaryDto: [dto('modules/payroll/dto/update-salary.dto.ts', 'UpdateSalaryDto'), [
    {}, { basic: '45000' }, { effectiveFrom: 'x' }, { reason: '' },
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
      const type = ['PaginationDto', 'LedgerFilterDto', 'QueryInvoicesDto', 'QueryEmployeesDto'].includes(name) ? 'query' : 'body';
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
