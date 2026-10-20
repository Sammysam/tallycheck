import { useState } from 'react';
import { NavLink, useLocation, useNavigate } from 'react-router-dom';
import {
  LayoutDashboard,
  Database,
  ArrowLeftRight,
  Calculator,
  BarChart2,
  ShieldCheck,
  Users,
  ChevronDown,
  LogOut,
} from 'lucide-react';
import { useAuth } from '../contexts/AuthContext';

// ─── Navigation definition (also used by the top bar for page titles) ───────
export const navigation = [
  { id: 'dashboard', label: 'Dashboard', icon: LayoutDashboard, to: '/', standalone: true, permission: 'dashboard' },
  {
    id: 'master-data',
    label: 'Master Data',
    icon: Database,
    items: [
      { label: 'Accounts',  to: '/accounts',  permission: 'accounts' },
      { label: 'Customers', to: '/customers', permission: 'customers' },
      { label: 'Vendors',   to: '/vendors',   permission: 'vendors' },
    ],
  },
  {
    id: 'people',
    label: 'People',
    icon: Users,
    items: [
      { label: 'Employees', to: '/employees', permission: 'employees' },
      { label: 'Payroll',   to: '/payroll',   permission: 'payroll' },
    ],
  },
  {
    id: 'transactions',
    label: 'Transactions',
    icon: ArrowLeftRight,
    items: [
      { label: 'Invoices', to: '/invoices', permission: 'invoices' },
      { label: 'Bills',    to: '/bills',    permission: 'bills' },
      { label: 'Expenses', to: '/expenses', permission: 'expenses' },
      { label: 'Payments', to: '/payments', permission: 'payments' },
    ],
  },
  {
    id: 'accounting',
    label: 'Accounting',
    icon: Calculator,
    items: [
      { label: 'Ledger', to: '/ledger', permission: 'ledger' },
    ],
  },
  {
    id: 'reports',
    label: 'Reports',
    icon: BarChart2,
    items: [
      { label: 'Trial Balance', to: '/reports/trial-balance', permission: 'reports' },
      { label: 'Profit & Loss', to: '/reports/profit-loss',   permission: 'reports' },
      { label: 'Balance Sheet', to: '/reports/balance-sheet', permission: 'reports' },
      { label: 'AR Aging',      to: '/reports/ar-aging',      permission: 'reports' },
      { label: 'AP Aging',      to: '/reports/ap-aging',      permission: 'reports' },
    ],
  },
  {
    id: 'administration',
    label: 'Administration',
    icon: ShieldCheck,
    items: [
      { label: 'Users', to: '/users', permission: 'users' },
      { label: 'Roles', to: '/roles', permission: 'users' },
      { label: 'Activity Log', to: '/audit', permission: 'audit' },
      { label: 'Settings', to: '/settings', permission: 'settings' },
    ],
  },
];

/** [section, page] for the current path, for the top bar. */
export function pageTrail(pathname) {
  for (const group of navigation) {
    if (group.standalone) {
      if (pathname === group.to) return [null, group.label];
      continue;
    }
    const item = group.items.find((i) => pathname === i.to || pathname.startsWith(i.to + '/'));
    if (item) return [group.label, item.label];
  }
  if (pathname.startsWith('/2fa')) return [null, 'Two-factor sign-in'];
  return [null, ''];
}

export function initialsOf(name) {
  const parts = (name || '?').split(/[\s._-]+/).filter(Boolean);
  return ((parts[0]?.[0] ?? '?') + (parts[1]?.[0] ?? '')).toUpperCase();
}

// ─── Permission filtering ───────────────────────────────────────────────────
function filterNavigation(items, hasPermission) {
  return items
    .map((group) => {
      if (group.standalone) return group;
      const visibleItems = group.items.filter((item) => hasPermission(item.permission));
      return { ...group, items: visibleItems };
    })
    .filter((group) => (group.standalone ? hasPermission(group.permission) : group.items.length > 0));
}

/** Returns true if any child route of a group matches the current path */
function groupIsActive(group, pathname) {
  if (group.standalone) return pathname === group.to;
  return group.items?.some((item) =>
    pathname === item.to || pathname.startsWith(item.to + '/'),
  );
}

// ─── Sub-item ───────────────────────────────────────────────────────────────
function SubItem({ item }) {
  return (
    <NavLink
      to={item.to}
      className={({ isActive }) =>
        `relative flex items-center ml-[1.375rem] pl-5 pr-3 py-1.5 rounded-lg text-[13px] transition-colors duration-150 ${
          isActive
            ? 'font-medium text-white bg-white/10'
            : 'text-slate-400 hover:text-white hover:bg-white/5'
        }`
      }
    >
      {({ isActive }) => (
        <>
          <span
            className={`absolute left-0 top-1/2 -translate-y-1/2 w-[3px] rounded-full transition-all ${
              isActive ? 'h-4 bg-orange-500' : 'h-0'
            }`}
          />
          {item.label}
        </>
      )}
    </NavLink>
  );
}

