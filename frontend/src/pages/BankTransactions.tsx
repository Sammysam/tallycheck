import { useCallback, useEffect, useState } from 'react';
import { Link, useSearchParams } from 'react-router-dom';
import { CheckCircle2, Landmark, Sparkles, Upload } from 'lucide-react';
import api, { errMsg } from '../api/client';
import PageHeader from '../components/PageHeader';
import ActionMenu from '../components/ActionMenu';
import Dialog from '../components/desktop/Dialog';
import TxnLink from '../components/TxnLink';
import { useAuth } from '../contexts/AuthContext';
import { toast } from '../lib/toast';
import { fmtDate } from '../lib/region';
import { moneyIn } from '../lib/money';
import { lineCodes, useTaxCodes } from '../lib/taxes';

/**
 * D37: imported bank statement lines, one by one: match each to a payment or expense already
 * recorded, record the payment of an open invoice / bill or an expense from it, or ignore it.
 * The app suggests matches; nothing is matched without a click.
 */

type Status = 'UNMATCHED' | 'MATCHED' | 'IGNORED';
interface Suggestion { type: 'PAYMENT' | 'EXPENSE' | 'INVOICE' | 'BILL'; id: string; number: string; date: string; party?: string; amount: number; exact: boolean }
interface Match { type: 'PAYMENT' | 'EXPENSE'; id: string; number?: string | null; status?: string | null; journalEntryId?: string | null }
interface Line {
  id: string; date: string; description: string; reference?: string | null; amount: number; status: Status;
  suggestions?: Suggestion[]; match?: Match | null;
}
interface Account { id: string; code: string; name: string; type: string; currency?: string | null; isActive: boolean }
interface Doc { id: string; number: string; party: string; date: string; owed: number; currency?: string | null }

const TABS: [Status, string][] = [['UNMATCHED', 'To review'], ['MATCHED', 'Matched'], ['IGNORED', 'Ignored']];

