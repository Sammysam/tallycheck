import { useEffect, useRef } from 'react';

/**
 * Page commands for the desktop menu bar, toolbar and keyboard shortcuts.
 * A page registers what "New" (Ctrl+N) and "Close" (Esc) mean on it; the shell
 * runs whichever handler the page on screen has registered.
 */
export type Command = 'new' | 'close';

const handlers = new Map<Command, () => void>();
const listeners = new Set<() => void>();

export function useCommand(command: Command, handler: () => void) {
  const latest = useRef(handler);
  useEffect(() => {
    latest.current = handler;
  });
  useEffect(() => {
    const run = () => latest.current();
    handlers.set(command, run);
    listeners.forEach((l) => l());
    return () => {
      if (handlers.get(command) === run) handlers.delete(command);
      listeners.forEach((l) => l());
    };
  }, [command]);
}

export function runCommand(command: Command): boolean {
  const handler = handlers.get(command);
  if (!handler) return false;
  handler();
  return true;
}

export function hasCommand(command: Command) {
  return handlers.has(command);
}

/** Re-render when pages add or remove commands (greys out menu items). */
export function subscribeCommands(listener: () => void) {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

// Whether the open form has typing that is not saved yet (asked before Esc closes it).
let unsaved = false;
export const setUnsaved = (value: boolean) => {
  unsaved = value;
};
export const isUnsaved = () => unsaved;
