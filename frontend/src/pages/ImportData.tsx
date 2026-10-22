import { useEffect, useMemo, useState, type ChangeEvent } from 'react';
import { useNavigate } from 'react-router-dom';
import { CheckCircle2, Download, FileSpreadsheet, Landmark, AlertTriangle, Receipt, Scale, Truck, Undo2, Upload, Users } from 'lucide-react';
import api, { errMsg } from '../api/client';
import PageHeader from '../components/PageHeader';
import ConfirmDialog from '../components/ConfirmDialog';
import { useAuth } from '../contexts/AuthContext';
import { toast } from '../lib/toast';
import { fmtDate, region, todayIso } from '../lib/region';
import { money } from '../lib/money';
import {
  KINDS, columnLetter, guessDateOrder, guessMapping, kindInfo, sampleCsv,
  type DateOrder, type ImportKind,
} from '../lib/importFields';

/**
 * D37: import from file. Choose what to import, choose a CSV or Excel file, match its columns to
 * ours (remembered as a named layout), check the preview, import. Everything arrives as drafts or
 * as bank lines waiting to be matched; an import can be undone while nothing has used it.
 */

interface Sheet { name: string; rows: string[][]; truncated: boolean }
interface Account { id: string; code: string; name: string; type: string; currency?: string | null; isActive: boolean }
interface RowResult { row: number; status: 'NEW' | 'DUPLICATE' | 'ERROR'; label: string; message?: string | null }
interface Outcome { id: string | null; name: string; created: number; skipped: number; errors: number; rows: RowResult[] }
interface ImportRecord {
  id: string; kind: string; source: string; fileName?: string | null; accountName?: string | null;
  created: number; skipped: number; status: string; createdBy?: string | null; createdAt: string;
}
interface Layout { id: string; kind: string; name: string; layout: SavedLayout }
interface SavedLayout { hasHeader: boolean; dateOrder: DateOrder; columns: Record<string, string | number> }
interface OpeningLine { row: number; accountId?: string; code: string; debit: number; credit: number; note: string; error?: string }

const ICONS: Record<ImportKind, typeof Landmark> = { BANK: Landmark, INVOICES: Receipt, CUSTOMERS: Users, VENDORS: Truck, OPENING: Scale };
const STATUS_STYLE: Record<RowResult['status'], string> = {
  NEW: 'bg-green-50 text-green-700 border-green-200',
  DUPLICATE: 'bg-gray-50 text-gray-600 border-gray-200',
  ERROR: 'bg-red-50 text-red-700 border-red-200',
};
const STATUS_TEXT: Record<RowResult['status'], string> = { NEW: 'New', DUPLICATE: 'Already there', ERROR: 'Error' };
const DATE_ORDERS: [DateOrder, string][] = [['DMY', 'Day / month / year (31/12/2026)'], ['MDY', 'Month / day / year (12/31/2026)'], ['YMD', 'Year-month-day (2026-12-31)']];

const defaultDateOrder = (): DateOrder => (/^en-US$/i.test(region().locale) ? 'MDY' : 'DMY');

function download(name: string, text: string) {
  const a = document.createElement('a');
  a.href = URL.createObjectURL(new Blob([text], { type: 'text/csv;charset=utf-8' }));
  a.download = name;
  a.click();
  URL.revokeObjectURL(a.href);
}

/** Exact cents from text such as "1,234.50", "(250.00)" or "Rs. 1,000", for the opening-balance preview. */
function cents(text: string): number | null {
  let t = text.trim();
  if (!t) return 0;
  let neg = false;
  if (/^\(.*\)$/.test(t)) { neg = true; t = t.slice(1, -1); }
  t = t.replace(/[^0-9.-]/g, '');
  if (t.startsWith('-')) { neg = !neg; t = t.slice(1); }
  if (!/^\d+(\.\d{1,2})?$/.test(t)) return null;
  const [i, f = ''] = t.split('.');
  const c = Number(i) * 100 + Number((f + '00').slice(0, 2));
  return neg ? -c : c;
}

