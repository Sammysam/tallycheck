import { useEffect, useMemo, useState, type ChangeEvent } from 'react';
import { Link, useNavigate } from 'react-router-dom';
import { AlertTriangle, CheckCircle2, Download, FileSpreadsheet, Undo2, Upload } from 'lucide-react';
import api, { errMsg } from '../api/client';
import PageHeader from '../components/PageHeader';
import ConfirmDialog from '../components/ConfirmDialog';
import QuickStartForm, { type QuickStartBody } from '../components/QuickStartForm';
import { toast } from '../lib/toast';
import { useAuth } from '../contexts/AuthContext';
import { fmtDate, region, todayIso } from '../lib/region';
import { money } from '../lib/money';
import { columnLetter, guessDateOrder, type DateOrder } from '../lib/importFields';
import {
  SECTIONS, SOURCES, freeCode, guessColumns, rowsOf, sampleCsv, sectionInfo, splitNumber, suggestAccount, typeFrom,
  type AccountType, type ExistingAccount, type SectionKey,
} from '../lib/conversionFiles';

/**
 * D59: move books in from another accounting system on a cut-over date. Read the old system's
 * exports (trial balance, chart of accounts, unpaid invoices and bills, customers, vendors),
 * match its accounts to ours, check that everything adds up, then bring it all in at once.
 * Unpaid invoices and bills become posted documents against 3900 Opening Balance Equity; the
 * trial balance becomes the opening entry. A conversion can be reversed (nothing is deleted).
 */

interface Sheet { name: string; rows: string[][]; truncated: boolean }
interface Loaded { fileName: string; sheets: Sheet[]; sheet: number; headerRow: number; mapping: Record<string, number> }
interface AccountChoice { key: string; typeText: string; accountId: string; code: string; name: string; type: AccountType | '' }
interface Issue { section: string; row: number; label: string; message: string }
interface Outcome {
  id: string | null; name: string; canSave: boolean; issues: Issue[];
  totals: Record<string, number>;
  check: { debits: number; credits: number; receivablesBalance: number; openInvoices: number; payablesBalance: number; openBills: number; leftInOpeningEquity: number };
}
interface Conversion {
  id: string; source: string; cutoverDate: string; status: 'ACTIVE' | 'REVERSED'; openingEntryId?: string | null;
  createdByName?: string | null; createdAt: string; reversedAt?: string | null;
  summary?: { totals?: Record<string, number> };
}

const TYPES: [AccountType, string][] = [['ASSET', 'Asset'], ['LIABILITY', 'Liability'], ['EQUITY', 'Equity'], ['REVENUE', 'Income'], ['EXPENSE', 'Expense']];
const SECTION_NAMES: Record<string, string> = { accounts: 'Accounts', balances: 'Trial balance', invoices: 'Unpaid invoices', bills: 'Unpaid bills', customers: 'Customers', vendors: 'Vendors', general: 'Check' };
const DATE_ORDERS: [DateOrder, string][] = [['DMY', 'Day / month / year'], ['MDY', 'Month / day / year'], ['YMD', 'Year-month-day']];

/** The last day of last month: the usual cut-over. */
function lastMonthEnd(): string {
  const [y, m] = todayIso().split('-').map(Number);
  const d = new Date(Date.UTC(y, m - 1, 0));
  return d.toISOString().slice(0, 10);
}

function download(name: string, text: string) {
  const a = document.createElement('a');
  a.href = URL.createObjectURL(new Blob([text], { type: 'text/csv;charset=utf-8' }));
  a.download = name;
  a.click();
  URL.revokeObjectURL(a.href);
}

const norm = (s: string) => s.trim().replace(/\s+/g, ' ').toLowerCase();

