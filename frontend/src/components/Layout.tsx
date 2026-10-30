import { useCallback, useEffect, useRef, useState, useSyncExternalStore, type ReactNode } from 'react';
// eslint-disable-next-line @typescript-eslint/ban-ts-comment
// @ts-ignore
import CollapsibleSidebar, { canSee, navigation, pageTrail } from './CollapsibleSidebar';
import { useAuth } from '../contexts/AuthContext';
import { useLocation, useNavigate } from 'react-router-dom';
import {
  ArrowLeft,
  ArrowRight,
  BarChart2,
  CreditCard,
  FileSpreadsheet,
  FileText,
  Home,
  Printer,
  Receipt,
  RefreshCw,
  Truck,
  UserRound,
  Users,
  Wallet,
} from 'lucide-react';
import MenuBar, { type Menu, type MenuEntry } from './desktop/MenuBar';
import Toolbar, { type ToolbarButton } from './desktop/Toolbar';
import StatusBar from './desktop/StatusBar';
import Dialog from './desktop/Dialog';
import UpdateDialog from './desktop/UpdateDialog';
import { useAvailableUpdate } from '../lib/updates';
import { hasCommand, isUnsaved, runCommand, setUnsaved, subscribeCommands } from '../lib/commands';
import { exportScreen } from '../lib/exportCsv';
import { companyIsSetUp, region, SETUP_LATER, regionFromSettings, setRegion, subscribeRegion } from '../lib/region';
import api from '../api/client';
import ToastHost from './ToastHost';

declare const __APP_VERSION__: string;

interface NavItem { label: string; to: string; permission: string | string[] }
interface NavGroup { id: string; label: string; standalone?: boolean; items?: NavItem[] }

const SHORTCUTS: [string, string][] = [
  ['Ctrl+N', 'New record on this screen'],
  ['Ctrl+S', 'Save the open form'],
  ['Esc', 'Close the open form'],
  ['Ctrl+F', 'Find (jump to the search box)'],
  ['Ctrl+P', 'Print this screen'],
  ['Ctrl+E', 'Export the tables on this screen to Excel (CSV)'],
  ['F5', 'Refresh this screen'],
  ['Ctrl+B', 'Show or hide the navigation pane'],
  ['Alt+Left / Alt+Right', 'Back / forward'],
  ['Alt+letter', 'Open a menu (Alt+F opens File)'],
  ['F1', 'This list'],
];

/** Browser keys that make no sense in a desktop window: zoom, view source, history, downloads… */
const BROWSER_KEYS = ['+', '=', '-', '_', '0', 'u', 'g', 'h', 'j', 'l', 'd', 'o', 'e', 'k', 't', 'w'];

const isTextField = (el: EventTarget | null) =>
  el instanceof HTMLElement && (el.isContentEditable || /^(INPUT|TEXTAREA|SELECT)$/.test(el.tagName));

