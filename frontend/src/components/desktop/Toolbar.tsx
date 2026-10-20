import type { ComponentType } from 'react';

export interface ToolbarButton {
  label: string;
  title: string;
  icon: ComponentType<{ size?: number; strokeWidth?: number }>;
  onClick: () => void;
  disabled?: boolean;
}

/** Icon bar under the menu: small icon over a short label. `null` draws a divider. */
export default function Toolbar({ buttons }: { buttons: (ToolbarButton | null)[] }) {
  return (
    <div role="toolbar" aria-label="Main toolbar" className="desk-toolbar">
      {buttons.map((b, i) =>
        b === null ? (
          <span key={`div-${i}`} className="desk-toolbar-div" />
        ) : (
          <button
            key={b.label}
            type="button"
            title={b.title}
            disabled={b.disabled}
            onClick={b.onClick}
            className="desk-toolbar-btn"
          >
            <b.icon size={18} strokeWidth={1.75} />
            <span>{b.label}</span>
          </button>
        ),
      )}
    </div>
  );
}