// ─── Group row (collapsible) ─────────────────────────────────────────────────
function GroupRow({ group, isOpen, onToggle, isAnyChildActive }) {
  const Icon = group.icon;
  return (
    <button
      onClick={onToggle}
      className={`w-full flex items-center gap-3 px-3 py-2 rounded-lg text-sm font-medium transition-colors duration-150 ${
        isAnyChildActive ? 'text-white' : 'text-slate-300 hover:text-white hover:bg-white/5'
      }`}
    >
      <Icon size={17} className={isAnyChildActive ? 'text-orange-400' : 'text-slate-400'} strokeWidth={2} />
      <span className="flex-1 text-left">{group.label}</span>
      <ChevronDown
        size={14}
        className={`text-slate-500 transition-transform duration-200 ${isOpen ? 'rotate-180' : 'rotate-0'}`}
      />
    </button>
  );
}

// ─── Standalone row (Dashboard) ──────────────────────────────────────────────
function StandaloneRow({ group }) {
  const Icon = group.icon;
  return (
    <NavLink
      to={group.to}
      end
      className={({ isActive }) =>
        `flex items-center gap-3 px-3 py-2 rounded-lg text-sm font-medium transition-colors duration-150 ${
          isActive
            ? 'text-white bg-gradient-to-r from-orange-500 to-orange-600 shadow-lg shadow-orange-900/30'
            : 'text-slate-300 hover:text-white hover:bg-white/5'
        }`
      }
    >
      {({ isActive }) => (
        <>
          <Icon size={17} className={isActive ? 'text-white' : 'text-slate-400'} strokeWidth={2} />
          <span className="flex-1">{group.label}</span>
        </>
      )}
    </NavLink>
  );
}

// ─── Main component ──────────────────────────────────────────────────────────
export default function CollapsibleSidebar() {
  const location = useLocation();
  const navigate = useNavigate();
  const { user, hasPermission, logout } = useAuth();

  const visibleNavigation = filterNavigation(navigation, hasPermission);

  // Which group is currently open — start with the one that has the active route
  const initialOpen = visibleNavigation.find(
    (g) => !g.standalone && groupIsActive(g, location.pathname),
  )?.id ?? null;

  const [openGroup, setOpenGroup] = useState(initialOpen);
  const toggle = (id) => setOpenGroup((prev) => (prev === id ? null : id));

  return (
    <aside className="w-64 shrink-0 flex flex-col bg-slate-900 h-full">
      {/* ── Brand ── */}
      <div className="px-5 pt-5 pb-4 shrink-0">
        <img src="/Logo_black.png" alt="Cenango" className="sidebar-logo h-6 w-auto max-w-full object-contain" />
        <span className="mt-1.5 block text-[11px] font-medium uppercase tracking-[0.18em] text-orange-400">
          Accounting System
        </span>
      </div>

      {/* ── Navigation ── */}
      <nav className="nav-scroll flex-1 overflow-y-auto px-3 pb-3 space-y-0.5">
        <p className="px-3 pt-2 pb-1.5 text-[10px] font-semibold uppercase tracking-[0.14em] text-slate-500">Menu</p>
        {visibleNavigation.map((group) => {
          if (group.standalone) {
            return <StandaloneRow key={group.id} group={group} />;
          }
          const isOpen = openGroup === group.id;
          const anyChildActive = groupIsActive(group, location.pathname);
          return (
            <div key={group.id}>
              <GroupRow
                group={group}
                isOpen={isOpen}
                onToggle={() => toggle(group.id)}
                isAnyChildActive={anyChildActive}
              />
              <div
                className={`grid transition-all duration-200 ease-in-out ${
                  isOpen ? 'grid-rows-[1fr] opacity-100' : 'grid-rows-[0fr] opacity-0'
                }`}
              >
                <div className="overflow-hidden">
                  <div className="relative pt-0.5 pb-1.5 space-y-0.5 before:absolute before:left-[1.375rem] before:top-1 before:bottom-2 before:w-px before:bg-slate-700/70">
                    {group.items.map((item) => (
                      <SubItem key={item.to} item={item} />
                    ))}
                  </div>
                </div>
              </div>
            </div>
          );
        })}
      </nav>

      {/* ── Signed-in user ── */}
      <div className="m-3 p-2.5 rounded-xl bg-white/5 border border-white/5 shrink-0">
        <div className="flex items-center gap-2.5">
          <div className="w-9 h-9 rounded-full bg-gradient-to-br from-orange-400 to-orange-600 flex items-center justify-center shrink-0 shadow">
            <span className="text-white text-xs font-bold">{initialsOf(user?.username)}</span>
          </div>
          <div className="flex flex-col leading-tight min-w-0 flex-1">
            <span className="text-sm font-medium text-white truncate">{user?.username ?? ''}</span>
            <span className="text-[11px] text-slate-400 truncate">{user?.role || 'No role'}</span>
          </div>
          <button
            type="button"
            onClick={() => { logout(); navigate('/login'); }}
            title="Sign out"
            aria-label="Sign out"
            className="p-2 rounded-lg text-slate-400 hover:text-white hover:bg-white/10 transition-colors"
          >
            <LogOut size={16} />
          </button>
        </div>
      </div>
    </aside>
  );
}