export default function Layout({ children }: { children: ReactNode }) {
  const { user, logout, hasPermission } = useAuth();
  const navigate = useNavigate();
  const location = useLocation();
  const { pathname } = location;
  const [section, page] = pageTrail(pathname) as [string | null, string];
  const [paneOpen, setPaneOpen] = useState(() => {
    try {
      return localStorage.getItem('navPane') !== 'hidden';
    } catch {
      return true;
    }
  });
  const [refreshKey, setRefreshKey] = useState(0);
  const [dialog, setDialog] = useState<'shortcuts' | 'about' | 'discard' | 'nothing-to-export' | 'update' | null>(null);
  const mainRef = useRef<HTMLElement>(null);
  // Menu items grey out when the screen has no "New" / "Close".
  const canNew = useSyncExternalStore(subscribeCommands, () => hasCommand('new'));
  const canClose = useSyncExternalStore(subscribeCommands, () => hasCommand('close'));

  const isAdmin = user?.role === 'Admin';
  // D45: an Administrator is told when a newer TallyCheck is published.
  const [update] = useAvailableUpdate(isAdmin);

  // D25: the company's details and currency, loaded once; every screen formats with them.
  const [regionReady, setRegionReady] = useState(false);
  useSyncExternalStore(subscribeRegion, region);
  useEffect(() => {
    api.get('/settings')
      .then((r) => {
        setRegion(regionFromSettings(r.data));
        // A new installation: the administrator enters the company before anything else.
        if (!companyIsSetUp(r.data) && hasPermission('settings') && !sessionStorage.getItem(SETUP_LATER)) navigate('/company-setup', { replace: true });
      })
      .catch(() => {})
      .finally(() => setRegionReady(true));
  }, [hasPermission, navigate]);

  // ── Actions shared by the menu bar, toolbar and keyboard ───────────────
  const newRecord = useCallback(() => {
    setUnsaved(false);
    runCommand('new');
  }, []);

  /** Goes to a list and opens its "New" form, as the toolbar's Invoice/Bill/… buttons do. */
  const openNew = (to: string) => {
    if (pathname === to) newRecord();
    else navigate(to, { state: { command: 'new' } });
  };

  const save = useCallback(() => {
    const main = mainRef.current;
    if (!main) return;
    const focused = document.activeElement?.closest('form');
    const form =
      (focused && main.contains(focused) ? focused : null) ??
      [...main.querySelectorAll('form')].find((f) => f.querySelector('button[type=submit], button:not([type])'));
    form?.requestSubmit();
  }, []);

  const closeForm = useCallback(() => {
    if (!hasCommand('close')) return;
    if (isUnsaved()) setDialog('discard');
    else runCommand('close');
  }, []);

  const find = useCallback(() => {
    const box = mainRef.current?.querySelector<HTMLInputElement>(
      'input[type=search], input[placeholder*="search" i], input[placeholder*="filter" i]',
    );
    box?.focus();
    box?.select();
  }, []);

  // D23: every table on the screen into one CSV file named after the screen.
  const exportTables = useCallback(() => {
    const main = document.querySelector<HTMLElement>('main.desk-main');
    // Named after the screen's own heading ("Customer Statement", "Profit & Loss", …).
    const title = main?.querySelector('h1')?.textContent?.trim() || pageTrail(window.location.pathname)[1] || 'Export';
    if (!main || !exportScreen(main, title)) setDialog('nothing-to-export');
  }, []);

  const refresh = useCallback(() => {
    setUnsaved(false);
    setRefreshKey((k) => k + 1);
  }, []);

  const togglePane = useCallback(() => {
    setPaneOpen((open) => {
      try {
        localStorage.setItem('navPane', open ? 'hidden' : 'shown');
      } catch {
        /* the pane state is only a convenience */
      }
      return !open;
    });
  }, []);

  const signOut = () => {
    logout();
    navigate('/login');
  };

  // A "New …" asked for from another screen runs once that screen has mounted.
  useEffect(() => {
    setUnsaved(false);
    const state = location.state as { command?: 'new' } | null;
    if (state?.command === 'new') {
      runCommand('new');
      navigate(location.pathname + location.search, { replace: true, state: null });
    }
  }, [location, navigate]);

  // Typing in a form marks it unsaved; saving it clears that.
  useEffect(() => {
    const main = mainRef.current;
    if (!main) return;
    const onInput = (e: Event) => {
      if ((e.target as HTMLElement).closest('form')) setUnsaved(true);
    };
    const onSubmit = () => {
      setUnsaved(false);
    };
    main.addEventListener('input', onInput);
    main.addEventListener('submit', onSubmit);
    return () => {
      main.removeEventListener('input', onInput);
      main.removeEventListener('submit', onSubmit);
    };
  }, [refreshKey, regionReady]);

  // ── Keyboard: desktop shortcuts on, browser shortcuts off ──────────────
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.defaultPrevented) return;
      const ctrl = e.ctrlKey || e.metaKey;
      const k = e.key.toLowerCase();
      const plain = ctrl && !e.shiftKey && !e.altKey;
      const act = (fn: () => void) => {
        e.preventDefault();
        fn();
      };
      if (ctrl && e.shiftKey && k === 'r') return act(() => window.location.reload());
      if (e.key === 'F5' || (plain && k === 'r')) return act(refresh);
      if (plain && k === 'n') return act(newRecord);
      if (plain && k === 's') return act(save);
      if (plain && k === 'p') return act(() => window.print());
      if (plain && k === 'e') return act(exportTables);
      if (plain && k === 'f') return act(find);
      if (plain && k === 'b') return act(togglePane);
      if (e.key === 'F1') return act(() => setDialog('shortcuts'));
      if (e.key === 'F3' || e.key === 'F7') return e.preventDefault();
      if (e.altKey && e.key === 'ArrowLeft') return act(() => navigate(-1));
      if (e.altKey && e.key === 'ArrowRight') return act(() => navigate(1));
      if (e.key === 'Escape' && !document.querySelector('.action-menu')) return act(closeForm);
      if (ctrl && !e.altKey && BROWSER_KEYS.includes(k)) e.preventDefault();
    };
    // Ctrl+wheel would zoom the page like a browser.
    const onWheel = (e: WheelEvent) => {
      if (e.ctrlKey) e.preventDefault();
    };
    // Right-click keeps Cut/Copy/Paste in text boxes and on selected text, and nothing else.
    const onContext = (e: MouseEvent) => {
      if (isTextField(e.target) || String(window.getSelection() ?? '').trim()) return;
      e.preventDefault();
    };
    window.addEventListener('keydown', onKey);
    window.addEventListener('wheel', onWheel, { passive: false });
    window.addEventListener('contextmenu', onContext);
    return () => {
      window.removeEventListener('keydown', onKey);
      window.removeEventListener('wheel', onWheel);
      window.removeEventListener('contextmenu', onContext);
    };
  }, [refresh, newRecord, save, find, togglePane, closeForm, exportTables, navigate]);

  // ── Menus ──────────────────────────────────────────────────────────────
  const go = (to: string) => () => navigate(to);
  const navMenus: Menu[] = (navigation as NavGroup[])
    .filter((g) => !g.standalone)
    .map((g) => ({
      label: g.label === 'Master Data' ? 'Lists' : g.label === 'Administration' ? 'Admin' : g.label,
      items: (g.items ?? [])
        .filter((i) => canSee(i.permission, hasPermission))
        .map((i): MenuEntry => ({ label: i.label, onSelect: go(i.to) })),
    }))
    .filter((m) => m.items.length > 0);

  const fileItems: MenuEntry[] = [
    { label: 'New', shortcut: 'Ctrl+N', onSelect: newRecord, disabled: !canNew },
    { label: 'Save', shortcut: 'Ctrl+S', onSelect: save },
    { label: 'Close form', shortcut: 'Esc', onSelect: closeForm, disabled: !canClose },
    'separator',
    { label: 'Print…', shortcut: 'Ctrl+P', onSelect: () => window.print() },
    { label: 'Export to Excel (CSV)…', shortcut: 'Ctrl+E', onSelect: exportTables },
    ...(['banking', 'invoices', 'customers', 'vendors', 'ledger'].some((p) => hasPermission(p))
      ? [{ label: 'Import from file…', onSelect: go('/import') }]
      : []),
    'separator',
    ...(isAdmin
      ? [{ label: 'Backup and restore…', onSelect: go('/settings?tab=backup') }, 'separator' as const]
      : []),
    ...(user?.twoFactorEnabled === false ? [{ label: 'Turn on two-factor sign-in…', onSelect: go('/2fa/setup') }] : []),
    { label: 'Sign out', onSelect: signOut },
  ];

  const viewItems: MenuEntry[] = [
    ...(hasPermission('dashboard') ? [{ label: 'Dashboard', onSelect: go('/') }, 'separator' as const] : []),
    { label: 'Back', shortcut: 'Alt+Left', onSelect: () => navigate(-1) },
    { label: 'Forward', shortcut: 'Alt+Right', onSelect: () => navigate(1) },
    { label: 'Refresh', shortcut: 'F5', onSelect: refresh },
    'separator',
    { label: paneOpen ? 'Hide navigation pane' : 'Show navigation pane', shortcut: 'Ctrl+B', onSelect: togglePane },
    { label: 'Reload window', shortcut: 'Ctrl+Shift+R', onSelect: () => window.location.reload() },
  ];

  const menus: Menu[] = [
    { label: 'File', items: fileItems },
    {
      label: 'Edit',
      items: [
        { label: 'Find', shortcut: 'Ctrl+F', onSelect: find },
        'separator',
        { label: 'Cut', shortcut: 'Ctrl+X', onSelect: () => document.execCommand('cut') },
        { label: 'Copy', shortcut: 'Ctrl+C', onSelect: () => document.execCommand('copy') },
        { label: 'Select all', shortcut: 'Ctrl+A', onSelect: () => document.execCommand('selectAll') },
      ],
    },
    { label: 'View', items: viewItems },
    ...navMenus,
    {
      label: 'Help',
      items: [
        { label: 'Keyboard shortcuts', shortcut: 'F1', onSelect: () => setDialog('shortcuts') },
        'separator',
        ...(isAdmin ? [{ label: 'Check for updates…', onSelect: () => setDialog('update') }] : []),
        { label: 'About TallyCheck', onSelect: () => setDialog('about') },
      ],
    },
  ];

  // ── Toolbar ────────────────────────────────────────────────────────────
  const tool = (permission: string, b: ToolbarButton) => (hasPermission(permission) ? [b] : []);
  const toolbar: (ToolbarButton | null)[] = [
    { label: 'Back', title: 'Back (Alt+Left)', icon: ArrowLeft, onClick: () => navigate(-1) },
    { label: 'Forward', title: 'Forward (Alt+Right)', icon: ArrowRight, onClick: () => navigate(1) },
    ...tool('dashboard', { label: 'Home', title: 'Dashboard', icon: Home, onClick: go('/') }),
    null,
    ...tool('invoices', { label: 'Invoice', title: 'Create an invoice', icon: FileText, onClick: () => openNew('/invoices') }),
    ...tool('bills', { label: 'Bill', title: 'Enter a vendor bill', icon: Receipt, onClick: () => openNew('/bills') }),
    ...tool('expenses', { label: 'Expense', title: 'Record an expense', icon: Wallet, onClick: () => openNew('/expenses') }),
    ...tool('payments', { label: 'Payment', title: 'Record a payment', icon: CreditCard, onClick: () => openNew('/payments') }),
    null,
    ...tool('customers', { label: 'Customers', title: 'Customer list', icon: Users, onClick: go('/customers') }),
    ...tool('vendors', { label: 'Vendors', title: 'Vendor list', icon: Truck, onClick: go('/vendors') }),
    ...tool('employees', { label: 'Employees', title: 'Employee list', icon: UserRound, onClick: go('/employees') }),
    ...tool('reports', { label: 'Reports', title: 'Profit & Loss', icon: BarChart2, onClick: go('/reports/profit-loss') }),
    null,
    { label: 'Refresh', title: 'Refresh (F5)', icon: RefreshCw, onClick: refresh },
    { label: 'Print', title: 'Print (Ctrl+P)', icon: Printer, onClick: () => window.print() },
    { label: 'Export', title: 'Export the tables on this screen to Excel (Ctrl+E)', icon: FileSpreadsheet, onClick: exportTables },
  ];
  // No divider twice in a row when a whole group is hidden by permissions.
  const tools = toolbar.filter((b, i, all) => b !== null || (i > 0 && all[i - 1] !== null));

  const closeDialog = () => setDialog(null);

  if (!regionReady) return <div className="desk-shell" />;

  return (
    <div className="desk-shell">
      <div className="desk-chrome">
        <MenuBar menus={menus} />
        <Toolbar buttons={tools} />
      </div>

      <div className="flex-1 flex min-h-0">
        {paneOpen && <CollapsibleSidebar />}
        <main ref={mainRef} className="desk-main" key={refreshKey}>
          {children}
        </main>
      </div>

      <StatusBar
        location={section ? `${section}  ›  ${page}` : page}
        user={user?.username ?? ''}
        role={user?.role ?? ''}
        offerTwoFactor={user?.twoFactorEnabled === false}
        onTwoFactor={go('/2fa/setup')}
        updateVersion={update?.available?.version ?? null}
        onUpdate={() => setDialog('update')}
      />

      {dialog === 'update' && <UpdateDialog initial={update} onClose={closeDialog} />}

      {dialog === 'shortcuts' && (
        <Dialog
          title="Keyboard shortcuts"
          onClose={closeDialog}
          width={460}
          buttons={<button type="button" className="btn btn-primary" onClick={closeDialog}>OK</button>}
        >
          <table className="w-full text-sm">
            <tbody>
              {SHORTCUTS.map(([keys, what]) => (
                <tr key={keys} className="border-b border-gray-100 last:border-0">
                  <td className="py-1.5 pr-4 whitespace-nowrap"><kbd className="desk-kbd">{keys}</kbd></td>
                  <td className="py-1.5 text-gray-700">{what}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </Dialog>
      )}

      {dialog === 'about' && (
        <Dialog
          title="About TallyCheck"
          onClose={closeDialog}
          buttons={<button type="button" className="btn btn-primary" onClick={closeDialog}>OK</button>}
        >
          <div className="flex items-center gap-4">
            <img src="/favicon-96x96.png" alt="" className="w-12 h-12" />
            <div className="text-sm">
              <p className="font-semibold text-gray-900">TallyCheck</p>
              <p className="text-gray-600">Version {__APP_VERSION__}</p>
              <p className="text-gray-500 mt-1">Offline accounting, employees and payroll.</p>
            </div>
          </div>
          <div className="text-xs text-gray-600 mt-4 space-y-1.5">
            <p>Copyright © 2026 Cenango (Pvt) Ltd.</p>
            <p>
              Licensed under the MIT License or the Apache License 2.0, at your option. It comes with no warranty.
              The names TallyCheck and Cenango and their logos are not covered by these licenses.
            </p>
            <p>
              The licenses (LICENSE-MIT.txt, LICENSE-APACHE.txt), NOTICE.txt and the third-party notices are in the program
              folder. Source code:
              <br />
              <span className="select-all">https://github.com/Sammysam/tallycheck</span>
            </p>
          </div>
        </Dialog>
      )}

      {dialog === 'nothing-to-export' && (
        <Dialog
          title="Export to Excel"
          onClose={closeDialog}
          buttons={<button type="button" className="btn btn-primary" onClick={closeDialog}>OK</button>}
        >
          <p className="text-sm text-gray-700">There is no table on this screen to export.</p>
        </Dialog>
      )}

      {dialog === 'discard' && (
        <Dialog
          title="TallyCheck"
          onClose={closeDialog}
          buttons={
            <>
              <button
                type="button"
                className="btn btn-primary"
                onClick={() => {
                  setDialog(null);
                  setUnsaved(false);
                  runCommand('close');
                }}
              >
                Close without saving
              </button>
              <button type="button" className="btn btn-secondary" onClick={closeDialog}>Keep editing</button>
            </>
          }
        >
          <p className="text-sm text-gray-700">This form has changes that are not saved. Close it anyway?</p>
        </Dialog>
      )}
      <ToastHost />
    </div>
  );
}