export default function ImportData() {
  const navigate = useNavigate();
  const { hasPermission } = useAuth();
  const kinds = KINDS.filter((k) => hasPermission(k.permission));
  const [step, setStep] = useState<'start' | 'map' | 'preview' | 'done'>('start');
  const [kind, setKind] = useState<ImportKind>(kinds[0]?.kind ?? 'BANK');
  const info = kindInfo(kind);

  const [accounts, setAccounts] = useState<Account[]>([]);
  const [history, setHistory] = useState<ImportRecord[]>([]);
  const [layouts, setLayouts] = useState<Layout[]>([]);

  // The file
  const [fileName, setFileName] = useState('');
  const [sheets, setSheets] = useState<Sheet[]>([]);
  const [sheetIndex, setSheetIndex] = useState(0);
  const [reading, setReading] = useState(false);

  // The mapping
  const [hasHeader, setHasHeader] = useState(true);
  const [mapping, setMapping] = useState<Record<string, number>>({});
  const [dateOrder, setDateOrder] = useState<DateOrder>(defaultDateOrder());
  const [source, setSource] = useState('');
  const [accountId, setAccountId] = useState('');
  const [defaultAccountId, setDefaultAccountId] = useState('');
  const [openingDate, setOpeningDate] = useState(todayIso());
  const [layoutName, setLayoutName] = useState('');

  // Preview / result
  const [outcome, setOutcome] = useState<Outcome | null>(null);
  const [opening, setOpening] = useState<OpeningLine[] | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const [onlyProblems, setOnlyProblems] = useState(false);
  const [undoing, setUndoing] = useState<ImportRecord | null>(null);

  const loadHistory = () => api.get('/imports').then((r) => setHistory(r.data)).catch(() => {});
  useEffect(() => {
    api.get('/accounts').then((r) => {
      const list: Account[] = r.data;
      setAccounts(list);
      setDefaultAccountId(list.find((a) => a.code === '4000')?.id ?? '');
    });
    loadHistory();
  }, []);
  useEffect(() => {
    api.get('/imports/layouts', { params: { kind } }).then((r) => setLayouts(r.data)).catch(() => setLayouts([]));
  }, [kind]);

  const sheet = sheets[sheetIndex];
  const allRows = useMemo(() => sheet?.rows ?? [], [sheet]);
  const width = Math.max(0, ...allRows.slice(0, 50).map((r) => r.length));
  const headings = hasHeader && allRows[0] ? Array.from({ length: width }, (_, i) => allRows[0][i] ?? '') : [];
  const dataRows = hasHeader ? allRows.slice(1) : allRows;
  const columnName = (i: number) => (headings[i] ? `${columnLetter(i)} · ${headings[i]}` : `Column ${columnLetter(i)}`);

  const bankAccounts = accounts.filter((a) => a.isActive && (a.type === 'ASSET' || a.type === 'LIABILITY'));
  const revenueAccounts = accounts.filter((a) => a.isActive && a.type === 'REVENUE');

  const reset = () => {
    setStep('start'); setFileName(''); setSheets([]); setSheetIndex(0); setMapping({}); setOutcome(null);
    setOpening(null); setError(''); setLayoutName(''); setOnlyProblems(false);
  };

  const applyGuess = (rows: string[][], header: boolean) => {
    const w = Math.max(0, ...rows.slice(0, 50).map((r) => r.length));
    const heads = header && rows[0] ? Array.from({ length: w }, (_, i) => rows[0][i] ?? '') : [];
    const m = guessMapping(info.fields, heads);
    setMapping(m);
    const dateCol = m.date;
    if (dateCol !== undefined) {
      setDateOrder(guessDateOrder((header ? rows.slice(1) : rows).slice(0, 200).map((r) => r[dateCol] ?? ''), defaultDateOrder()));
    }
  };

  const chooseFile = async (e: ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0];
    e.target.value = '';
    if (!file) return;
    setError('');
    setReading(true);
    try {
      const form = new FormData();
      form.append('file', file);
      const r = await api.post('/imports/read', form);
      const found: Sheet[] = r.data.sheets;
      const first = Math.max(0, found.findIndex((s) => s.rows.length > 0));
      if (!found[first]?.rows.length) throw new Error('The file is empty.');
      setFileName(r.data.fileName);
      setSheets(found);
      setSheetIndex(first);
      setHasHeader(true);
      setSource((s) => s || file.name.replace(/\.[^.]+$/, ''));
      applyGuess(found[first].rows, true);
      setStep('map');
    } catch (err) {
      setError(errMsg(err, 'Could not read the file'));
    } finally {
      setReading(false);
    }
  };

  const applyLayout = (id: string) => {
    const l = layouts.find((x) => x.id === id);
    if (!l) return;
    const saved = l.layout;
    setHasHeader(saved.hasHeader);
    setDateOrder(saved.dateOrder);
    const heads = saved.hasHeader && allRows[0] ? allRows[0].map((h) => h.trim().toLowerCase()) : [];
    const m: Record<string, number> = {};
    for (const [field, col] of Object.entries(saved.columns)) {
      const i = typeof col === 'number' ? col : heads.indexOf(String(col).trim().toLowerCase());
      if (i >= 0) m[field] = i;
    }
    setMapping(m);
    setSource(l.name);
    setLayoutName(l.name);
    toast(`Layout "${l.name}" applied.`, 'info');
  };

  const saveLayout = async () => {
    const name = layoutName.trim();
    if (!name) { toast('Name the layout first, for example the bank’s name.', 'error'); return; }
    const columns: Record<string, string | number> = {};
    for (const [field, i] of Object.entries(mapping)) columns[field] = hasHeader && headings[i] ? headings[i] : i;
    try {
      await api.put('/imports/layouts', { kind, name, layout: { hasHeader, dateOrder, columns } });
      const r = await api.get('/imports/layouts', { params: { kind } });
      setLayouts(r.data);
      toast(`Layout "${name}" saved. Next time choose it and the columns are set for you.`);
    } catch (err) {
      toast(errMsg(err, 'Could not save the layout'), 'error');
    }
  };

  const missing = info.fields.filter((f) => f.required && mapping[f.key] === undefined)
    .filter((f) => !(kind === 'BANK' && f.key === 'description' && mapping.reference !== undefined));
  const bankAmountMissing = kind === 'BANK' && mapping.amount === undefined && mapping.moneyIn === undefined && mapping.moneyOut === undefined;
  const openingAmountMissing = kind === 'OPENING' && mapping.debit === undefined && mapping.credit === undefined;

  const mappedRows = () => dataRows.map((r) => {
    const o: Record<string, string> = {};
    for (const [field, i] of Object.entries(mapping)) o[field] = r[i] ?? '';
    return o;
  });

  const body = () => ({
    kind, source: source.trim(), fileName, dateOrder, rows: mappedRows(),
    accountId: kind === 'BANK' ? accountId : undefined,
    defaultAccountId: kind === 'INVOICES' ? defaultAccountId || undefined : undefined,
  });

  const preview = async () => {
    setError('');
    if (missing.length || bankAmountMissing || openingAmountMissing) return;
    if (kind === 'BANK' && !accountId) { setError('Choose the bank account this statement belongs to.'); return; }
    if (kind === 'OPENING') {
      const byCode = new Map(accounts.map((a) => [a.code, a]));
      const lines: OpeningLine[] = mappedRows().map((r, i) => {
        const code = (r.account ?? '').trim().split(/\s+/)[0] ?? '';
        const [d, c] = [cents(r.debit ?? ''), cents(r.credit ?? '')];
        const acct = byCode.get(code);
        const line: OpeningLine = { row: i + 1, code, accountId: acct?.id, debit: d ?? 0, credit: c ?? 0, note: (r.description ?? '').trim() };
        if (!code) line.error = 'No account code';
        else if (!acct) line.error = `No account with code ${code}`;
        else if (d === null || c === null) line.error = 'The amount is not a number';
        else if (d < 0 || c < 0) line.error = 'Use the other column instead of a minus';
        return line;
      }).filter((l) => l.error || l.debit || l.credit);
      setOpening(lines);
      setStep('preview');
      return;
    }
    setBusy(true);
    try {
      const r = await api.post('/imports/preview', body());
      setOutcome(r.data);
      setStep('preview');
    } catch (err) {
      setError(errMsg(err, 'Could not check the file'));
    } finally {
      setBusy(false);
    }
  };

  const runImport = async () => {
    setBusy(true);
    setError('');
    try {
      const r = await api.post('/imports', body());
      setOutcome(r.data);
      setStep('done');
      toast(r.data.created ? `${r.data.name}.` : 'Nothing new to import — everything in this file is already there.', r.data.created ? 'success' : 'info');
      loadHistory();
    } catch (err) {
      setError(errMsg(err, 'Could not import the file'));
    } finally {
      setBusy(false);
    }
  };

  const openJournal = () => {
    if (!opening) return;
    navigate('/ledger', {
      state: {
        prefill: {
          date: openingDate,
          description: 'Opening balances',
          reference: 'OPENING',
          lines: opening.filter((l) => !l.error).map((l) => ({
            accountId: l.accountId!, debit: l.debit ? (l.debit / 100).toFixed(2) : '', credit: l.credit ? (l.credit / 100).toFixed(2) : '', description: l.note,
          })),
        },
      },
    });
  };

  const undo = async () => {
    if (!undoing) return;
    setBusy(true);
    try {
      await api.delete(`/imports/${undoing.id}`);
      toast('Import undone. What it brought in has been removed.');
      setUndoing(null);
      loadHistory();
    } catch (err) {
      toast(errMsg(err, 'Could not undo the import'), 'error');
      setUndoing(null);
    } finally {
      setBusy(false);
    }
  };

  // ── Screens ────────────────────────────────────────────────────────────
  const stepper = (
    <ol className="flex items-center gap-2 text-xs mb-4" aria-label="Steps">
      {(['Choose', 'Match columns', 'Check', 'Done'] as const).map((label, i) => {
        const at = ['start', 'map', 'preview', 'done'].indexOf(step);
        return (
          <li key={label} className="flex items-center gap-2">
            <span className={`w-5 h-5 rounded-full grid place-items-center text-[11px] font-semibold ${i <= at ? 'bg-orange-600 text-white' : 'bg-gray-200 text-gray-600'}`}>{i + 1}</span>
            <span className={i === at ? 'font-semibold text-gray-900' : 'text-gray-500'}>{label}</span>
            {i < 3 && <span className="w-6 h-px bg-gray-300" />}
          </li>
        );
      })}
    </ol>
  );

  const counts = outcome && (
    <div className="flex flex-wrap gap-2 text-sm" data-testid="import-counts">
      <span className="px-2 py-1 rounded border bg-green-50 border-green-200 text-green-800">{outcome.created} new</span>
      <span className="px-2 py-1 rounded border bg-gray-50 border-gray-200 text-gray-700">{outcome.skipped} already there</span>
      {outcome.errors > 0 && <span className="px-2 py-1 rounded border bg-red-50 border-red-200 text-red-700">{outcome.errors} with errors (skipped)</span>}
    </div>
  );

  if (kinds.length === 0) {
    return <div><PageHeader title="Import from File" /><p className="text-sm text-gray-600">Your role cannot import anything.</p></div>;
  }

  return (
    <div>
      <PageHeader
        title="Import from File"
        subtitle="Bring in bank statements, sales, customers, vendors and opening balances from Excel or CSV."
        action={step !== 'start' ? <button className="btn btn-secondary" onClick={reset}>Start again</button> : undefined}
      />
      {stepper}
      {error && <div className="mb-3 px-3 py-2 rounded border border-red-200 bg-red-50 text-sm text-red-700" role="alert">{error}</div>}

      {step === 'start' && (
        <>
          <section className="card">
            <h3 className="text-sm font-semibold text-gray-800 mb-3">1. What are you importing?</h3>
            <div className="grid gap-2 mb-4" style={{ gridTemplateColumns: 'repeat(auto-fill, minmax(210px, 1fr))' }}>
              {kinds.map((k) => {
                const Icon = ICONS[k.kind];
                const on = k.kind === kind;
                return (
                  <button key={k.kind} type="button" onClick={() => { setKind(k.kind); setMapping({}); }} data-testid={`kind-${k.kind}`}
                    className={`text-left p-3 rounded border ${on ? 'border-orange-500 bg-orange-50 ring-1 ring-orange-400' : 'border-[#e5e7eb] hover:border-orange-300'}`}>
                    <div className="flex items-center gap-2 font-medium text-sm text-gray-900"><Icon size={16} className="text-orange-600" />{k.label}</div>
                    <p className="text-xs text-gray-600 mt-1">{k.description}</p>
                  </button>
                );
              })}
            </div>
            <h3 className="text-sm font-semibold text-gray-800 mb-2">2. Choose the file</h3>
            <div className="flex flex-wrap items-center gap-3">
              <label className={`btn btn-primary inline-flex items-center gap-2 cursor-pointer ${reading ? 'opacity-60 pointer-events-none' : ''}`}>
                <Upload size={15} /> {reading ? 'Reading…' : 'Choose CSV or Excel file…'}
                <input type="file" accept=".csv,.txt,.tsv,.xlsx,.xlsm,.xls,.xlsb,.ods" className="hidden" onChange={chooseFile} aria-label="Choose file" />
              </label>
              <button type="button" className="btn btn-secondary inline-flex items-center gap-2" onClick={() => download(`${info.label} sample.csv`, sampleCsv(info))}>
                <Download size={15} /> Download a sample file
              </button>
              <span className="text-xs text-gray-500">Most banks, shops and systems can export CSV or Excel. Up to 10 MB.</span>
            </div>
          </section>

          <section className="card">
            <h3 className="text-sm font-semibold text-gray-800 mb-2">Earlier imports</h3>
            {history.length === 0 ? <p className="text-sm text-gray-500">Nothing imported yet.</p> : (
              <table className="min-w-full text-sm" data-testid="import-history">
                <thead><tr className="text-left text-xs text-gray-500 border-b">
                  <th className="py-1.5 pr-3">When</th><th className="pr-3">What</th><th className="pr-3">From</th><th className="pr-3">File</th>
                  <th className="pr-3 text-right">New</th><th className="pr-3 text-right">Skipped</th><th className="pr-3">By</th><th className="pr-3">Status</th><th />
                </tr></thead>
                <tbody>
                  {history.map((h) => (
                    <tr key={h.id} className="border-b border-[#f3f3f3] last:border-0">
                      <td className="py-1.5 pr-3 whitespace-nowrap">{fmtDate(h.createdAt.slice(0, 10))}</td>
                      <td className="pr-3">{KINDS.find((k) => k.kind === h.kind)?.label ?? h.kind}{h.accountName ? ` · ${h.accountName}` : ''}</td>
                      <td className="pr-3">{h.source}</td>
                      <td className="pr-3 text-gray-600">{h.fileName ?? '—'}</td>
                      <td className="pr-3 text-right tabular-nums">{h.created}</td>
                      <td className="pr-3 text-right tabular-nums">{h.skipped}</td>
                      <td className="pr-3">{h.createdBy ?? '—'}</td>
                      <td className="pr-3">{h.status === 'UNDONE' ? <span className="text-gray-500">Undone</span> : <span className="text-green-700">Imported</span>}</td>
                      <td className="text-right">
                        {h.status === 'ACTIVE' && hasPermission(KINDS.find((k) => k.kind === h.kind)?.permission ?? '') && (
                          <button className="btn-link text-xs inline-flex items-center gap-1" onClick={() => setUndoing(h)}><Undo2 size={12} /> Undo</button>
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

      {step === 'map' && sheet && (
        <section className="card">
          <div className="flex flex-wrap items-center gap-3 mb-3 text-sm">
            <FileSpreadsheet size={16} className="text-green-700" />
            <b>{fileName}</b>
            {sheets.length > 1 && (
              <label className="text-xs text-gray-600 flex items-center gap-2">Sheet
                <select value={sheetIndex} onChange={(e) => { const i = Number(e.target.value); setSheetIndex(i); applyGuess(sheets[i].rows, hasHeader); }} aria-label="Sheet">
                  {sheets.map((s, i) => <option key={s.name + i} value={i}>{s.name} ({s.rows.length} rows)</option>)}
                </select>
              </label>
            )}
            <span className="text-gray-500">{dataRows.length} rows{sheet.truncated ? ' (only the first 10,000 are read)' : ''}</span>
            <label className="text-xs text-gray-700 flex items-center gap-1.5 ml-auto">
              <input type="checkbox" checked={hasHeader} onChange={(e) => { setHasHeader(e.target.checked); applyGuess(allRows, e.target.checked); }} />
              The first row has the column names
            </label>
          </div>

          <div className="grid gap-4" style={{ gridTemplateColumns: 'minmax(320px, 1fr) 1.4fr' }}>
            <div>
              <div className="grid gap-2 mb-3">
                <label className="text-xs font-medium text-gray-600">Where is this file from?
                  <input className="mt-1" value={source} onChange={(e) => setSource(e.target.value)} placeholder="e.g. Commercial Bank, Shopify, old system" aria-label="Source" />
                </label>
                {kind === 'BANK' && (
                  <label className="text-xs font-medium text-gray-600">Bank account it belongs to
                    <select className="mt-1" value={accountId} onChange={(e) => setAccountId(e.target.value)} aria-label="Bank account">
                      <option value="">Choose…</option>
                      {bankAccounts.map((a) => <option key={a.id} value={a.id}>{a.code} – {a.name}{a.currency ? ` (${a.currency})` : ''}</option>)}
                    </select>
                  </label>
                )}
                {kind === 'INVOICES' && (
                  <label className="text-xs font-medium text-gray-600">Revenue account for lines without one
                    <select className="mt-1" value={defaultAccountId} onChange={(e) => setDefaultAccountId(e.target.value)} aria-label="Default revenue account">
                      <option value="">Leave blank (choose on each invoice)</option>
                      {revenueAccounts.map((a) => <option key={a.id} value={a.id}>{a.code} – {a.name}</option>)}
                    </select>
                  </label>
                )}
                {kind === 'OPENING' && (
                  <label className="text-xs font-medium text-gray-600">Opening date
                    <input className="mt-1" type="date" value={openingDate} onChange={(e) => setOpeningDate(e.target.value)} aria-label="Opening date" />
                  </label>
                )}
                {mapping.date !== undefined && (
                  <label className="text-xs font-medium text-gray-600">Dates in this file are written
                    <select className="mt-1" value={dateOrder} onChange={(e) => setDateOrder(e.target.value as DateOrder)} aria-label="Date order">
                      {DATE_ORDERS.map(([v, l]) => <option key={v} value={v}>{l}</option>)}
                    </select>
                  </label>
                )}
                {layouts.length > 0 && (
                  <label className="text-xs font-medium text-gray-600">Saved layout
                    <select className="mt-1" value="" onChange={(e) => applyLayout(e.target.value)} aria-label="Saved layout">
                      <option value="">Use a saved layout…</option>
                      {layouts.map((l) => <option key={l.id} value={l.id}>{l.name}</option>)}
                    </select>
                  </label>
                )}
              </div>

              <h4 className="text-xs font-semibold text-gray-700 uppercase tracking-wide mb-1">Match the columns</h4>
              <table className="w-full text-sm">
                <tbody>
                  {info.fields.map((f) => (
                    <tr key={f.key} className="border-b border-[#f3f3f3] last:border-0">
                      <td className="py-1.5 pr-2 align-top w-[44%]">
                        <div className="font-medium text-gray-800">{f.label}{f.required && <span className="text-red-600"> *</span>}</div>
                        {f.hint && <div className="text-[11px] text-gray-500">{f.hint}</div>}
                      </td>
                      <td className="py-1.5">
                        <select value={mapping[f.key] ?? ''} aria-label={`Column for ${f.label}`}
                          onChange={(e) => setMapping((m) => { const n = { ...m }; if (e.target.value === '') delete n[f.key]; else n[f.key] = Number(e.target.value); return n; })}>
                          <option value="">— not in this file —</option>
                          {Array.from({ length: width }, (_, i) => <option key={i} value={i}>{columnName(i)}</option>)}
                        </select>
                        {mapping[f.key] !== undefined && dataRows[0] && (
                          <div className="text-[11px] text-gray-500 mt-0.5 truncate">e.g. {dataRows.slice(0, 3).map((r) => r[mapping[f.key]] || '·').join('  |  ')}</div>
                        )}
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
              {(missing.length > 0 || bankAmountMissing || openingAmountMissing) && (
                <p className="text-xs text-red-700 mt-2 flex items-center gap-1"><AlertTriangle size={12} />
                  Still to match: {[...missing.map((f) => f.label), ...(bankAmountMissing ? ['Amount, or Money in / Money out'] : []), ...(openingAmountMissing ? ['Debit or Credit'] : [])].join(', ')}
                </p>
              )}
              <div className="flex items-center gap-2 mt-3">
                <input value={layoutName} onChange={(e) => setLayoutName(e.target.value)} placeholder="Layout name, e.g. Commercial Bank" aria-label="Layout name" className="flex-1" />
                <button type="button" className="btn btn-secondary" onClick={saveLayout}>Save layout</button>
              </div>
            </div>

            <div className="min-w-0">
              <h4 className="text-xs font-semibold text-gray-700 uppercase tracking-wide mb-1">The file</h4>
              <div className="overflow-auto border rounded max-h-[420px]">
                <table className="text-xs min-w-full">
                  <thead className="bg-gray-50 sticky top-0">
                    <tr>{Array.from({ length: width }, (_, i) => {
                      const field = info.fields.find((f) => mapping[f.key] === i);
                      return <th key={i} className={`px-2 py-1 text-left whitespace-nowrap border-b ${field ? 'bg-orange-50 text-orange-800' : 'text-gray-600'}`}>
                        {columnName(i)}{field && <div className="font-normal">→ {field.label}</div>}
                      </th>;
                    })}</tr>
                  </thead>
                  <tbody>
                    {dataRows.slice(0, 12).map((r, j) => (
                      <tr key={j} className="border-b border-[#f3f3f3]">{Array.from({ length: width }, (_, i) => <td key={i} className="px-2 py-1 whitespace-nowrap">{r[i] ?? ''}</td>)}</tr>
                    ))}
                  </tbody>
                </table>
              </div>
              {dataRows.length > 12 && <p className="text-[11px] text-gray-500 mt-1">The first 12 of {dataRows.length} rows.</p>}
            </div>
          </div>

          <div className="flex gap-2 mt-4">
            <button className="btn btn-primary" onClick={preview} disabled={busy || missing.length > 0 || bankAmountMissing || openingAmountMissing || !source.trim()}>
              {busy ? 'Checking…' : 'Next: check the rows'}
            </button>
            <button className="btn btn-secondary" onClick={reset}>Cancel</button>
          </div>
        </section>
      )}

      {step === 'preview' && outcome && kind !== 'OPENING' && (
        <section className="card">
          <h3 className="text-sm font-semibold text-gray-800 mb-2">Check before importing — nothing has been saved yet</h3>
          {counts}
          <label className="text-xs text-gray-700 flex items-center gap-1.5 mt-3">
            <input type="checkbox" checked={onlyProblems} onChange={(e) => setOnlyProblems(e.target.checked)} /> Show only rows with errors or already there
          </label>
          <ResultTable rows={outcome.rows.filter((r) => !onlyProblems || r.status !== 'NEW')} />
          <div className="flex gap-2 mt-4">
            <button className="btn btn-primary" onClick={runImport} disabled={busy || outcome.created === 0}>
              {busy ? 'Importing…' : outcome.created === 0 ? 'Nothing new to import' : `Import ${outcome.created} ${outcome.created === 1 ? 'row' : 'rows'}`}
            </button>
            <button className="btn btn-secondary" onClick={() => setStep('map')}>Back</button>
          </div>
          {kind !== 'BANK' && <p className="text-xs text-gray-500 mt-2">{kind === 'INVOICES' ? 'Invoices arrive as drafts: check and post them on the Invoices screen.' : 'You can undo an import from the list of earlier imports.'}</p>}
        </section>
      )}

      {step === 'preview' && opening && (
        <section className="card">
          <h3 className="text-sm font-semibold text-gray-800 mb-2">Opening balances on {fmtDate(openingDate)}</h3>
          <table className="min-w-full text-sm" data-testid="opening-preview">
            <thead><tr className="text-left text-xs text-gray-500 border-b"><th className="py-1.5 pr-3">Row</th><th className="pr-3">Account</th><th className="pr-3">Note</th><th className="pr-3 text-right">Debit</th><th className="pr-3 text-right">Credit</th><th /></tr></thead>
            <tbody>
              {opening.map((l) => {
                const a = accounts.find((x) => x.id === l.accountId);
                return (
                  <tr key={l.row} className="border-b border-[#f3f3f3]">
                    <td className="py-1.5 pr-3 text-gray-500">{l.row}</td>
                    <td className="pr-3">{a ? `${a.code} – ${a.name}` : l.code}</td>
                    <td className="pr-3 text-gray-600">{l.note}</td>
                    <td className="pr-3 text-right tabular-nums">{l.debit ? money(l.debit / 100) : ''}</td>
                    <td className="pr-3 text-right tabular-nums">{l.credit ? money(l.credit / 100) : ''}</td>
                    <td className="text-xs text-red-700">{l.error}</td>
                  </tr>
                );
              })}
            </tbody>
            <tfoot>
              {(() => {
                const ok = opening.filter((l) => !l.error);
                const [d, c] = [ok.reduce((s, l) => s + l.debit, 0), ok.reduce((s, l) => s + l.credit, 0)];
                return (
                  <tr className="font-semibold">
                    <td colSpan={3} className="py-1.5">Total {d === c ? <span className="text-green-700 font-normal">✓ Balanced</span> : <span className="text-red-700 font-normal">— differs by {money(Math.abs(d - c) / 100)}; the journal entry will not post until it balances</span>}</td>
                    <td className="pr-3 text-right tabular-nums">{money(d / 100)}</td>
                    <td className="pr-3 text-right tabular-nums">{money(c / 100)}</td>
                    <td />
                  </tr>
                );
              })()}
            </tfoot>
          </table>
          <div className="flex gap-2 mt-4">
            <button className="btn btn-primary" onClick={openJournal} disabled={!opening.some((l) => !l.error)}>Open as a journal entry</button>
            <button className="btn btn-secondary" onClick={() => setStep('map')}>Back</button>
          </div>
          <p className="text-xs text-gray-500 mt-2">The entry opens on the Ledger screen for you to check and post. Rows with errors are left out.</p>
        </section>
      )}

      {step === 'done' && outcome && (
        <section className="card">
          <div className="flex items-center gap-2 text-green-700 font-semibold mb-2"><CheckCircle2 size={18} /> {outcome.created ? 'Imported' : 'Nothing new'}</div>
          {counts}
          <div className="flex flex-wrap gap-2 mt-4">
            {kind === 'BANK' && outcome.created > 0 && <button className="btn btn-primary" onClick={() => navigate(`/bank-transactions?accountId=${accountId}`)}>Match the transactions now</button>}
            {kind === 'INVOICES' && <button className="btn btn-primary" onClick={() => navigate('/invoices')}>Open Invoices</button>}
            {kind === 'CUSTOMERS' && <button className="btn btn-primary" onClick={() => navigate('/customers')}>Open Customers</button>}
            {kind === 'VENDORS' && <button className="btn btn-primary" onClick={() => navigate('/vendors')}>Open Vendors</button>}
            <button className="btn btn-secondary" onClick={() => { reset(); loadHistory(); }}>Import another file</button>
          </div>
          {outcome.errors > 0 && <ResultTable rows={outcome.rows.filter((r) => r.status === 'ERROR')} />}
        </section>
      )}

      {undoing && (
        <ConfirmDialog title="Undo this import" confirmLabel="Undo import" danger busy={busy} onConfirm={undo} onCancel={() => setUndoing(null)}>
          <p>Remove the {undoing.created} {undoing.created === 1 ? 'record' : 'records'} brought in from <b>{undoing.source}</b>{undoing.fileName ? ` (${undoing.fileName})` : ''}?</p>
          <p className="text-gray-600">This works only while nothing has used them: bank transactions not yet matched, invoices still drafts, customers and vendors on no document.</p>
        </ConfirmDialog>
      )}
    </div>
  );
}

function ResultTable({ rows }: { rows: RowResult[] }) {
  return (
    <div className="overflow-auto border rounded max-h-[420px] mt-3">
      <table className="min-w-full text-sm" data-testid="import-preview">
        <thead className="bg-gray-50 sticky top-0"><tr className="text-left text-xs text-gray-500">
          <th className="px-2 py-1.5">Row</th><th className="px-2">Result</th><th className="px-2">What</th><th className="px-2">Note</th>
        </tr></thead>
        <tbody>
          {rows.slice(0, 1000).map((r) => (
            <tr key={r.row} className="border-t border-[#f3f3f3]">
              <td className="px-2 py-1 text-gray-500 tabular-nums">{r.row}</td>
              <td className="px-2"><span className={`text-[11px] px-1.5 py-0.5 rounded border ${STATUS_STYLE[r.status]}`}>{STATUS_TEXT[r.status]}</span></td>
              <td className="px-2">{r.label}</td>
              <td className={`px-2 text-xs ${r.status === 'ERROR' ? 'text-red-700' : 'text-gray-600'}`}>{r.message}</td>
            </tr>
          ))}
          {rows.length === 0 && <tr><td colSpan={4} className="px-2 py-3 text-gray-500">No rows to show.</td></tr>}
        </tbody>
      </table>
    </div>
  );
}
