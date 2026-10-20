import { useEffect, useRef, useState } from 'react';

export type MenuEntry =
  | { label: string; shortcut?: string; onSelect: () => void; disabled?: boolean }
  | 'separator';

export interface Menu {
  label: string;
  items: MenuEntry[];
}

/** Picks a distinct Alt+letter for each menu, like Windows does: File, Edit, View… */
function mnemonics(menus: Menu[]) {
  const used = new Set<string>();
  return menus.map((m) => {
    const letters = m.label.toLowerCase().split('').map((c, i) => [c, i] as const).filter(([c]) => /[a-z]/.test(c));
    const pick = letters.find(([c]) => !used.has(c));
    if (!pick) return -1;
    used.add(pick[0]);
    return pick[1];
  });
}

function Label({ text, at, show }: { text: string; at: number; show: boolean }) {
  if (at < 0 || !show) return <>{text}</>;
  return (
    <>
      {text.slice(0, at)}
      <u>{text[at]}</u>
      {text.slice(at + 1)}
    </>
  );
}

/** The classic Windows menu bar: click or Alt+letter opens a menu, arrows move, Enter runs, Esc closes. */
export default function MenuBar({ menus }: { menus: Menu[] }) {
  const [open, setOpen] = useState<number | null>(null);
  const [active, setActive] = useState(-1);
  const [altHeld, setAltHeld] = useState(false);
  const barRef = useRef<HTMLDivElement>(null);
  const keys = mnemonics(menus);

  const selectable = (m: number) =>
    menus[m].items.map((e, i) => (e !== 'separator' && !e.disabled ? i : -1)).filter((i) => i >= 0);

  const openMenu = (m: number, first = false) => {
    setOpen(m);
    setActive(first ? (selectable(m)[0] ?? -1) : -1);
  };
  const close = () => {
    setOpen(null);
    setActive(-1);
  };
  const run = (entry: MenuEntry) => {
    if (entry === 'separator' || entry.disabled) return;
    close();
    entry.onSelect();
  };

  // Alt+letter opens a menu; Alt on its own shows the underlined letters.
  useEffect(() => {
    const down = (e: KeyboardEvent) => {
      if (e.key === 'Alt') setAltHeld(true);
      if (e.altKey && !e.ctrlKey && /^[a-z]$/i.test(e.key)) {
        const m = menus.findIndex((menu, i) => keys[i] >= 0 && menu.label[keys[i]].toLowerCase() === e.key.toLowerCase());
        if (m >= 0) {
          e.preventDefault();
          openMenu(m, true);
        }
      }
    };
    const up = (e: KeyboardEvent) => {
      if (e.key === 'Alt') setAltHeld(false);
    };
    const blur = () => setAltHeld(false);
    window.addEventListener('keydown', down);
    window.addEventListener('keyup', up);
    window.addEventListener('blur', blur);
    return () => {
      window.removeEventListener('keydown', down);
      window.removeEventListener('keyup', up);
      window.removeEventListener('blur', blur);
    };
  });

  // While a menu is open the keyboard drives it.
  useEffect(() => {
    if (open === null) return;
    const onKey = (e: KeyboardEvent) => {
      const items = selectable(open);
      const pos = items.indexOf(active);
      const keysHandled: Record<string, () => void> = {
        Escape: close,
        ArrowDown: () => setActive(items[(pos + 1) % items.length] ?? -1),
        ArrowUp: () => setActive(items[(pos - 1 + items.length) % items.length] ?? -1),
        ArrowRight: () => openMenu((open + 1) % menus.length, true),
        ArrowLeft: () => openMenu((open - 1 + menus.length) % menus.length, true),
        Enter: () => active >= 0 && run(menus[open].items[active]),
      };
      const handler = keysHandled[e.key];
      if (handler) {
        e.preventDefault();
        e.stopImmediatePropagation();
        handler();
      }
    };
    const onClick = (e: MouseEvent) => {
      if (barRef.current && !barRef.current.contains(e.target as Node)) close();
    };
    window.addEventListener('keydown', onKey, true);
    document.addEventListener('mousedown', onClick);
    window.addEventListener('blur', close);
    return () => {
      window.removeEventListener('keydown', onKey, true);
      document.removeEventListener('mousedown', onClick);
      window.removeEventListener('blur', close);
    };
  });

  return (
    <div ref={barRef} role="menubar" className="desk-menubar">
      {menus.map((menu, m) => (
        <div key={menu.label} className="relative">
          <button
            type="button"
            role="menuitem"
            aria-haspopup="menu"
            aria-expanded={open === m}
            className={`desk-menubar-item ${open === m ? 'is-open' : ''}`}
            onMouseDown={(e) => {
              e.preventDefault();
              if (open === m) close();
              else openMenu(m);
            }}
            onMouseEnter={() => open !== null && open !== m && openMenu(m)}
          >
            <Label text={menu.label} at={keys[m]} show={altHeld || open !== null} />
          </button>
          {open === m && (
            <div role="menu" className="desk-menu">
              {menu.items.map((entry, i) =>
                entry === 'separator' ? (
                  <div key={`sep-${i}`} className="desk-menu-sep" role="separator" />
                ) : (
                  <button
                    key={entry.label}
                    type="button"
                    role="menuitem"
                    disabled={entry.disabled}
                    className={`desk-menu-item ${active === i ? 'is-active' : ''}`}
                    onMouseEnter={() => setActive(i)}
                    onClick={() => run(entry)}
                  >
                    <span>{entry.label}</span>
                    {entry.shortcut && <span className="desk-menu-key">{entry.shortcut}</span>}
                  </button>
                ),
              )}
            </div>
          )}
        </div>
      ))}
    </div>
  );
}
