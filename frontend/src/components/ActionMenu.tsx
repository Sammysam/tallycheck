import { useEffect, useLayoutEffect, useRef, useState } from 'react';
import { createPortal } from 'react-dom';

export interface ActionItem {
  label: string;
  onSelect: () => void;
  /** Red text — for void, delete and the like. */
  danger?: boolean;
}

/**
 * Row actions behind a ⋮ button.
 *
 * The menu is portalled to the body and positioned against the button's
 * screen rect: the table scrolls inside `overflow-x-auto`, which would clip a
 * dropdown rendered inside the cell.
 */
export default function ActionMenu({ items }: { items: ActionItem[] }) {
  const [open, setOpen] = useState(false);
  const [pos, setPos] = useState({ top: 0, left: 0 });
  const btnRef = useRef<HTMLButtonElement>(null);
  const menuRef = useRef<HTMLDivElement>(null);

  const WIDTH = 176;

  // Place it after mount so the real menu height decides up vs down.
  useLayoutEffect(() => {
    if (!open || !btnRef.current || !menuRef.current) return;
    const button = btnRef.current.getBoundingClientRect();
    const height = menuRef.current.offsetHeight;
    const below = button.bottom + 4;
    setPos({
      top: below + height > window.innerHeight - 8 ? Math.max(8, button.top - height - 4) : below,
      left: Math.max(8, Math.min(button.right - WIDTH, window.innerWidth - WIDTH - 8)),
    });
  }, [open]);

  useEffect(() => {
    if (!open) return;

    const onPointerDown = (e: MouseEvent) => {
      const target = e.target as Node;
      if (btnRef.current?.contains(target) || menuRef.current?.contains(target)) return;
      setOpen(false);
    };
    const onKey = (e: KeyboardEvent) => { if (e.key === 'Escape') setOpen(false); };
    const close = () => setOpen(false);

    document.addEventListener('mousedown', onPointerDown);
    document.addEventListener('keydown', onKey);
    // Any scroll or resize moves the button out from under the menu.
    window.addEventListener('scroll', close, true);
    window.addEventListener('resize', close);
    return () => {
      document.removeEventListener('mousedown', onPointerDown);
      document.removeEventListener('keydown', onKey);
      window.removeEventListener('scroll', close, true);
      window.removeEventListener('resize', close);
    };
  }, [open]);

  if (!items.length) return null;

  return (
    <>
      <button
        ref={btnRef}
        type="button"
        aria-label="Actions"
        aria-haspopup="menu"
        aria-expanded={open}
        onClick={() => setOpen((v) => !v)}
        className="action-dots"
      >
        ⋮
      </button>

      {open && createPortal(
        <div
          ref={menuRef}
          role="menu"
          className="action-menu"
          style={{ top: pos.top, left: pos.left, width: WIDTH }}
        >
          {items.map((item) => (
            <button
              key={item.label}
              type="button"
              role="menuitem"
              className={item.danger ? 'danger' : undefined}
              onClick={() => { setOpen(false); item.onSelect(); }}
            >
              {item.label}
            </button>
          ))}
        </div>,
        document.body,
      )}
    </>
  );
}
