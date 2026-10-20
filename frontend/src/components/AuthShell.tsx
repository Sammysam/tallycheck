import { useState, type InputHTMLAttributes, type ReactNode } from 'react';
import { BarChart3, Eye, EyeOff, Lock, ShieldCheck, WifiOff } from 'lucide-react';

/** Sign-in, first-run and 2FA screens: brand panel on the left, the form card on the right. */
export default function AuthShell({ children }: { children: ReactNode }) {
  return (
    <div className="min-h-screen flex bg-[#f4f6fa]">
      <aside className="auth-panel hidden lg:flex w-[46%] max-w-[640px] flex-col justify-between p-12 text-white">
        <div>
          <img src="/Logo_black.png" alt="Cenango" className="sidebar-logo h-8 w-auto" />
          <p className="mt-2 text-xs font-medium uppercase tracking-[0.2em] text-orange-400">Accounting System</p>
        </div>

        <div>
          <h2 className="text-4xl font-bold leading-tight tracking-tight">
            Your books,<br />
            <span className="text-orange-400">clear and in order.</span>
          </h2>
          <p className="mt-4 max-w-md text-slate-300">
            Invoices, bills, payroll and reports in one place, running on this computer.
          </p>
          <ul className="mt-8 space-y-3 text-sm text-slate-200">
            {[
              { icon: WifiOff, text: 'Works fully offline — your data stays on this PC' },
              { icon: Lock, text: 'Encrypted backups and employee documents' },
              { icon: BarChart3, text: 'Live profit & loss, balance sheet and ageing' },
              { icon: ShieldCheck, text: 'Optional two-factor sign-in' },
            ].map(({ icon: I, text }) => (
              <li key={text} className="flex items-center gap-3">
                <span className="w-8 h-8 rounded-lg bg-white/10 flex items-center justify-center">
                  <I size={16} className="text-orange-300" />
                </span>
                {text}
              </li>
            ))}
          </ul>
        </div>

        <p className="text-xs text-slate-500">© {new Date().getFullYear()} Cenango</p>
      </aside>

      <main className="flex-1 flex items-center justify-center p-6">
        <div className="w-full max-w-md">
          <img src="/Logo_black.png" alt="Cenango" className="lg:hidden h-7 w-auto mx-auto mb-8" />
          <div className="bg-white rounded-3xl border border-gray-200/80 shadow-xl shadow-slate-900/5 p-9">{children}</div>
        </div>
      </main>
    </div>
  );
}

/** Password field with a show/hide eye. */
export function PasswordInput(props: InputHTMLAttributes<HTMLInputElement>) {
  const [shown, setShown] = useState(false);
  return (
    <div className="relative">
      <input {...props} type={shown ? 'text' : 'password'} className={`${props.className ?? ''} !pr-10`} />
      <button
        type="button"
        tabIndex={-1}
        onClick={() => setShown((s) => !s)}
        className="absolute right-2 top-1/2 -translate-y-1/2 p-1.5 text-gray-400 hover:text-gray-600 rounded-md"
        aria-label={shown ? 'Hide password' : 'Show password'}
        title={shown ? 'Hide password' : 'Show password'}
      >
        {shown ? <EyeOff size={16} /> : <Eye size={16} />}
      </button>
    </div>
  );
}
