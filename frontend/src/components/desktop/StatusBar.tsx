import { useEffect, useState } from 'react';
import { Download, ShieldCheck } from 'lucide-react';

interface StatusBarProps {
  location: string;
  user: string;
  role: string;
  offerTwoFactor: boolean;
  onTwoFactor: () => void;
  /** D45: a newer version to offer, or null. */
  updateVersion: string | null;
  onUpdate: () => void;
}

/** Bottom strip: where you are, who is signed in, the server link, Caps Lock and the date. */
export default function StatusBar({ location, user, role, offerTwoFactor, onTwoFactor, updateVersion, onUpdate }: StatusBarProps) {
  const [online, setOnline] = useState(true);
  const [caps, setCaps] = useState(false);

  useEffect(() => {
    // A public, read-only endpoint: answers as long as the server is up.
    const ping = () =>
      fetch('/api/auth/first-run', { cache: 'no-store' })
        .then((r) => setOnline(r.ok))
        .catch(() => setOnline(false));
    ping();
    const timer = window.setInterval(ping, 30000);
    return () => window.clearInterval(timer);
  }, []);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (typeof e.getModifierState === 'function') setCaps(e.getModifierState('CapsLock'));
    };
    window.addEventListener('keydown', onKey);
    window.addEventListener('keyup', onKey);
    return () => {
      window.removeEventListener('keydown', onKey);
      window.removeEventListener('keyup', onKey);
    };
  }, []);

  const today = new Date().toLocaleDateString('en-GB', { weekday: 'short', day: 'numeric', month: 'short', year: 'numeric' });

  return (
    <footer className="desk-statusbar">
      <span className="desk-status-cell flex-1 truncate">{location || 'Ready'}</span>
      {updateVersion && (
        <button
          type="button"
          onClick={onUpdate}
          className="desk-status-cell desk-status-btn"
          title="A newer version of TallyCheck is published"
        >
          <Download size={12} />
          Update to {updateVersion}
        </button>
      )}
      {offerTwoFactor && (
        <button
          type="button"
          onClick={onTwoFactor}
          className="desk-status-cell desk-status-btn"
          title="Optional: ask for a code from your phone at every sign-in"
        >
          <ShieldCheck size={12} />
          Turn on 2FA
        </button>
      )}
      <span className="desk-status-cell">
        {user} ({role || 'No role'})
      </span>
      <span className="desk-status-cell" title={online ? 'The accounting server is answering' : 'Cannot reach the accounting server'}>
        <span className={`inline-block w-2 h-2 rounded-full ${online ? 'bg-emerald-500' : 'bg-red-500'}`} />
        {online ? 'Connected' : 'Not connected'}
      </span>
      <span className="desk-status-cell w-12 justify-center">{caps ? 'CAPS' : ''}</span>
      <span className="desk-status-cell">{today}</span>
    </footer>
  );
}
