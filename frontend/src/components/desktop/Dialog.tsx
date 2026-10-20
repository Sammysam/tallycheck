import { useEffect, useRef, type ReactNode } from 'react';
import { X } from 'lucide-react';

interface DialogProps {
  title: string;
  children: ReactNode;
  /** Buttons along the bottom right, Windows style. */
  buttons: ReactNode;
  onClose: () => void;
  width?: number;
}

/** A modal window inside the app: title strip, content, buttons bottom right. Esc closes it. */
export default function Dialog({ title, children, buttons, onClose, width = 420 }: DialogProps) {
  const boxRef = useRef<HTMLDivElement>(null);

  const closeRef = useRef(onClose);
  useEffect(() => {
    closeRef.current = onClose;
  });

  useEffect(() => {
    boxRef.current?.querySelector<HTMLButtonElement>('.desk-dialog-buttons .btn')?.focus();
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.preventDefault();
        e.stopImmediatePropagation();
        closeRef.current();
      }
    };
    window.addEventListener('keydown', onKey, true);
    return () => window.removeEventListener('keydown', onKey, true);
  }, []);

  return (
    <div className="desk-dialog-backdrop" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div ref={boxRef} role="dialog" aria-modal="true" aria-label={title} className="desk-dialog" style={{ width }}>
        <div className="desk-dialog-title">
          <span>{title}</span>
          <button type="button" aria-label="Close" className="desk-dialog-x" onClick={onClose}>
            <X size={14} />
          </button>
        </div>
        <div className="desk-dialog-body">{children}</div>
        <div className="desk-dialog-buttons">{buttons}</div>
      </div>
    </div>
  );
}
