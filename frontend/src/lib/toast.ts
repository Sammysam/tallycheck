/**
 * D36: small notices in the corner instead of the browser's alert() boxes. `toast()` from
 * anywhere; <ToastHost /> in the Layout shows them for a few seconds.
 */
export type ToastKind = 'success' | 'error' | 'info';
export interface Toast { id: number; kind: ToastKind; text: string }

let toasts: Toast[] = [];
let nextId = 1;
const listeners = new Set<() => void>();
const emit = () => listeners.forEach((l) => l());

export function toast(text: string, kind: ToastKind = 'success') {
  const t = { id: nextId++, kind, text };
  toasts = [...toasts, t];
  emit();
  setTimeout(() => dismiss(t.id), kind === 'error' ? 7000 : 4000);
}

export function dismiss(id: number) {
  toasts = toasts.filter((t) => t.id !== id);
  emit();
}

export const subscribeToasts = (l: () => void) => {
  listeners.add(l);
  return () => { listeners.delete(l); };
};
export const currentToasts = () => toasts;
