// D78: "What's new" — shown once after TallyCheck is updated to a version listed here, and any
// time from Help → What's new. Written in the app, so it works offline. Add the new version's
// points at the top with every release (the same points as the release notes).

export const WHATS_NEW: Record<string, string[]> = {
  '1.1.4': [
    'Sign in with your email. The first-run screen and New user ask for an email; usernames still work, and administrators can add an email to existing users (Users → ⋮ → Add email).',
    'US payroll: enter what employees were already paid this year before TallyCheck (US payroll → Payroll before TallyCheck). It counts towards the wage limits and Forms 941, 940 and W-2.',
    '"Save as file" on the recovery key screen now saves the key into your Downloads folder and says so.',
    'The activity log now says exactly what was done with each US pay run.',
  ],
  '1.1.3': [
    'US sales tax you pay to vendors is now part of the cost, and the tax return owes only the sales tax you collected. Each tax code says whether purchase tax can be claimed back (VAT / GST).',
    'The Viewer role is read-only: posting, reversing and reconciling need the new "Post to the ledger" right.',
    'Trial balance with net balances and an "as of" date; balance sheet with profit inside total equity; empty accounts hidden unless you ask.',
    'Cash flow counts balances brought in from your old system as opening cash; Quick start numbers equipment, stock and loans by kind.',
    'New bills are due in 30 days; account lists offer only the accounts that fit; questions now open in TallyCheck\'s own window.',
  ],
  '1.1.2': [
    'TallyCheck from the Microsoft Store now checks the Store for updates when an administrator opens it, and shows "Update to …" at the bottom of the window. Without internet it says nothing.',
    'The version number is at the bottom right; click it to see what is new.',
    'Security updates to the libraries TallyCheck uses.',
  ],
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