export default function BankTransactions() {
  const { hasPermission } = useAuth();
  const [params, setParams] = useSearchParams();
  const accountId = params.get('accountId') ?? '';
  const [accounts, setAccounts] = useState<Account[]>([]);
  const [withLines, setWithLines] = useState<Record<string, { total: number; unmatched: number }>>({});
  const [status, setStatus] = useState<Status>('UNMATCHED');
  const [lines, setLines] = useState<Line[]>([]);
  const [counts, setCounts] = useState<Record<Status, number>>({ UNMATCHED: 0, MATCHED: 0, IGNORED: 0 });
  const [currency, setCurrency] = useState<string | null>(null);
  /** Which account + tab the lines on screen belong to; anything else is still loading. */
  const [loadedFor, setLoadedFor] = useState('');
  const [paying, setPaying] = useState<Line | null>(null);
  const [expensing, setExpensing] = useState<Line | null>(null);
  const [busyId, setBusyId] = useState('');

  useEffect(() => {
    api.get('/accounts').then((r) => setAccounts(r.data));
    api.get('/bank-lines/accounts').then((r) => {
      const m: Record<string, { total: number; unmatched: number }> = {};
      for (const a of r.data) m[a.accountId] = { total: a.total, unmatched: a.unmatched };
      setWithLines(m);
      if (!params.get('accountId') && r.data.length) {
        const first = [...r.data].sort((a: { unmatched: number }, b: { unmatched: number }) => b.unmatched - a.unmatched)[0];
        setParams({ accountId: first.accountId }, { replace: true });
      }
    });
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  const load = useCallback(() => {
    if (!accountId) return;
    api.get('/bank-lines', { params: { accountId, status } })
      .then((r) => { setLines(r.data.data); setCounts(r.data.counts); setCurrency(r.data.currency); setLoadedFor(`${accountId}|${status}`); })
      .catch((e) => toast(errMsg(e, 'Could not load the bank transactions'), 'error'));
  }, [accountId, status]);
  useEffect(() => { load(); }, [load]);

  const act = async (line: Line, path: string, body: object | undefined, done: string) => {
    setBusyId(line.id);
    try {
      await api.post(`/bank-lines/${line.id}/${path}`, body);
      toast(done);
      load();
      api.get('/bank-lines/accounts').then((r) => {
        const m: Record<string, { total: number; unmatched: number }> = {};
        for (const a of r.data) m[a.accountId] = { total: a.total, unmatched: a.unmatched };
        setWithLines(m);
      });
      return true;
    } catch (e) {
      toast(errMsg(e, 'Could not do that'), 'error');
      return false;
    } finally {
      setBusyId('');
    }
  };

  const takeSuggestion = (line: Line, s: Suggestion) => {
    if (s.type === 'PAYMENT' || s.type === 'EXPENSE') return act(line, 'match', { type: s.type, id: s.id }, `Matched to ${s.number}.`);
    if (!hasPermission('payments')) { toast('Recording a payment needs the Payments permission.', 'error'); return; }
    return act(line, 'payment', s.type === 'INVOICE' ? { invoiceId: s.id } : { billId: s.id }, `Payment recorded for ${s.number}.`);
  };

  const loading = loadedFor !== `${accountId}|${status}`;
  const fmt = (v: number) => moneyIn(v, currency);
  const banks = accounts.filter((a) => a.isActive && (a.type === 'ASSET' || a.type === 'LIABILITY'));
  const shownAccounts = [...banks].sort((a, b) => Number(!!withLines[b.id]) - Number(!!withLines[a.id]) || a.code.localeCompare(b.code));

  const suggestionText = (line: Line, s: Suggestion) => {
    switch (s.type) {
      case 'PAYMENT': return `Already recorded: ${s.number} (${fmtDate(s.date)})`;
      case 'EXPENSE': return `Already recorded: ${s.number} ${s.party ?? ''}`;
      case 'INVOICE': return `${s.exact ? 'Pays' : 'Part-pays'} ${s.number} · ${s.party ?? ''} (owed ${fmt(s.amount)})`;
      case 'BILL': return `${s.exact ? 'Pays' : 'Part-pays'} ${s.number} · ${s.party ?? ''} (owed ${fmt(s.amount)})`;
    }
    return line.id;
  };

  return (
    <div>
      <PageHeader
        title="Bank Transactions"
        subtitle="Lines imported from bank statements. Match each one to what it was for; the app suggests matches."
        action={<Link to="/import" className="btn btn-secondary inline-flex items-center gap-1.5"><Upload size={14} /> Import a statement</Link>}
      />

      <div className="card !py-3 mb-3 flex flex-wrap items-center gap-3">
        <label className="text-xs font-medium text-gray-600 flex items-center gap-2">Account
          <select value={accountId} onChange={(e) => setParams({ accountId: e.target.value })} aria-label="Account" className="min-w-[260px]">
            <option value="">Choose an account…</option>
            {shownAccounts.map((a) => (
              <option key={a.id} value={a.id}>
                {a.code} – {a.name}{a.currency ? ` (${a.currency})` : ''}{withLines[a.id] ? ` · ${withLines[a.id].unmatched} to review` : ''}
              </option>
            ))}
          </select>
        </label>
        {accountId && (
          <div className="flex gap-1 ml-auto" role="tablist">
            {TABS.map(([s, label]) => (
              <button key={s} role="tab" aria-selected={status === s} onClick={() => setStatus(s)}
                className={`px-3 py-1.5 text-sm rounded border ${status === s ? 'bg-orange-50 border-orange-400 text-orange-800 font-medium' : 'border-[#e5e7eb] text-gray-700 hover:border-orange-300'}`}>
                {label} <span className="text-xs text-gray-500">({counts[s]})</span>
              </button>
            ))}
          </div>
        )}
      </div>

      {!accountId && (
        <div className="card text-center py-10 text-sm text-gray-600">
          <Landmark size={28} className="mx-auto mb-2 text-gray-400" />
          Choose an account, or <Link to="/import" className="btn-link">import a bank statement</Link> first.
        </div>
      )}

      {accountId && !loading && lines.length === 0 && (
        <div className="card text-center py-10 text-sm text-gray-600">
          <CheckCircle2 size={28} className="mx-auto mb-2 text-green-600" />
          {status === 'UNMATCHED'
            ? <>Nothing to review on this account. {counts.MATCHED > 0 && <Link to="/reconcile" className="btn-link">Reconcile it against the statement →</Link>}</>
            : 'Nothing here.'}
        </div>
      )}

      {accountId && !loading && lines.length > 0 && (
        <div className="rounded-md border border-[#e0e0e0] bg-white overflow-x-auto">
          <table className="min-w-full text-sm" data-testid="bank-lines">
            <thead className="bg-[#fafafa] text-xs text-gray-600">
              <tr className="text-left">
                <th className="px-3 py-2">Date</th><th className="px-3">Description</th>
                <th className="px-3 text-right">Money in</th><th className="px-3 text-right">Money out</th>
                <th className="px-3">{status === 'UNMATCHED' ? 'Suggested match' : status === 'MATCHED' ? 'Matched to' : ''}</th><th className="px-3" />
              </tr>
            </thead>
            <tbody>
              {lines.map((l) => (
                <tr key={l.id} className={`border-t border-[#f0f0f0] align-top ${busyId === l.id ? 'opacity-50' : ''}`}>
                  <td className="px-3 py-2 whitespace-nowrap">{fmtDate(l.date)}</td>
                  <td className="px-3 py-2">
                    <div className="text-gray-900">{l.description}</div>
                    {l.reference && <div className="text-xs text-gray-500">{l.reference}</div>}
                  </td>
                  <td className="px-3 py-2 text-right tabular-nums text-green-700">{l.amount > 0 ? fmt(l.amount) : ''}</td>
                  <td className="px-3 py-2 text-right tabular-nums">{l.amount < 0 ? fmt(-l.amount) : ''}</td>
                  <td className="px-3 py-2">
                    {l.status === 'UNMATCHED' && (l.suggestions?.length ? (
                      <div className="flex flex-col gap-1 items-start">
                        {l.suggestions.map((s) => (
                          <button key={s.type + s.id} type="button" disabled={!!busyId} onClick={() => takeSuggestion(l, s)} data-testid="suggestion"
                            className={`text-left text-xs px-2 py-1 rounded border inline-flex items-center gap-1.5 ${s.exact ? 'border-green-300 bg-green-50 text-green-800 hover:bg-green-100' : 'border-[#e5e7eb] text-gray-700 hover:border-orange-300'}`}>
                            <Sparkles size={12} /> {suggestionText(l, s)}
                          </button>
                        ))}
                      </div>
                    ) : <span className="text-xs text-gray-400">No suggestion — use the ⋮ menu</span>)}
                    {l.status === 'MATCHED' && l.match && (
                      <span className="text-sm">
                        <TxnLink entryId={l.match.journalEntryId}>{l.match.number ?? l.match.type}</TxnLink>
                        <span className="text-xs text-gray-500"> · {l.match.type === 'PAYMENT' ? 'payment' : 'expense'}{l.match.status === 'VOID' ? ' (voided)' : ''}</span>
                      </span>
                    )}
                  </td>
                  <td className="px-3 py-2 text-right">
                    {l.status === 'UNMATCHED' && (
                      <ActionMenu items={[
                        ...(l.amount > 0 && hasPermission('payments') ? [{ label: 'Payment for an invoice…', onSelect: () => setPaying(l) }] : []),
                        ...(l.amount < 0 && hasPermission('payments') ? [{ label: 'Payment for a bill…', onSelect: () => setPaying(l) }] : []),
                        ...(l.amount < 0 && !currency && hasPermission('expenses') ? [{ label: 'Record as an expense…', onSelect: () => setExpensing(l) }] : []),
                        { label: 'Ignore', onSelect: () => act(l, 'ignore', undefined, 'Ignored. Find it under Ignored if you need it back.') },
                      ]} />
                    )}
                    {l.status === 'MATCHED' && <button className="btn-link text-xs" disabled={!!busyId} onClick={() => act(l, 'unmatch', undefined, 'Unmatched. The payment or expense itself is kept.')}>Unmatch</button>}
                    {l.status === 'IGNORED' && <button className="btn-link text-xs" disabled={!!busyId} onClick={() => act(l, 'restore', undefined, 'Moved back to review.')}>Restore</button>}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
      {status === 'MATCHED' && lines.length > 0 && (
        <p className="text-xs text-gray-500 mt-2">Unmatching only frees the bank line. To cancel a payment or expense, void it on its own screen.</p>
      )}

      {paying && <PayDialog line={paying} currency={currency} fmt={fmt} onClose={() => setPaying(null)}
        onPay={async (doc, method) => {
          const ok = await act(paying, 'payment', { ...(paying.amount > 0 ? { invoiceId: doc } : { billId: doc }), paymentMethod: method }, 'Payment recorded.');
          if (ok) setPaying(null);
        }} />}
      {expensing && <ExpenseDialog line={expensing} accounts={accounts} fmt={fmt} onClose={() => setExpensing(null)}
        onSave={async (body) => {
          const ok = await act(expensing, 'expense', body, 'Expense recorded and posted.');
          if (ok) setExpensing(null);
        }} />}
    </div>
  );
}

function PayDialog({ line, currency, fmt, onClose, onPay }: {
  line: Line; currency: string | null; fmt: (v: number) => string; onClose: () => void; onPay: (docId: string, method: string) => void;
}) {
  const isIn = line.amount > 0;
  const size = Math.abs(line.amount);
  const [docs, setDocs] = useState<Doc[] | null>(null);
  const [docId, setDocId] = useState('');
  const [method, setMethod] = useState('BANK_TRANSFER');

  useEffect(() => {
    api.get(`/bank-lines/${line.id}/documents`).then((r) => {
      const list: Doc[] = (r.data as { id: string; number: string; party?: string; date: string; amount: number }[])
        .map((d) => ({ id: d.id, number: d.number, party: d.party ?? '', date: d.date, owed: d.amount, currency }));
      setDocs(list);
      const exact = list.find((d) => Math.round(d.owed * 100) === Math.round(size * 100));
      if (exact) setDocId(exact.id);
    }).catch(() => setDocs([]));
  }, [line.id, currency, size]);

  const chosen = docs?.find((d) => d.id === docId);
  const tooMuch = chosen && Math.round(size * 100) > Math.round(chosen.owed * 100);
  return (
    <Dialog title={isIn ? 'Payment for an invoice' : 'Payment for a bill'} width={520} onClose={onClose}
      buttons={<>
        <button className="btn btn-primary" disabled={!docId || !!tooMuch} onClick={() => onPay(docId, method)}>Record payment</button>
        <button className="btn btn-secondary" onClick={onClose}>Cancel</button>
      </>}>
      <p className="text-sm text-gray-700 mb-3">{fmtDate(line.date)} · {line.description} · <b>{fmt(size)}</b></p>
      <label className="text-xs font-medium text-gray-600 block mb-2">{isIn ? 'Invoice' : 'Bill'}
        <select className="mt-1 w-full" value={docId} onChange={(e) => setDocId(e.target.value)} aria-label={isIn ? 'Invoice' : 'Bill'}>
          <option value="">{docs === null ? 'Loading…' : docs.length ? 'Choose…' : `No open ${isIn ? 'invoices' : 'bills'} in this currency`}</option>
          {docs?.map((d) => <option key={d.id} value={d.id}>{d.number} · {d.party} · {fmtDate(d.date)} · owed {fmt(d.owed)}</option>)}
        </select>
      </label>
      <label className="text-xs font-medium text-gray-600 block">Method
        <select className="mt-1 w-full" value={method} onChange={(e) => setMethod(e.target.value)} aria-label="Payment method">
          <option value="BANK_TRANSFER">Bank transfer</option><option value="CHECK">Cheque</option><option value="CASH">Cash</option>
        </select>
      </label>
      {tooMuch && <p className="text-xs text-red-700 mt-2">The bank amount is more than is owed on {chosen?.number}. Record it on the Payments screen instead.</p>}
      <p className="text-xs text-gray-500 mt-3">The payment is recorded on {fmtDate(line.date)} for {fmt(size)} through this account, and posted.</p>
    </Dialog>
  );
}

function ExpenseDialog({ line, accounts, fmt, onClose, onSave }: {
  line: Line; accounts: Account[]; fmt: (v: number) => string; onClose: () => void;
  onSave: (body: { expenseAccountId: string; taxCodeId?: string; description: string }) => void;
}) {
  const codes = lineCodes(useTaxCodes());
  const expenseAccounts = accounts.filter((a) => a.isActive && a.type === 'EXPENSE');
  const [accountId, setAccountId] = useState('');
  const [taxCodeId, setTaxCodeId] = useState('');
  const [description, setDescription] = useState(line.description);
  return (
    <Dialog title="Record as an expense" width={480} onClose={onClose}
      buttons={<>
        <button className="btn btn-primary" disabled={!accountId} onClick={() => onSave({ expenseAccountId: accountId, taxCodeId: taxCodeId || undefined, description })}>Record and post</button>
        <button className="btn btn-secondary" onClick={onClose}>Cancel</button>
      </>}>
      <p className="text-sm text-gray-700 mb-3">{fmtDate(line.date)} · <b>{fmt(-line.amount)}</b> paid out</p>
      <label className="text-xs font-medium text-gray-600 block mb-2">Description
        <input className="mt-1 w-full" value={description} onChange={(e) => setDescription(e.target.value)} aria-label="Description" />
      </label>
      <label className="text-xs font-medium text-gray-600 block mb-2">Expense account
        <select className="mt-1 w-full" value={accountId} onChange={(e) => setAccountId(e.target.value)} aria-label="Expense account">
          <option value="">Choose…</option>
          {expenseAccounts.map((a) => <option key={a.id} value={a.id}>{a.code} – {a.name}</option>)}
        </select>
      </label>
      <label className="text-xs font-medium text-gray-600 block">Tax (included in the amount)
        <select className="mt-1 w-full" value={taxCodeId} onChange={(e) => setTaxCodeId(e.target.value)} aria-label="Tax code">
          <option value="">No tax</option>
          {codes.map((c) => <option key={c.id} value={c.id}>{c.name}</option>)}
        </select>
      </label>
    </Dialog>
  );
}
