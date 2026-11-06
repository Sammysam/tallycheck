/**
 * D53: the same screens run on Windows and macOS. Shortcuts accept Ctrl or ⌘ everywhere
 * (`Layout.tsx`); this only changes how they are *written*, so a Mac shows ⌘ and ⌥.
 */
export const isMac = /Mac/i.test(navigator.platform || navigator.userAgent);

/** "Ctrl+P" → "⌘P" and "Alt+Left" → "⌥Left" on a Mac; unchanged on Windows. */
export const keyLabel = (keys: string): string =>
  isMac ? keys.replace(/Ctrl\+/g, '⌘').replace(/Alt\+/g, '⌥') : keys;
