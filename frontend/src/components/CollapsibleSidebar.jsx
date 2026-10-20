import { useState } from 'react';
import { NavLink, useLocation } from 'react-router-dom';
import {
  LayoutDashboard,
  Database,
  ArrowLeftRight,
  Calculator,
  BarChart2,
  ShieldCheck,
  Users,
  ChevronRight,
} from 'lucide-react';
import { useAuth } from '../contexts/AuthContext';

// ─── Navigation definition (also used by the menu bar and status bar) ───────
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
      { label: 'Credit Notes', to: '/credit-notes', permission: 'invoices' },
      { label: 'Debit Notes',  to: '/debit-notes',  permission: 'bills' },
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

/** [section, page] for the current path, for the status bar. */
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

function groupIsActive(group, pathname) {
  if (group.standalone) return pathname === group.to;
  return group.items?.some((item) => pathname === item.to || pathname.startsWith(item.to + '/'));
}

// Folded groups are remembered between sessions, like a tree in Explorer.
function loadFolded() {
  try {
    return new Set(JSON.parse(localStorage.getItem('navFolded') || '[]'));
  } catch {
    return new Set();
  }
}

const rowClass = (isActive) =>
  `desk-nav-row ${isActive ? 'is-active' : ''}`;

// ─── Navigation pane ─────────────────────────────────────────────────────────
export default function CollapsibleSidebar() {
  const { pathname } = useLocation();
  const { hasPermission } = useAuth();
  const [folded, setFolded] = useState(loadFolded);

  const toggle = (id) =>
    setFolded((prev) => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      try {
        localStorage.setItem('navFolded', JSON.stringify([...next]));
      } catch {
        /* only a convenience */
      }
      return next;
    });

  return (
    <aside className="desk-nav" aria-label="Navigation pane">
      <nav className="nav-scroll flex-1 overflow-y-auto py-1.5">
        {filterNavigation(navigation, hasPermission).map((group) => {
          const Icon = group.icon;
          if (group.standalone) {
            return (
              <NavLink key={group.id} to={group.to} end className={({ isActive }) => rowClass(isActive)}>
                <span className="w-3.5" />
                <Icon size={15} strokeWidth={1.75} className="desk-nav-icon" />
                {group.label}
              </NavLink>
            );
          }
          const open = !folded.has(group.id) || groupIsActive(group, pathname);
          return (
            <div key={group.id}>
              <button type="button" className="desk-nav-row desk-nav-group" onClick={() => toggle(group.id)} aria-expanded={open}>
                <ChevronRight size={13} className={`text-gray-500 transition-transform ${open ? 'rotate-90' : ''}`} />
                <Icon size={15} strokeWidth={1.75} className="desk-nav-icon" />
                {group.label}
              </button>
              {open &&
                group.items.map((item) => (
                  <NavLink key={item.to} to={item.to} className={({ isActive }) => `${rowClass(isActive)} pl-[2.6rem]`}>
                    {item.label}
                  </NavLink>
                ))}
            </div>
          );
        })}
      </nav>
      <div className="px-3 py-2 border-t border-[#e0e0e0] shrink-0">
        <img src="/Logo_black.png" alt="Cenango" className="h-4 w-auto opacity-70" />
      </div>
    </aside>
  );
}
