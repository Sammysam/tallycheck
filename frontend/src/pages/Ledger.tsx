import { Fragment, useEffect, useState, type FormEvent } from 'react';
import api, { errMsg } from '../api/client';
import PageHeader from '../components/PageHeader';
import StatusBadge from '../components/StatusBadge';
import Dialog from '../components/desktop/Dialog';
import { money } from '../lib/money';
import { useCommand, setUnsaved } from '../lib/commands';
import { fmtDate, todayIso } from '../lib/region';

interface Line { id: string; debit: number | string; credit: number | string; description?: string | null; account?: { code: string; name: string } }
interface Entry {
  id: string; entryNumber: string; date: string; description: string; reference?: string | null;
  status: string; sourceType?: string | null; reversalOfId?: string | null; lines: Line[];
}
interface Account { id: string; code: string; name: string }
interface FormLine { _key: number; accountId: string; debit: string; credit: string; description: string }

let nextKey = 1;
const blankLine = (): FormLine => ({ _key: nextKey++, accountId: '', debit: '', credit: '', description: '' });
/** Amounts in cents, so the balance check never meets floating-point noise. */
const cents = (v: string) => Math.round((Number(v) || 0) * 100);

export default function Ledger() {
  const [rows, setRows] = useState<Entry[]>([]);
  const [expanded, setExpanded] = useState<string | null>(null);
  const [accounts, setAccounts] = useState<Account[]>([]);
  const [showForm, setShowForm] = useState(false);
  const [date, setDate] = useState(todayIso());
  const [description, setDescription] = useState('');
  const [reference, setReference] = useState('');
  const [lines, setLines] = useState<FormLine[]>([blankLine(), blankLine()]);
  const [error, setError] = useState('');
  const [saving, setSaving] = useState(false);
  const [reversing, setReversing] = useState<Entry | null>(null);
  const [busy, setBusy] = useState(false);

  const load = () => api.get('/ledger/entries').then((r) => setRows(r.data));
  useEffect(() => {
    load();
    api.get('/accounts').then((r) => setAccounts(r.data));
  }, []);

  const reset = () => {
    setDate(todayIso());
    setDescription('');
    setReference('');
    setLines([blankLine(), blankLine()]);
    setError('');
    setUnsaved(false);
  };
  const open = () => { reset(); setShowForm(true); };
  const close = () => { reset(); setShowForm(false); };
  useCommand('new', open);
  useCommand('close', close);

  const setLine = (i: number, patch: Partial<FormLine>) => {
    setUnsaved(true);
    setLines((ls) => ls.map((l, j) => {
      if (j !== i) return l;
      const next = { ...l, ...patch };
      // A line is a debit or a credit, never both.
      if (patch.debit) next.credit = '';
      if (patch.credit) next.debit = '';
      return next;
    }));
  };

  const debits = lines.reduce((s, l) => s + cents(l.debit), 0);
  const credits = lines.reduce((s, l) => s + cents(l.credit), 0);
  const difference = debits - credits;
  const used = lines.filter((l) => l.accountId && (cents(l.debit) || cents(l.credit)));
  const balanced = difference === 0 && debits > 0 && used.length >= 2;

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    setError('');
    if (!balanced) {
      setError('Debits and credits must be equal, with at least two lines, before the entry can be posted.');
      return;
    }
    setSaving(true);
    try {
      await api.post('/ledger/entries', {
        date,
        description: description.trim(),
        reference: reference.trim() || undefined,
        lines: used.map((l) => ({
          accountId: l.accountId,
          debit: cents(l.debit) / 100,
          credit: cents(l.credit) / 100,
          description: l.description.trim() || undefined,
        })),
      });
      close();
      await load();
    } catch (err) {
      setError(errMsg(err, 'Could not post the journal entry'));
    } finally {
      setSaving(false);
    }
  };

  const confirmReverse = async () => {
    if (!reversing) return;
    setBusy(true);
    try {
      await api.post(`/ledger/entries/${reversing.id}/reverse`);
      setReversing(null);
      await load();
    } catch (err) {
      setError(errMsg(err, 'Could not reverse the entry'));
      setReversing(null);
    } finally {
      setBusy(false);
    }
  };

  const fmt = (v: number) => money(Number(v));
  const reversedIds = new Set(rows.map((r) => r.reversalOfId).filter(Boolean));

  return (
    <div>
      <PageHeader title="Journal Entries" action={!showForm && <button onClick={open} className="btn btn-primary">+ New Journal Entry</button>} />

      {showForm && (
        <form onSubmit={submit} aria-label="Journal entry" className="bg-white rounded-lg border border-gray-200 p-4 mb-4 space-y-3">
          <h3 className="text-sm font-semibold text-gray-700">New Journal Entry</h3>
          <p className="text-xs text-gray-500">For opening balances, bank charges, interest, depreciation and corrections. Invoices, bills, payments and payroll post their own entries.</p>
          <div className="form-grid" style={{ gridTemplateColumns: '160px 2fr 1fr' }}>
            <input type="date" required value={date} onChange={(e) => { setUnsaved(true); setDate(e.target.value); }} aria-label="Entry date" />
            <input required placeholder="Description (e.g. Opening balances)" value={description} onChange={(e) => { setUnsaved(true); setDescription(e.target.value); }} />
            <input placeholder="Reference (optional)" value={reference} onChange={(e) => { setUnsaved(true); setReference(e.target.value); }} />
          </div>
          <table className="min-w-full text-sm">
            <thead>
              <tr className="text-xs text-gray-500">
                <th className="text-left pb-1">Account</th>
                <th className="text-left pb-1">Line note</th>
                <th className="text-right pb-1" style={{ width: 140 }}>Debit</th>
                <th className="text-right pb-1" style={{ width: 140 }}>Credit</th>
                <th style={{ width: 30 }} />
              </tr>
            </thead>
            <tbody>
              {lines.map((l, i) => (
                <tr key={l._key}>
                  <td className="pr-2 py-1">
                    <select value={l.accountId} onChange={(e) => setLine(i, { accountId: e.target.value })} aria-label={`Account for line ${i + 1}`}>
                      <option value="">Choose an account…</option>
                      {accounts.map((a) => <option key={a.id} value={a.id}>{a.code} – {a.name}</option>)}
                    </select>
                  </td>
                  <td className="pr-2 py-1">
                    <input placeholder="optional" value={l.description} onChange={(e) => setLine(i, { description: e.target.value })} aria-label={`Note for line ${i + 1}`} />
                  </td>
                  <td className="pr-2 py-1">
                    <input type="number" min="0" step="0.01" className="text-right" value={l.debit} onChange={(e) => setLine(i, { debit: e.target.value })} aria-label={`Debit for line ${i + 1}`} />
                  </td>
                  <td className="pr-2 py-1">
                    <input type="number" min="0" step="0.01" className="text-right" value={l.credit} onChange={(e) => setLine(i, { credit: e.target.value })} aria-label={`Credit for line ${i + 1}`} />
                  </td>
                  <td className="py-1 text-center">
                    {lines.length > 2 && (
                      <button type="button" className="text-red-600" title="Remove line" onClick={() => setLines((ls) => ls.filter((_, j) => j !== i))}>×</button>
                    )}
                  </td>
                </tr>
              ))}
            </tbody>
            <tfoot>
              <tr className="font-semibold">
                <td className="pt-2">
                  <button type="button" className="btn-link text-xs" onClick={() => setLines((ls) => [...ls, blankLine()])}>+ Add line</button>
                </td>
                <td className="pt-2 text-right text-gray-600">Totals</td>
                <td className="pt-2 text-right tabular-nums">{fmt(debits / 100)}</td>
                <td className="pt-2 text-right tabular-nums">{fmt(credits / 100)}</td>
                <td />
              </tr>
            </tfoot>
          </table>
          <p className={`text-sm ${difference === 0 ? 'text-green-700' : 'text-orange-700'}`} data-testid="je-difference">
            {difference === 0 ? (debits > 0 ? '✓ Balanced' : 'Enter the amounts') : `Out of balance by ${fmt(Math.abs(difference) / 100)}`}
          </p>
          {error && <p className="text-sm text-red-600" role="alert">{error}</p>}
          <div className="flex gap-2">
            <button type="submit" className="btn btn-primary" disabled={saving || !balanced}>{saving ? 'Posting…' : 'Post Entry'}</button>
            <button type="button" className="btn btn-secondary" onClick={close}>Cancel</button>
          </div>
        </form>
      )}
      {!showForm && error && <p className="text-sm text-red-600 mb-3" role="alert">{error}</p>}

      <div className="bg-white rounded-lg border border-gray-200 overflow-hidden">
        <table className="min-w-full text-sm">
          <thead className="bg-gray-50 border-b border-gray-200">
            <tr>
              <th className="px-4 py-3 text-left font-semibold text-gray-600">#</th>
              <th className="px-4 py-3 text-left font-semibold text-gray-600">Date</th>
              <th className="px-4 py-3 text-left font-semibold text-gray-600">Description</th>
              <th className="px-4 py-3 text-left font-semibold text-gray-600">Reference</th>
              <th className="px-4 py-3 text-left font-semibold text-gray-600">Status</th>
            </tr>
          </thead>
          <tbody className="divide-y divide-gray-100">
            {rows.map((entry) => (
              <Fragment key={entry.id}>
                <tr className="hover:bg-gray-50 cursor-pointer" onClick={() => setExpanded(expanded === entry.id ? null : entry.id)}>
                  <td className="px-4 py-3 font-mono text-xs text-blue-600">{entry.entryNumber}</td>
                  <td className="px-4 py-3 text-gray-700">{fmtDate(entry.date)}</td>
                  <td className="px-4 py-3 text-gray-700">{entry.description}</td>
                  <td className="px-4 py-3 text-gray-500">{entry.reference ?? ''}</td>
                  <td className="px-4 py-3"><StatusBadge status={entry.status} /></td>
                </tr>
                {expanded === entry.id && (
                  <tr>
                    <td colSpan={5} className="px-8 py-3 bg-gray-50">
                      <table className="min-w-full text-xs">
                        <thead>
                          <tr className="text-gray-500">
                            <th className="text-left pb-1">Account</th>
                            <th className="text-left pb-1">Note</th>
                            <th className="text-right pb-1">Debit</th>
                            <th className="text-right pb-1">Credit</th>
                          </tr>
                        </thead>
                        <tbody>
                          {entry.lines?.map((line) => (
                            <tr key={line.id}>
                              <td className="py-0.5">{line.account?.code} {line.account?.name}</td>
                              <td className="py-0.5 text-gray-500">{line.description ?? ''}</td>
                              <td className="text-right py-0.5">{Number(line.debit) > 0 ? fmt(Number(line.debit)) : ''}</td>
                              <td className="text-right py-0.5">{Number(line.credit) > 0 ? fmt(Number(line.credit)) : ''}</td>
                            </tr>
                          ))}
                        </tbody>
                      </table>
                      {/* Entries a document posted are undone from that document (D18). */}
                      {!entry.sourceType && !entry.reversalOfId && !reversedIds.has(entry.id) && entry.status === 'POSTED' && (
                        <button type="button" className="btn btn-secondary mt-2 text-xs" onClick={(e) => { e.stopPropagation(); setReversing(entry); }}>
                          Reverse this entry
                        </button>
                      )}
                    </td>
                  </tr>
                )}
              </Fragment>
            ))}
          </tbody>
        </table>
        {rows.length === 0 && <p className="text-gray-400 text-sm py-8 text-center">No journal entries yet.</p>}
      </div>

      {reversing && (
        <Dialog
          title="Reverse journal entry"
          onClose={() => !busy && setReversing(null)}
          buttons={
            <>
              <button type="button" className="btn btn-primary" disabled={busy} onClick={confirmReverse}>{busy ? 'Reversing…' : 'Reverse entry'}</button>
              <button type="button" className="btn btn-secondary" disabled={busy} onClick={() => setReversing(null)}>Cancel</button>
            </>
          }
        >
          <p className="text-sm text-gray-800">
            Reverse <strong>{reversing.entryNumber}</strong> ({reversing.description})?
          </p>
          <p className="mt-2 text-sm text-gray-600">A new entry with the opposite amounts is posted. The original stays in the ledger.</p>
        </Dialog>
      )}
    </div>
  );
}
