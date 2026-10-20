import { useState, type InputHTMLAttributes, type ReactNode } from 'react';
import { Eye, EyeOff, WifiOff } from 'lucide-react';

/** Sign-in, first-run and 2FA screens: one centred sign-in window, like a desktop program's. */
export default function AuthShell({ children }: { children: ReactNode }) {
  return (
    <div className="h-screen overflow-auto flex items-center justify-center p-6 bg-[#e9ecef]">
      <div className="w-full max-w-[420px] bg-white border border-[#cfcfcf] rounded-lg shadow-[0_16px_40px_rgba(0,0,0,0.18)] overflow-hidden">
        <div className="auth-panel px-6 py-5">
          <img src="/Logo_black.png" alt="Cenango" className="logo-white h-6 w-auto" />
          <p className="mt-1.5 text-[11px] font-medium uppercase tracking-[0.18em] text-orange-300">Accounting System</p>
        </div>
        <div className="px-6 py-5">{children}</div>
        <div className="flex items-center gap-1.5 px-6 py-2 text-[11px] text-gray-500 bg-[#f3f3f3] border-t border-[#e5e5e5]">
          <WifiOff size={12} />
          Works offline. Your data stays on this PC.
        </div>
      </div>
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
        className="absolute right-1 top-1/2 -translate-y-1/2 p-1 text-gray-500 hover:bg-gray-100 rounded"
        aria-label={shown ? 'Hide password' : 'Show password'}
        title={shown ? 'Hide password' : 'Show password'}
      >
        {shown ? <EyeOff size={15} /> : <Eye size={15} />}
      </button>
    </div>
  );
}
