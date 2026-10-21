import type { ReactNode } from 'react';
import Dialog from './desktop/Dialog';

/** D36: the app's own yes / no question, instead of the browser's confirm() box. */
export default function ConfirmDialog({ title, children, confirmLabel, danger, busy, onConfirm, onCancel }: {
  title: string;
  children: ReactNode;
  confirmLabel: string;
  danger?: boolean;
  busy?: boolean;
  onConfirm: () => void;
  onCancel: () => void;
}) {
  return (
    <Dialog
      title={title}
      onClose={() => !busy && onCancel()}
      buttons={
        <>
          <button type="button" className={`btn ${danger ? 'btn-danger' : 'btn-primary'}`} disabled={busy} onClick={onConfirm}>
            {busy ? 'Working…' : confirmLabel}
          </button>
          <button type="button" className="btn btn-secondary" disabled={busy} onClick={onCancel}>Cancel</button>
        </>
      }
    >
      <div className="text-sm text-gray-800 space-y-2">{children}</div>
    </Dialog>
  );
}
