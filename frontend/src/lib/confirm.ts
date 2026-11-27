/**
 * N3: the app's own yes / no question, asked from anywhere with `await askConfirm(…)`, instead
 * of the browser's confirm() box (which freezes the window and looks foreign). `ConfirmHost` in
 * the layout shows it. UI test scripts set `window.__tcAutoConfirm = true` to answer yes.
 */
export interface ConfirmRequest {
  id: number;
  message: string;
  title: string;
  confirmLabel: string;
  danger: boolean;
  resolve: (ok: boolean) => void;
}

let current: ConfirmRequest | null = null;
let nextId = 1;
const listeners = new Set<() => void>();
const emit = () => listeners.forEach((l) => l());

export function askConfirm(
  message: string,
  opts: { title?: string; confirmLabel?: string; danger?: boolean } = {},
): Promise<boolean> {
  if ((window as unknown as { __tcAutoConfirm?: boolean }).__tcAutoConfirm) return Promise.resolve(true);
  // A question still open is answered "no" before the next one is asked.
  current?.resolve(false);
  return new Promise((resolve) => {
    const danger = opts.danger ?? /delete|remove|void|stop/i.test(message);
    current = {
      id: nextId++,
      message,
      title: opts.title ?? 'Please confirm',
      confirmLabel: opts.confirmLabel ?? (danger ? 'Yes, do it' : 'OK'),
      danger,
      resolve: (ok) => {
        current = null;
        emit();
        resolve(ok);
      },
    };
    emit();
  });
}

export const currentConfirm = () => current;
export const subscribeConfirm = (l: () => void) => {
  listeners.add(l);
  return () => listeners.delete(l);
};
