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
  ChevronDown,
} from 'lucide-react';
import { useAuth } from '../contexts/AuthContext';

// ─── Navigation definition ─────────────────────────────────────────────────
const navigation = [
  {
    id: 'dashboard',
    label: 'Dashboard',
    icon: LayoutDashboard,
    iconBg: 'bg-teal-100',
    iconColor: 'text-teal-600',
    accentColor: 'bg-teal-500',
    borderColor: 'border-teal-500',
    to: '/',
    standalone: true,
    permission: 'dashboard',
  },
  {
    id: 'master-data',
    label: 'Master Data',
    icon: Database,
    iconBg: 'bg-blue-100',
    iconColor: 'text-blue-600',
    accentColor: 'bg-blue-500',
    borderColor: 'border-blue-500',
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
    iconBg: 'bg-emerald-100',
    iconColor: 'text-emerald-600',
    accentColor: 'bg-emerald-500',
    borderColor: 'border-emerald-500',
    items: [
      { label: 'Employees', to: '/employees', permission: 'employees' },
      { label: 'Payroll',   to: '/payroll',   permission: 'payroll' },
    ],
  },
  {
    id: 'transactions',
    label: 'Transactions',
    icon: ArrowLeftRight,
    iconBg: 'bg-orange-100',
    iconColor: 'text-orange-600',
    accentColor: 'bg-orange-500',
    borderColor: 'border-orange-500',
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
    iconBg: 'bg-slate-100',
    iconColor: 'text-slate-600',
    accentColor: 'bg-slate-500',
    borderColor: 'border-slate-500',
    items: [
      { label: 'Ledger', to: '/ledger', permission: 'ledger' },
    ],
  },
  {
    id: 'reports',
    label: 'Reports',
    icon: BarChart2,
    iconBg: 'bg-violet-100',
    iconColor: 'text-violet-600',
    accentColor: 'bg-violet-500',
    borderColor: 'border-violet-500',
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
    iconBg: 'bg-rose-100',
    iconColor: 'text-rose-600',
    accentColor: 'bg-rose-500',
    borderColor: 'border-rose-500',
    items: [
      { label: 'Users', to: '/users', permission: 'users' },
      { label: 'Roles', to: '/roles', permission: 'users' },
      { label: 'Activity Log', to: '/audit', permission: 'audit' },
      { label: 'Settings', to: '/settings', permission: 'settings' },
    ],
  },
];

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

// ─── Helpers ────────────────────────────────────────────────────────────────

/** Returns true if any child route of a group matches the current path */
function groupIsActive(group, pathname) {
  if (group.standalone) return pathname === group.to;
  return group.items?.some((item) =>
    pathname === item.to || pathname.startsWith(item.to + '/'),
  );
}

