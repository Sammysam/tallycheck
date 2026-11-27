// D78: "What's new" — shown once after TallyCheck is updated to a version listed here, and any
// time from Help → What's new. Written in the app, so it works offline. Add the new version's
// points at the top with every release (the same points as the release notes).

export const WHATS_NEW: Record<string, string[]> = {
  '1.1.1': [
    'Getting started checklist on the dashboard that ticks itself off as you go.',
    'Backups are turned on in the setup wizard, and the first copy is made straight away.',
    'Opening balances for a business that is already running (Move books in → Quick start).',
    'Receive payment / Pay bill from an invoice or bill, with the amount filled in.',
    'Account lists offer the usual accounts first, with "+ New expense account…" on bills.',
    'Estimates / quotes that become invoices with one click.',
    'Recurring invoices: weekly, monthly, quarterly or yearly drafts.',
    'Automatic backups every few days, with Choose folder….',
    'US payroll in several states: 2026 state income tax, disability and paid leave, unemployment and local taxes.',
  ],
};

const SEEN_KEY = 'tc.whatsNew.seenVersion';

let pending: boolean | undefined;

/** True the first time this version runs after an update (and until the window is closed). */
export function whatsNewPending(current: string): boolean {
  if (pending === undefined) {
    let seen: string | null;
    try {
      seen = localStorage.getItem(SEEN_KEY);
      localStorage.setItem(SEEN_KEY, current);
    } catch {
      seen = current;
    }
    // A first start (nothing seen yet) is a new install, not an update: nothing to announce.
    pending = seen !== null && seen !== current && Boolean(WHATS_NEW[current]);
  }
  return pending;
}

export function whatsNewSeen() {
  pending = false;
}
