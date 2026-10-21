/**
 * D36: the permission keys in plain words, grouped as the menu is. The keys themselves come from
 * the server (`/roles/permissions`); any key missing here still shows, under "Other".
 */
export interface PermissionInfo { key: string; label: string; description: string }
export interface PermissionGroup { title: string; items: PermissionInfo[] }

export const PERMISSION_GROUPS: PermissionGroup[] = [
  { title: 'General', items: [
    { key: 'dashboard', label: 'Dashboard', description: 'See the year-to-date figures and what needs attention.' },
  ] },
  { title: 'Sales', items: [
    { key: 'customers', label: 'Customers', description: 'Add and change customers; open their statements.' },
    { key: 'invoices', label: 'Invoices', description: 'Create, post, print and void their own invoices; credit notes.' },
    { key: 'invoices:all', label: "Everyone's invoices", description: 'See and work on invoices made by other users too.' },
  ] },
  { title: 'Purchases', items: [
    { key: 'vendors', label: 'Vendors', description: 'Add and change suppliers.' },
    { key: 'bills', label: 'Bills', description: 'Enter, post and void supplier bills; debit notes.' },
    { key: 'expenses', label: 'Expenses', description: 'Record costs paid on the spot.' },
    { key: 'payments', label: 'Payments', description: 'Record and void money received and paid.' },
  ] },
  { title: 'Accounting', items: [
    { key: 'accounts', label: 'Chart of accounts', description: 'Add, edit and delete accounts; revalue foreign-currency accounts.' },
    { key: 'ledger', label: 'Ledger', description: 'See journal entries and transactions; post and reverse manual entries.' },
    { key: 'banking', label: 'Banking', description: 'Reconcile bank accounts to their statements.' },
    { key: 'reports', label: 'Reports', description: 'Profit & Loss, Balance Sheet, Cash Flow, ageing, tax return.' },
  ] },
  { title: 'People & payroll', items: [
    { key: 'employees', label: 'Employees', description: 'Employee profiles, photos and documents.' },
    { key: 'payroll', label: 'Payroll', description: 'Salaries, increments, bank details and monthly payroll runs.' },
  ] },
  { title: 'Administration', items: [
    { key: 'users', label: 'Users & roles', description: 'Add users, reset passwords and 2FA, change roles.' },
    { key: 'audit', label: 'Activity log', description: 'See who did what, and when.' },
    { key: 'settings', label: 'Settings', description: 'Company details, taxes, locking the books.' },
  ] },
];

const ALL = new Map(PERMISSION_GROUPS.flatMap((g) => g.items).map((p) => [p.key, p]));

/** The plain-words label for a permission key. */
export const permissionLabel = (key: string) => ALL.get(key)?.label ?? key;

/** The groups, with any server key this file does not know added under "Other". */
export function groupsFor(catalog: { key: string; label: string }[]): PermissionGroup[] {
  const unknown = catalog.filter((c) => !ALL.has(c.key)).map((c) => ({ key: c.key, label: c.label, description: '' }));
  const known = new Set(catalog.map((c) => c.key));
  const groups = PERMISSION_GROUPS.map((g) => ({ ...g, items: g.items.filter((i) => known.has(i.key)) })).filter((g) => g.items.length);
  return unknown.length ? [...groups, { title: 'Other', items: unknown }] : groups;
}
