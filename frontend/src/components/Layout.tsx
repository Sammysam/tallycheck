import { useEffect, useRef, useState, type ReactNode } from 'react';
// eslint-disable-next-line @typescript-eslint/ban-ts-comment
// @ts-ignore
import CollapsibleSidebar, { pageTrail, initialsOf } from './CollapsibleSidebar';
import { useAuth } from '../contexts/AuthContext';
import { useLocation, useNavigate } from 'react-router-dom';
import { CalendarDays, ChevronDown, ChevronRight, DatabaseBackup, LogOut, ShieldCheck } from 'lucide-react';

export default function Layout({ children }: { children: ReactNode }) {
  const { user, logout } = useAuth();
  const navigate = useNavigate();
  const { pathname } = useLocation();
  const [section, page] = pageTrail(pathname) as [string | null, string];
  const [menuOpen, setMenuOpen] = useState(false);
  const menuRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!menuOpen) return;
    const close = (e: MouseEvent) => {
      if (menuRef.current && !menuRef.current.contains(e.target as Node)) setMenuOpen(false);
    };
    document.addEventListener('mousedown', close);
    return () => document.removeEventListener('mousedown', close);
  }, [menuOpen]);

  const handleLogout = () => {
    logout();
    navigate('/login');
  };

  const today = new Date().toLocaleDateString('en-GB', { weekday: 'short', day: 'numeric', month: 'short', year: 'numeric' });

  return (
    <div className="flex h-screen overflow-hidden bg-[#f4f6fa]">
      <CollapsibleSidebar />

      <div className="flex-1 flex flex-col overflow-hidden">
        {/* Top bar */}
        <header className="h-16 bg-white/80 backdrop-blur border-b border-gray-200/80 flex items-center justify-between px-8 shrink-0">
          <nav className="flex items-center gap-1.5 text-sm min-w-0" aria-label="Breadcrumb">
            {section && (
              <>
                <span className="text-gray-400">{section}</span>
                <ChevronRight size={14} className="text-gray-300 shrink-0" />
              </>
            )}
            <span className="font-semibold text-gray-800 truncate">{page}</span>
          </nav>

          <div className="flex items-center gap-3">
            <span className="hidden lg:flex items-center gap-1.5 text-xs text-gray-500 px-3 py-1.5 rounded-full bg-gray-100">
              <CalendarDays size={13} />
              {today}
            </span>
            {user?.twoFactorEnabled === false && (
              <button
                onClick={() => navigate('/2fa/setup')}
                className="flex items-center gap-1.5 text-xs font-medium text-orange-700 bg-orange-50 border border-orange-200 hover:bg-orange-100 transition-colors px-3 py-1.5 rounded-full"
                title="Optional: ask for a code from your phone at every sign-in"
              >
                <ShieldCheck size={13} />
                Turn on 2FA
              </button>
            )}

            <div className="relative" ref={menuRef}>
              <button
                type="button"
                onClick={() => setMenuOpen((o) => !o)}
                className="flex items-center gap-2 pl-1 pr-2 py-1 rounded-full hover:bg-gray-100 transition-colors"
                aria-haspopup="menu"
                aria-expanded={menuOpen}
              >
                <span className="w-8 h-8 rounded-full bg-gradient-to-br from-orange-400 to-orange-600 flex items-center justify-center text-white text-xs font-bold">
                  {initialsOf(user?.username)}
                </span>
                <span className="hidden md:block text-sm font-medium text-gray-700">{user?.username}</span>
                <ChevronDown size={14} className="text-gray-400" />
              </button>

              {menuOpen && (
                <div role="menu" className="absolute right-0 mt-2 w-56 bg-white rounded-xl border border-gray-200 shadow-xl shadow-gray-900/10 p-1.5 z-40">
                  <div className="px-3 py-2 border-b border-gray-100 mb-1">
                    <p className="text-sm font-semibold text-gray-900 truncate">{user?.username}</p>
                    <p className="text-xs text-gray-500">{user?.role || 'No role'}</p>
                  </div>
                  {user?.role === 'Admin' && (
                    <a
                      href="/system/backup"
                      role="menuitem"
                      className="flex items-center gap-2.5 px-3 py-2 text-sm text-gray-700 rounded-lg hover:bg-gray-50"
                    >
                      <DatabaseBackup size={15} className="text-gray-400" />
                      Backup and restore
                    </a>
                  )}
                  <button
                    type="button"
                    role="menuitem"
                    onClick={handleLogout}
                    className="w-full flex items-center gap-2.5 px-3 py-2 text-sm text-red-600 rounded-lg hover:bg-red-50"
                  >
                    <LogOut size={15} />
                    Sign out
                  </button>
                </div>
              )}
            </div>
          </div>
        </header>

        {/* Page content */}
        <main className="flex-1 overflow-y-auto px-8 py-7">
          <div className="max-w-[1400px] mx-auto">{children}</div>
        </main>
      </div>
    </div>
  );
}
