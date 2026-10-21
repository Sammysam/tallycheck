import { useSyncExternalStore } from 'react';
import { CheckCircle2, Info, X, XCircle } from 'lucide-react';
import { currentToasts, dismiss, subscribeToasts } from '../lib/toast';

const STYLE = {
  success: { icon: CheckCircle2, color: '#15803d', border: '#bbf7d0' },
  error: { icon: XCircle, color: '#b91c1c', border: '#fecaca' },
  info: { icon: Info, color: '#1d4ed8', border: '#bfdbfe' },
};

/** D36: the corner notices raised with `toast()`. */
export default function ToastHost() {
  const toasts = useSyncExternalStore(subscribeToasts, currentToasts);
  if (!toasts.length) return null;
  return (
    <div className="fixed right-4 bottom-10 z-[1000] flex flex-col gap-2 no-print" role="status" aria-live="polite">
      {toasts.map((t) => {
        const s = STYLE[t.kind];
        const Icon = s.icon;
        return (
          <div key={t.id} className="flex items-start gap-2 bg-white rounded-md shadow-lg px-3 py-2.5 text-sm max-w-sm"
            style={{ border: `1px solid ${s.border}`, borderLeft: `4px solid ${s.color}` }} data-testid="toast">
            <Icon size={16} style={{ color: s.color, marginTop: 2, flexShrink: 0 }} />
            <span className="flex-1 text-gray-800 whitespace-pre-line">{t.text}</span>
            <button className="text-gray-400 hover:text-gray-700" onClick={() => dismiss(t.id)} aria-label="Dismiss"><X size={14} /></button>
          </div>
        );
      })}
    </div>
  );
}