export default function MoveBooks() {
  const { user } = useAuth();
  const [step, setStep] = useState<'start' | 'quick' | 'files' | 'accounts' | 'check' | 'done'>('start');
  // D59: quick start (typed figures) or the old system's files.
  const [mode, setMode] = useState<'quick' | 'files'>('quick');
  const [quickBody, setQuickBody] = useState<QuickStartBody | null>(null);
  const navigate = useNavigate();
  const [source, setSource] = useState(SOURCES[0]);
  const [cutover, setCutover] = useState(lastMonthEnd());
  const [dateOrder, setDateOrder] = useState<DateOrder>(/^en-US$/i.test(region().locale) ? 'MDY' : 'DMY');
  const [files, setFiles] = useState<Partial<Record<SectionKey, Loaded>>>({});
  const [reading, setReading] = useState<SectionKey | null>(null);
  const [existing, setExisting] = useState<ExistingAccount[]>([]);
  const [choices, setChoices] = useState<AccountChoice[]>([]);
  const [outcome, setOutcome] = useState<Outcome | null>(null);
  const [accept, setAccept] = useState(false);
  const [busy, setBusy] = useState(false);
  const [confirming, setConfirming] = useState(false);
  const [error, setError] = useState('');
  const [history, setHistory] = useState<Conversion[]>([]);
  const [reversing, setReversing] = useState<Conversion | null>(null);

  const loadHistory = () => api.get('/conversions').then((r) => setHistory(r.data)).catch(() => {});
  // Reloaded whenever a conversion starts, saves or is reversed: new codes must not clash.
  const loadAccounts = () => api.get('/accounts').then((r) => setExisting(r.data)).catch(() => {});
  useEffect(() => {
    loadAccounts();
    loadHistory();
  }, []);
  const active = history.find((c) => c.status === 'ACTIVE');

  // ── Reading files ────────────────────────────────────────────────────────
  const load = (key: SectionKey, fileName: string, sheets: Sheet[], sheet: number) => {
    const rows = sheets[sheet]?.rows ?? [];
    const { headerRow, mapping } = guessColumns(sectionInfo(key), rows);
    setFiles((f) => ({ ...f, [key]: { fileName, sheets, sheet, headerRow, mapping } }));
    // The day / month order, from the dates in an invoices or bills file.
    if (key === 'invoices' || key === 'bills') {
      const dates = rowsOf(sectionInfo(key), rows, headerRow, mapping).map((r) => r.date ?? '');
      if (dates.length) setDateOrder((d) => guessDateOrder(dates, d));
    }
  };
  const choose = (key: SectionKey) => async (e: ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0];
    e.target.value = '';
    if (!file) return;
    setError('');
    setReading(key);
    try {
      const form = new FormData();
      form.append('file', file);
      const r = await api.post('/imports/read', form);
      const sheets: Sheet[] = r.data.sheets;
      if (!sheets.some((s) => s.rows.length)) throw new Error('The file is empty.');
      // A workbook with a sheet named like the section (e.g. "Trial Balance") uses that sheet.
      const label = norm(sectionInfo(key).label);
      let sheet = sheets.findIndex((s) => norm(s.name).includes(label) || label.includes(norm(s.name)));
      if (sheet < 0) sheet = Math.max(0, sheets.findIndex((s) => s.rows.length > 0));
      load(key, r.data.fileName, sheets, sheet);
    } catch (err) {
      setError(errMsg(err, 'Could not read the file'));
    } finally {
      setReading(null);
    }
  };
  const rowsFor = (key: SectionKey): Record<string, string>[] => {
    const f = files[key];
    if (!f) return [];
    return rowsOf(sectionInfo(key), f.sheets[f.sheet]?.rows ?? [], f.headerRow, f.mapping);
  };
  const missingFields = (key: SectionKey) => {
    const f = files[key];
    if (!f) return [];
    return sectionInfo(key).fields.filter((x) => x.required && f.mapping[x.key] === undefined).map((x) => x.label);
  };

  // ── Accounts ─────────────────────────────────────────────────────────────
  const buildChoices = () => {
    const listed = rowsFor('accounts');
    const balances = rowsFor('balances');
    const typeOf = new Map<string, string>();
    const keys: string[] = [];
    const add = (key: string, type = '') => {
      const k = key.trim();
      if (!k) return;
      if (!typeOf.has(norm(k))) { keys.push(k); typeOf.set(norm(k), type); } else if (type && !typeOf.get(norm(k))) typeOf.set(norm(k), type);
    };
    for (const a of listed) {
      const name = a.account ?? '';
      add(a.number && !name.startsWith(a.number) ? `${a.number} ${name}` : name, a.type ?? '');
    }
    for (const b of balances) add(b.account ?? '');
    const taken = new Set(existing.map((a) => a.code));
    const previous = new Map(choices.map((c) => [norm(c.key), c]));
    const next = keys.map((key): AccountChoice => {
      const kept = previous.get(norm(key));
      if (kept) return kept;
      const typeText = typeOf.get(norm(key)) ?? '';
      const match = suggestAccount(key, typeText, existing);
      const split = splitNumber(key);
      const type = match ? (match.type as AccountType) : (typeFrom(typeText) || typeFrom(split.name));
      let code = '';
      if (!match) {
        code = split.code && !taken.has(split.code) ? split.code : freeCode(type, taken);
        if (code) taken.add(code);
      }
      return { key, typeText, accountId: match?.id ?? '', code, name: split.name, type };
    });
    setChoices(next);
  };
  const setChoice = (i: number, patch: Partial<AccountChoice>) => setChoices((cs) => cs.map((c, j) => (j === i ? { ...c, ...patch } : c)));
  const tbAmount = useMemo(() => {
    const m = new Map<string, string>();
    for (const b of rowsFor('balances')) m.set(norm(b.account ?? ''), b.debit ? `Dr ${b.debit}` : `Cr ${b.credit ?? ''}`);
    return m;
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [files]);
  const unchosen = choices.filter((c) => !c.accountId && (!c.code.trim() || !c.name.trim() || !c.type));

  // ── Preview and save ────────────────────────────────────────────────────
  // D75: what the Quick start figures say, in the form's own words (owns / owes / worth).
  const quickSummary = useMemo(() => {
    if (!quickBody) return null;
    const cents = (v: string) => Math.round(Number(v || 0) * 100);
    const lines = quickBody.balances.filter((b) => b.account !== 'Opening balance equity');
    const owns = lines.reduce((n, b) => n + cents(b.debit), 0) / 100;
    const owes = lines.reduce((n, b) => n + cents(b.credit), 0) / 100;
    return { owns, owes };
  }, [quickBody]);

  const payload = (acceptDifferences: boolean, quick: QuickStartBody | null = quickBody) => (mode === 'quick' && quick ? {
    source: 'Quick start', cutoverDate: cutover, dateOrder: 'YMD', acceptDifferences, ...quick,
  } : {
    source, cutoverDate: cutover, dateOrder, acceptDifferences,
    accounts: choices.map((c) => (c.accountId ? { key: c.key, accountId: c.accountId } : { key: c.key, code: c.code.trim(), name: c.name.trim(), type: c.type })),
    balances: rowsFor('balances').map((b) => ({ account: b.account ?? '', debit: b.debit ?? '', credit: b.credit ?? '' })),
    invoices: rowsFor('invoices').map((d) => ({ number: d.number ?? '', customer: d.customer ?? '', date: d.date ?? '', dueDate: d.dueDate ?? '', amount: d.amount ?? '' })),
    bills: rowsFor('bills').map((d) => ({ number: d.number ?? '', vendor: d.vendor ?? '', date: d.date ?? '', dueDate: d.dueDate ?? '', amount: d.amount ?? '' })),
    customers: rowsFor('customers'),
    vendors: rowsFor('vendors'),
  });
  const preview = async (acceptDifferences = accept, quick: QuickStartBody | null = quickBody) => {
    setBusy(true);
    setError('');
    try {
      const r = await api.post('/conversions/preview', payload(acceptDifferences, quick));
      setOutcome(r.data);
      setStep('check');
    } catch (err) {
      setError(errMsg(err, 'Could not check the files'));
    } finally {
      setBusy(false);
    }
  };
  const save = async () => {
    setBusy(true);
    setError('');
    try {
      const r = await api.post('/conversions', payload(accept));
      setOutcome(r.data);
      setConfirming(false);
      setStep('done');
      toast('The books are moved in');
      loadHistory();
      loadAccounts();
    } catch (err) {
      setConfirming(false);
      setError(errMsg(err, 'Nothing was saved'));
    } finally {
      setBusy(false);
    }
  };
  const reverse = async () => {
    if (!reversing) return;
    setBusy(true);
    try {
      await api.post(`/conversions/${reversing.id}/reverse`);
      toast('The conversion is reversed');
      setReversing(null);
      loadHistory();
      loadAccounts();
    } catch (err) {
      toast(errMsg(err, 'Could not reverse it'), 'error');
      setReversing(null);
    } finally {
      setBusy(false);
    }
  };
  const restart = () => {
    setStep('start'); setFiles({}); setChoices([]); setQuickBody(null); setOutcome(null); setAccept(false); setError('');
  };


  const differences = outcome?.issues.some((i) => i.section === 'general') ?? false;
  const otherIssues = outcome?.issues.filter((i) => i.section !== 'general') ?? [];

  const stepper = (
    <ol className="flex items-center gap-2 text-xs mb-4" aria-label="Steps">
      {(mode === 'quick' ? ['Start', 'Your figures', 'Check', 'Done'] : ['Start', 'Files', 'Accounts', 'Check', 'Done']).map((label, i, all) => {
        const at = (mode === 'quick' ? ['start', 'quick', 'check', 'done'] : ['start', 'files', 'accounts', 'check', 'done']).indexOf(step);
        return (
          <li key={label} className="flex items-center gap-2">
            <span className={`w-5 h-5 rounded-full grid place-items-center text-[11px] font-semibold ${i <= at ? 'bg-orange-600 text-white' : 'bg-gray-200 text-gray-600'}`}>{i + 1}</span>
            <span className={i === at ? 'font-semibold text-gray-900' : 'text-gray-500'}>{label}</span>
            {i < all.length - 1 && <span className="w-6 h-px bg-gray-300" />}
          </li>
        );
      })}
    </ol>
  );

  if (user?.role !== 'Admin') {
    return (
      <div>
        <PageHeader title="Move Books In" />
        <p className="text-sm text-gray-600">Only an administrator can move books in from another system.</p>
      </div>
    );
  }

  return (
    <div>
      <PageHeader
        title="Move Books In"
        subtitle="Bring your books over from QuickBooks, Xero, Tally or another system, starting from a cut-over date."
        action={step !== 'start' && step !== 'done' ? <button className="btn btn-secondary" onClick={restart}>Start again</button> : undefined}
      />
      {stepper}
      {error && <div className="mb-3 px-3 py-2 rounded border border-red-200 bg-red-50 text-sm text-red-700" role="alert">{error}</div>}

      {step === 'start' && (
        <>
          {active && (
            <div className="mb-3 px-3 py-2 rounded border border-amber-200 bg-amber-50 text-sm text-amber-800 flex gap-2" role="status">
              <AlertTriangle size={16} className="shrink-0 mt-0.5" />
              <span>Books were already moved in from {active.source} (cut-over {fmtDate(active.cutoverDate)}). Reverse that conversion below to do it again.</span>
            </div>
          )}
          <section className="card mb-3">
            <h3 className="text-sm font-semibold text-gray-800 mb-2">How do you want to start?</h3>
            <div className="grid gap-2 sm:grid-cols-2 mb-4" role="radiogroup" aria-label="How to start">
              {([
                ['quick', 'Quick start (a few minutes)', 'No files needed. Type your bank balances, who owes you, whom you owe, and anything else the business owns or owes. Best if your old system is custom-made or you just want to try TallyCheck.'],
                ['files', 'From your old system’s files', 'Upload the trial balance and, if you have them, the account list, unpaid invoices and bills, customers and vendors exported from QuickBooks, Xero, Tally or Excel.'],
              ] as const).map(([value, title, text]) => (
                <button
                  key={value}
                  type="button"
                  role="radio"
                  aria-checked={mode === value}
                  className={`text-left rounded border px-3 py-2 ${mode === value ? 'border-orange-500 bg-orange-50 ring-1 ring-orange-500' : 'border-gray-200 hover:border-gray-300'}`}
                  onClick={() => setMode(value)}
                >
                  <div className="text-sm font-semibold text-gray-900">{title}</div>
                  <div className="text-xs text-gray-600 mt-0.5">{text}</div>
                </button>
              ))}
            </div>
            <p className="text-sm text-gray-700 mb-3">
              Pick a <b>cut-over date</b>, usually the last day of a month: the balances are as at the end of that day, and
              TallyCheck carries on from the next day. Unpaid invoices and bills come in one by one, so they can be paid here
              as usual, and everything is checked to add up before anything is saved.
            </p>
            <div className="grid gap-3 sm:grid-cols-3">
              {mode === 'files' && (
                <label className="text-xs font-medium text-gray-600">Coming from
                  <select className="mt-1 w-full" value={source} onChange={(e) => setSource(e.target.value)}>
                    {SOURCES.map((s) => <option key={s}>{s}</option>)}
                  </select>
                </label>
              )}
              <label className="text-xs font-medium text-gray-600">Cut-over date
                <input className="mt-1 w-full" type="date" value={cutover} max={todayIso()} onChange={(e) => setCutover(e.target.value)} />
              </label>
              {mode === 'files' && (
                <label className="text-xs font-medium text-gray-600">Dates in the files
                  <select className="mt-1 w-full" value={dateOrder} onChange={(e) => setDateOrder(e.target.value as DateOrder)}>
                    {DATE_ORDERS.map(([v, l]) => <option key={v} value={v}>{l}</option>)}
                  </select>
                </label>
              )}
            </div>
            <div className="mt-4 flex gap-2">
              <button className="btn btn-primary" disabled={!!active || !cutover} onClick={() => { loadAccounts(); setStep(mode); }}>
                {mode === 'quick' ? 'Next: enter your figures' : 'Next: choose the files'}
              </button>
            </div>
          </section>

          <section className="card">
            <h3 className="text-sm font-semibold text-gray-800 mb-2">Earlier conversions</h3>
            {history.length === 0 ? <p className="text-sm text-gray-500">None yet.</p> : (
              <table className="w-full text-sm">
                <thead><tr className="text-left text-xs text-gray-500"><th>From</th><th>Cut-over</th><th>Brought in</th><th>By</th><th>Status</th><th /></tr></thead>
                <tbody>
                  {history.map((c) => (
                    <tr key={c.id} className="border-t border-gray-100">
                      <td className="py-1.5">{c.source}</td>
                      <td>{fmtDate(c.cutoverDate)}</td>
                      <td className="text-xs text-gray-600">
                        {c.summary?.totals ? `${c.summary.totals.invoices ?? 0} invoices, ${c.summary.totals.bills ?? 0} bills, ${c.summary.totals.accountsCreated ?? 0} new accounts` : ''}
                      </td>
                      <td>{c.createdByName ?? ''}</td>
                      <td>{c.status === 'ACTIVE' ? 'In force' : `Reversed ${c.reversedAt ? fmtDate(c.reversedAt.slice(0, 10)) : ''}`}</td>
                      <td className="text-right">
                        {c.status === 'ACTIVE' && (
                          <button className="btn btn-secondary" onClick={() => setReversing(c)}><Undo2 size={14} /> Reverse</button>
                        )}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
            )}
          </section>
        </>
      )}

      {/* Kept mounted on the check step, so Back returns to the figures as typed. */}
      {mode === 'quick' && (step === 'quick' || step === 'check') && (
        <div hidden={step !== 'quick'}>
        <QuickStartForm
          existing={existing}
          busy={busy}
          onBack={() => setStep('start')}
          onCheck={(body) => { setQuickBody(body); preview(accept, body); }}
        />
        </div>
      )}

      {step === 'files' && (
        <>
          <div className="grid gap-3" style={{ gridTemplateColumns: 'repeat(auto-fill, minmax(340px, 1fr))' }}>
            {SECTIONS.map((s) => {
              const f = files[s.key];
              const rows = rowsFor(s.key);
              const missing = missingFields(s.key);
              const headings = f ? (f.sheets[f.sheet]?.rows[f.headerRow] ?? []) : [];
              return (
                <section key={s.key} className="card" aria-label={s.label}>
                  <div className="flex items-start justify-between gap-2">
                    <div>
                      <h3 className="text-sm font-semibold text-gray-800 flex items-center gap-1.5">
                        <FileSpreadsheet size={15} /> {s.label} {s.optional ? <span className="text-xs font-normal text-gray-500">(if you have it)</span> : <span className="text-xs font-normal text-red-600">needed</span>}
                      </h3>
                      <p className="text-xs text-gray-600 mt-0.5">{s.description}</p>
                      {source.startsWith('QuickBooks Online') && <p className="text-xs text-gray-500 mt-0.5">QuickBooks: {s.where}</p>}
                    </div>
                  </div>
                  <div className="mt-2 flex flex-wrap items-center gap-2">
                    <label className="btn btn-secondary cursor-pointer">
                      <Upload size={14} /> {reading === s.key ? 'Reading…' : f ? 'Choose another file' : 'Choose file'}
                      <input type="file" className="hidden" accept=".csv,.txt,.xlsx,.xlsm,.xls,.ods" onChange={choose(s.key)} disabled={reading !== null} />
                    </label>
                    <button type="button" className="text-xs text-blue-700 hover:underline inline-flex items-center gap-1" onClick={() => download(`${s.label}.csv`, sampleCsv(s))}>
                      <Download size={12} /> Template
                    </button>
                    {f && <button type="button" className="text-xs text-gray-600 hover:underline" onClick={() => setFiles((x) => { const n = { ...x }; delete n[s.key]; return n; })}>Remove</button>}
                  </div>
                  {f && (
                    <div className="mt-2 text-xs">
                      <div className="text-gray-700 mb-1"><b>{f.fileName}</b>{f.sheets.length > 1 && (
                        <> · sheet <select className="text-xs" value={f.sheet} onChange={(e) => load(s.key, f.fileName, f.sheets, Number(e.target.value))}>
                          {f.sheets.map((sh, i) => <option key={sh.name} value={i}>{sh.name}</option>)}
                        </select></>
                      )} · {rows.length} row{rows.length === 1 ? '' : 's'}</div>
                      <div className="grid grid-cols-2 gap-x-2 gap-y-1">
                        {s.fields.map((fld) => (
                          <label key={fld.key} className="text-gray-600">{fld.label}{fld.required ? ' *' : ''}
                            <select
                              className="w-full text-xs"
                              value={f.mapping[fld.key] ?? ''}
                              onChange={(e) => {
                                const v = e.target.value;
                                setFiles((x) => {
                                  const cur = x[s.key]!;
                                  const mapping = { ...cur.mapping };
                                  if (v === '') delete mapping[fld.key]; else mapping[fld.key] = Number(v);
                                  return { ...x, [s.key]: { ...cur, mapping } };
                                });
                              }}
                            >
                              <option value="">{fld.key === s.groupField ? '(names on their own rows)' : '(none)'}</option>
                              {headings.map((h, i) => <option key={i} value={i}>{columnLetter(i)} · {h || '(blank)'}</option>)}
                            </select>
                          </label>
                        ))}
                      </div>
                      {missing.length > 0 && <p className="mt-1 text-red-700">Still to match: {missing.join(', ')}</p>}
                    </div>
                  )}
                </section>
              );
            })}
          </div>
          <div className="mt-3 flex gap-2">
            <button className="btn btn-secondary" onClick={() => setStep('start')}>Back</button>
            <button
              className="btn btn-primary"
              disabled={!files.balances || SECTIONS.some((s) => missingFields(s.key).length > 0)}
              onClick={() => { buildChoices(); setStep('accounts'); }}
            >Next: match the accounts</button>
          </div>
        </>
      )}

      {step === 'accounts' && (
        <section className="card">
          <h3 className="text-sm font-semibold text-gray-800 mb-1">Match each {source} account to a TallyCheck account</h3>
          <p className="text-xs text-gray-600 mb-3">
            Matches are suggested by name; anything without a match is created with the code shown. Receivables must go to
            1100 Accounts Receivable and payables to 2000 Accounts Payable, so the unpaid invoices and bills can stand in for them.
          </p>
          <div className="overflow-x-auto">
            <table className="w-full text-sm">
              <thead><tr className="text-left text-xs text-gray-500"><th className="py-1">{source} account</th><th>Balance</th><th>TallyCheck account</th><th>Code</th><th>Name</th><th>Type</th></tr></thead>
              <tbody>
                {choices.map((c, i) => (
                  <tr key={c.key} className="border-t border-gray-100 align-top">
                    <td className="py-1.5 pr-2">{c.key}{c.typeText && <div className="text-xs text-gray-500">{c.typeText}</div>}</td>
                    <td className="pr-2 text-xs text-gray-600 whitespace-nowrap">{tbAmount.get(norm(c.key)) ?? ''}</td>
                    <td className="pr-2">
                      <select className="w-full" value={c.accountId} onChange={(e) => setChoice(i, { accountId: e.target.value })} aria-label={`TallyCheck account for ${c.key}`}>
                        <option value="">+ Create a new account</option>
                        {existing.filter((a) => a.isActive).map((a) => <option key={a.id} value={a.id}>{a.code} {a.name}</option>)}
                      </select>
                    </td>
                    {c.accountId ? <td colSpan={3} className="text-xs text-gray-500">Uses the existing account</td> : (
                      <>
                        <td className="pr-2"><input className="w-20" value={c.code} onChange={(e) => setChoice(i, { code: e.target.value })} aria-label={`Code for ${c.key}`} /></td>
                        <td className="pr-2"><input className="w-full" value={c.name} onChange={(e) => setChoice(i, { name: e.target.value })} aria-label={`Name for ${c.key}`} /></td>
                        <td>
                          <select value={c.type} onChange={(e) => setChoice(i, { type: e.target.value as AccountType })} aria-label={`Type for ${c.key}`}>
                            <option value="">Choose…</option>
                            {TYPES.map(([v, l]) => <option key={v} value={v}>{l}</option>)}
                          </select>
                        </td>
                      </>
                    )}
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
          {unchosen.length > 0 && <p className="mt-2 text-xs text-red-700">{unchosen.length} new account{unchosen.length === 1 ? ' needs' : 's need'} a code, a name and a type.</p>}
          <div className="mt-3 flex gap-2">
            <button className="btn btn-secondary" onClick={() => setStep('files')}>Back</button>
            <button className="btn btn-primary" disabled={busy || unchosen.length > 0 || choices.length === 0} onClick={() => preview()}>
              {busy ? 'Checking…' : 'Next: check everything'}
            </button>
          </div>
        </section>
      )}

      {step === 'check' && outcome && (
        <section className="card">
          <h3 className="text-sm font-semibold text-gray-800 mb-2">Check before anything is saved</h3>
          <div className="grid gap-2 mb-3" style={{ gridTemplateColumns: 'repeat(auto-fill, minmax(150px, 1fr))' }}>
            {[
              ['Accounts matched', outcome.totals.accountsMatched], ['New accounts', outcome.totals.accountsCreated],
              ['New customers', outcome.totals.customersCreated], ['New vendors', outcome.totals.vendorsCreated],
              ['Unpaid invoices', outcome.totals.invoices], ['Unpaid bills', outcome.totals.bills],
            ].map(([l, v]) => (
              <div key={l as string} className="rounded border border-gray-200 px-3 py-2">
                <div className="text-xs text-gray-500">{l}</div><div className="text-lg font-semibold">{v as number}</div>
              </div>
            ))}
          </div>
          {/* D75: Quick start checks in the words of its own form; the accountant's table is for files. */}
          {mode === 'quick' && quickSummary ? (
            <table className="text-sm mb-3 report-table" aria-label="Your business today">
              <tbody>
                <tr><td className="pr-6">The business owns (bank, cash, what customers owe, other things)</td><td className="text-right">{money(quickSummary.owns)}</td></tr>
                <tr><td className="pr-6">The business owes (suppliers, loans, other debts)</td><td className="text-right">{money(quickSummary.owes)}</td></tr>
                <tr className="font-semibold"><td className="pr-6">So it is worth (the owner’s share)</td><td className="text-right">{money(quickSummary.owns - quickSummary.owes)}</td></tr>
                <tr><td className="pr-6">Customers owe you</td><td className="text-right">{money(outcome.check.openInvoices)}</td></tr>
                <tr><td className="pr-6">You owe suppliers</td><td className="text-right">{money(outcome.check.openBills)}</td></tr>
              </tbody>
            </table>
          ) : (
          <table className="text-sm mb-3 report-table">
            <tbody>
              <tr><td className="pr-6">Trial balance debits / credits</td><td className="text-right">{money(outcome.check.debits)}</td><td className="text-right pl-4">{money(outcome.check.credits)}</td></tr>
              <tr><td className="pr-6">Receivables in the trial balance / unpaid invoices</td><td className="text-right">{money(outcome.check.receivablesBalance)}</td><td className="text-right pl-4">{money(outcome.check.openInvoices)}</td></tr>
              <tr><td className="pr-6">Payables in the trial balance / unpaid bills</td><td className="text-right">{money(outcome.check.payablesBalance)}</td><td className="text-right pl-4">{money(outcome.check.openBills)}</td></tr>
              <tr><td className="pr-6">Difference left in 3900 Opening Balance Equity</td><td className="text-right" colSpan={2}>{money(outcome.check.leftInOpeningEquity)}</td></tr>
            </tbody>
          </table>
          )}

          {otherIssues.length > 0 && (
            <div className="mb-3">
              <p className="text-sm font-semibold text-red-700 mb-1">{otherIssues.length} problem{otherIssues.length === 1 ? '' : 's'} to fix{mode === 'files' ? ' in the files' : ''}</p>
              <ul className="text-sm text-red-700 space-y-0.5 max-h-72 overflow-auto">
                {otherIssues.map((i, n) => (
                  <li key={n}>{SECTION_NAMES[i.section] ?? i.section}{i.row ? `, row ${i.row}` : ''}{i.label ? ` (${i.label})` : ''}: {i.message}</li>
                ))}
              </ul>
            </div>
          )}
          {(differences || accept) && (
            <div className="mb-3 px-3 py-2 rounded border border-amber-200 bg-amber-50 text-sm text-amber-900">
              {outcome.issues.filter((i) => i.section === 'general').map((i, n) => <p key={n}>{i.message}</p>)}
              <label className="mt-1 flex items-center gap-2">
                <input type="checkbox" checked={accept} onChange={(e) => { setAccept(e.target.checked); preview(e.target.checked); }} />
                Accept the difference (it stays in 3900 Opening Balance Equity for your accountant to clear)
              </label>
            </div>
          )}
          {outcome.canSave && (
            <p className="mb-3 text-sm text-green-700 flex items-center gap-1.5"><CheckCircle2 size={16} /> Everything adds up. Nothing has been saved yet.</p>
          )}
          <div className="flex gap-2">
            <button className="btn btn-secondary" onClick={() => setStep(mode === 'quick' ? 'quick' : 'accounts')}>Back</button>
            <button className="btn btn-primary" disabled={!outcome.canSave || busy} onClick={() => setConfirming(true)}>Move the books in</button>
          </div>
        </section>
      )}

      {step === 'done' && outcome && (
        <section className="card">
          <p className="text-sm text-green-700 flex items-center gap-1.5 mb-2"><CheckCircle2 size={16} /> {outcome.name.replace(/\d{4}-\d{2}-\d{2}/g, (d) => fmtDate(d))}.</p>
          <ul className="text-sm list-disc pl-5 space-y-1 mb-3">
            {mode === 'quick'
              ? <li>See where the business stands on the <Link className="text-blue-700 hover:underline" to="/reports/balance-sheet">Balance Sheet</Link> as of {fmtDate(cutover)}.</li>
              : <li>Open the <Link className="text-blue-700 hover:underline" to="/reports/trial-balance">Trial Balance</Link> as of {fmtDate(cutover)} and compare it with the old system’s.</li>}
            <li>The unpaid invoices and bills are on the <Link className="text-blue-700 hover:underline" to="/invoices">Invoices</Link> and <Link className="text-blue-700 hover:underline" to="/bills">Bills</Link> screens, ready to be paid.</li>
            <li>Then <Link className="text-blue-700 hover:underline" to="/settings?tab=closing">lock the books</Link> through {fmtDate(cutover)}, so nothing is changed before the cut-over by mistake.</li>
          </ul>
          <div className="flex gap-2">
            <button className="btn btn-primary" onClick={() => navigate('/')}>Go to my dashboard</button>
            <button className="btn btn-secondary" onClick={restart}>Back to the start</button>
          </div>
        </section>
      )}

      {confirming && (
        <ConfirmDialog title="Move the books in?" confirmLabel="Move the books in" busy={busy} onConfirm={save} onCancel={() => setConfirming(false)}>
          <p>This creates the accounts, customers and vendors, posts {outcome?.totals.invoices ?? 0} unpaid invoice(s) and {outcome?.totals.bills ?? 0} unpaid bill(s), and posts the opening balances on {fmtDate(cutover)}.</p>
          <p>It can be reversed later as long as none of those invoices or bills has been paid.</p>
        </ConfirmDialog>
      )}
      {reversing && (
        <ConfirmDialog title="Reverse this conversion?" confirmLabel="Reverse" danger busy={busy} onConfirm={reverse} onCancel={() => setReversing(null)}>
          <p>The invoices and bills it brought in are voided and its opening entry is reversed, dated today. Nothing is deleted.</p>
          <p>The accounts, customers and vendors it created stay. Invoices or bills that have payments must have them voided first.</p>
        </ConfirmDialog>
      )}
    </div>
  );
}
