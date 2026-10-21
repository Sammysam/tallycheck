import { useState } from 'react';
import { Eye, EyeOff } from 'lucide-react';
import { passwordStrength } from '../lib/password';

/** A password box with a show / hide eye. */
export default function PasswordInput({ value, onChange, placeholder, minLength = 8, required, autoFocus, ariaLabel }: {
  value: string;
  onChange: (v: string) => void;
  placeholder?: string;
  minLength?: number;
  required?: boolean;
  autoFocus?: boolean;
  ariaLabel?: string;
}) {
  const [show, setShow] = useState(false);
  return (
    <div style={{ position: 'relative', display: 'flex', alignItems: 'center' }}>
      <input
        required={required}
        autoFocus={autoFocus}
        type={show ? 'text' : 'password'}
        placeholder={placeholder}
        minLength={minLength}
        value={value}
        onChange={(e) => onChange(e.target.value)}
        aria-label={ariaLabel ?? placeholder}
        style={{ width: '100%', paddingRight: '2.25rem' }}
      />
      <button
        type="button"
        onClick={() => setShow((s) => !s)}
        title={show ? 'Hide password' : 'Show password'}
        aria-label={show ? 'Hide password' : 'Show password'}
        style={{ position: 'absolute', right: '0.5rem', background: 'none', border: 'none', cursor: 'pointer', padding: 0, display: 'flex', color: '#6b7280' }}
      >
        {show ? <EyeOff size={18} /> : <Eye size={18} />}
      </button>
    </div>
  );
}

export function StrengthBar({ password }: { password: string }) {
  if (!password) return null;
  const s = passwordStrength(password);
  const colors = ['#dc2626', '#f97316', '#eab308', '#16a34a'];
  return (
    <div className="mt-1">
      <div className="flex gap-1">
        {[0, 1, 2].map((i) => (
          <span key={i} className="h-1 flex-1 rounded" style={{ background: i < s.score ? colors[s.score] : '#e5e7eb' }} />
        ))}
      </div>
      <p className="text-xs mt-0.5" style={{ color: colors[s.score] }}>{s.label}</p>
    </div>
  );
}