// ─── Sub-item ───────────────────────────────────────────────────────────────
function SubItem({ item, accentColor, borderColor }) {
  return (
    <NavLink
      to={item.to}
      end
      className={({ isActive }) =>
        `flex items-center gap-2.5 pl-10 pr-3 py-1.5 rounded-lg text-sm transition-all duration-150 group ${
          isActive
            ? `font-medium text-gray-900 bg-gray-100 border-l-2 ${borderColor} pl-9`
            : 'text-gray-500 hover:text-gray-800 hover:bg-gray-50'
        }`
      }
    >
      {({ isActive }) => (
        <>
          <span
            className={`w-1.5 h-1.5 rounded-full shrink-0 transition-colors ${
              isActive ? accentColor : 'bg-gray-300 group-hover:bg-gray-400'
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
      className={`w-full flex items-center gap-3 px-3 py-2 rounded-xl text-sm font-medium transition-all duration-150 group ${
        isAnyChildActive
          ? 'text-gray-900 bg-gray-100'
          : 'text-gray-600 hover:text-gray-900 hover:bg-gray-50'
      }`}
    >
      {/* Icon box */}
      <span
        className={`w-7 h-7 rounded-lg flex items-center justify-center shrink-0 ${group.iconBg}`}
      >
        <Icon size={14} className={group.iconColor} strokeWidth={2.2} />
      </span>

      {/* Label */}
      <span className="flex-1 text-left">{group.label}</span>

      {/* Chevron */}
      <ChevronDown
        size={14}
        className={`text-gray-400 transition-transform duration-200 ${
          isOpen ? 'rotate-180' : 'rotate-0'
        }`}
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
        `flex items-center gap-3 px-3 py-2 rounded-xl text-sm font-medium transition-all duration-150 ${
          isActive
            ? 'text-gray-900 bg-gray-100'
            : 'text-gray-600 hover:text-gray-900 hover:bg-gray-50'
        }`
      }
    >
      {({ isActive }) => (
        <>
          <span
            className={`w-7 h-7 rounded-lg flex items-center justify-center shrink-0 transition-colors ${
              isActive ? 'bg-teal-500' : group.iconBg
            }`}
          >
            <Icon
              size={14}
              className={isActive ? 'text-white' : group.iconColor}
              strokeWidth={2.2}
            />
          </span>
          <span className="flex-1">{group.label}</span>
          {isActive && (
            <span className={`w-1.5 h-1.5 rounded-full ${group.accentColor}`} />
          )}
        </>
      )}
    </NavLink>
  );
}

// ─── Main component ──────────────────────────────────────────────────────────
export default function CollapsibleSidebar() {
  const location = useLocation();
  const { hasPermission } = useAuth();

  const visibleNavigation = filterNavigation(navigation, hasPermission);

  // Which group is currently open — start with the one that has the active route
  const initialOpen = visibleNavigation.find(
    (g) => !g.standalone && groupIsActive(g, location.pathname),
  )?.id ?? null;

  const [openGroup, setOpenGroup] = useState(initialOpen);

  const toggle = (id) => setOpenGroup((prev) => (prev === id ? null : id));

  return (
    <aside className="w-60 shrink-0 flex flex-col bg-white border-r border-gray-200 h-full">

      {/* ── Header ── */}
      <div className="flex flex-col items-center justify-center gap-1 px-4 py-3 border-b border-gray-100 shrink-0">
        <img src="/Logo_black.png" alt="Cenango" className="h-6 w-auto max-w-full object-contain" />
        <span className="text-sm font-medium text-black">Accounting System</span>
      </div>

      {/* ── Navigation ── */}
      <nav className="flex-1 overflow-y-auto px-3 py-3 space-y-0.5">
        {visibleNavigation.map((group) => {
          if (group.standalone) {
            return <StandaloneRow key={group.id} group={group} />;
          }

          const isOpen        = openGroup === group.id;
          const anyChildActive = groupIsActive(group, location.pathname);

          return (
            <div key={group.id}>
              {/* Group header button */}
              <GroupRow
                group={group}
                isOpen={isOpen}
                onToggle={() => toggle(group.id)}
                isAnyChildActive={anyChildActive}
              />

              {/* Animated sub-items panel */}
              <div
                className={`grid transition-all duration-200 ease-in-out ${
                  isOpen ? 'grid-rows-[1fr] opacity-100' : 'grid-rows-[0fr] opacity-0'
                }`}
              >
                <div className="overflow-hidden">
                  <div className="pt-0.5 pb-1 space-y-0.5 ml-1">
                    {group.items.map((item) => (
                      <SubItem
                        key={item.to}
                        item={item}
                        accentColor={group.accentColor}
                        borderColor={group.borderColor}
                      />
                    ))}
                  </div>
                </div>
              </div>
            </div>
          );
        })}
      </nav>

      {/* ── Footer ── */}
      <div className="px-4 py-3 border-t border-gray-100 shrink-0">
        <div className="flex items-center gap-2.5">
          <div className="w-7 h-7 rounded-full bg-gradient-to-br from-blue-400 to-indigo-500 flex items-center justify-center shrink-0">
            <span className="text-white text-[10px] font-bold">A</span>
          </div>
          <div className="flex flex-col leading-none min-w-0">
            <span className="text-xs font-medium text-gray-700 truncate">Admin</span>
            <span className="text-[10px] text-gray-400 mt-0.5">Internal User</span>
          </div>
        </div>
      </div>
    </aside>
  );
}

/*
 * ─── Sample usage ────────────────────────────────────────────────────────────
 *
 * import { BrowserRouter } from 'react-router-dom';
 * import CollapsibleSidebar from './components/CollapsibleSidebar';
 *
 * function App() {
 *   return (
 *     <BrowserRouter>
 *       <div className="flex h-screen overflow-hidden">
 *         <CollapsibleSidebar />
 *         <main className="flex-1 overflow-y-auto p-6 bg-gray-50">
 *           <p>Page content here</p>
 *         </main>
 *       </div>
 *     </BrowserRouter>
 *   );
 * }
 *
 * export default App;
 */
