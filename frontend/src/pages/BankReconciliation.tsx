import { useEffect, useMemo, useState } from 'react';
import { CheckCircle2, AlertTriangle } from 'lucide-react';
import api, { errMsg } from '../api/client';
import PageHeader from '../components/PageHeader';
import StatusBadge from '../components/StatusBadge';
import Dialog from '../components/desktop/Dialog';
import { useCommand } from '../lib/commands';

/** D22: match a cash or bank account's ledger lines to a bank statement. */

interface Account { id: string; code: string; name: string; type: string; isActive: boolean }
interface Rec {
  id: string;
  statementDate: string;
  statementBalance: number;
  openingBalance: number;
  status: 'DRAFT' | 'COMPLETED';
  completedAt: string | null;
  completedBy: string | null;
}
interface Line {
  lineId: string;
  entryNumber: string;
  date: string;
  description: string;
  reference: string | null;
  debit: number;
  credit: number;
  cleared: boolean;
}
interface Workspace extends Rec { lines: Line[]; clearedBalance: number; difference: number }

const fmt = (v: number) => new Intl.NumberFormat('en-LK', { style: 'currency', currency: 'LKR' }).format(v);
const cents = (v: number) => Math.round(v * 100);
const todayIso = () => {
  const d = new Date();
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`;
};

function LineTable({ title, lines, ticked, amount, onToggle, onAll, readOnly }: {
  title: string;
  lines: Line[];
  ticked: Set<string>;
  amount: (l: Line) => number;
  onToggle: (id: string) => void;
  onAll: (ids: string[], on: boolean) => void;
  readOnly: boolean;
}) {
  const all = lines.length > 0 && lines.every((l) => ticked.has(l.lineId));
  const total = lines.filter((l) => ticked.has(l.lineId)).reduce((s, l) => s + cents(amount(l)), 0) / 100;
  return (
    <section className="bg-white border border-[#e0e0e0] rounded-md overflow-hidden">
      <header className="flex items-center justify-between px-3 py-2 bg-[#fafafa] border-b border-[#ececec]">
        <h3 className="text-sm font-semibold">{title}</h3>
        <span className="text-xs text-gray-600">Ticked: <strong className="tabular-nums">{fmt(total)}</strong></span>
      </header>
      <div className="max-h-[420px] overflow-auto">
        <table className="min-w-full text-sm">
          <thead>
            <tr className="bg-[#f5f5f5] border-b border-[#d9d9d9] text-xs text-gray-700">
              <th className="sticky top-0 bg-[#f5f5f5] w-8 px-3 py-1.5 text-left">
                {!readOnly && (
                  <input type="checkbox" aria-label={`Tick all ${title.toLowerCase()}`} checked={all}
                    onChange={(e) => onAll(lines.map((l) => l.lineId), e.target.checked)} />
                )}
              </th>
              <th className="sticky top-0 bg-[#f5f5f5] px-3 py-1.5 text-left font-semibold">Date</th>
              <th className="sticky top-0 bg-[#f5f5f5] px-3 py-1.5 text-left font-semibold">Entry</th>
              <th className="sticky top-0 bg-[#f5f5f5] px-3 py-1.5 text-left font-semibold">Description</th>
              <th className="sticky top-0 bg-[#f5f5f5] px-3 py-1.5 text-right font-semibold">Amount</th>
            </tr>
          </thead>
          <tbody>
            {lines.map((l) => (
              <tr key={l.lineId}
                className={`border-b border-[#efefef] last:border-b-0 ${ticked.has(l.lineId) ? 'bg-[#fff4ec]' : ''} ${readOnly ? '' : 'hover:bg-[#f7f7f7]'}`}
                onClick={() => !readOnly && onToggle(l.lineId)}>
                <td className="px-3 py-1.5">
                  <input type="checkbox" aria-label={`Clear ${l.entryNumber}`} checked={ticked.has(l.lineId)} disabled={readOnly}
                    onClick={(e) => e.stopPropagation()} onChange={() => onToggle(l.lineId)} />
                </td>
                <td className="px-3 py-1.5 whitespace-nowrap">{l.date}</td>
                <td className="px-3 py-1.5 whitespace-nowrap">{l.entryNumber}</td>
                <td className="px-3 py-1.5">{l.description}{l.reference ? <span className="text-gray-500"> · {l.reference}</span> : null}</td>
                <td className="px-3 py-1.5 text-right tabular-nums">{fmt(amount(l))}</td>
              </tr>
            ))}
            {lines.length === 0 && (
              <tr><td colSpan={5} className="px-3 py-6 text-center text-gray-500">Nothing here.</td></tr>
            )}
          </tbody>
        </table>
      </div>
    </section>
  );
}

export default function BankReconciliation() {
  const [accounts, setAccounts] = useState<Account[]>([]);
  const [accountId, setAccountId] = useState('');
  const [history, setHistory] = useState<Rec[]>([]);
  const [ws, setWs] = useState<Workspace | null>(null);
  const [ticked, setTicked] = useState<Set<string>>(new Set());
  const [start, setStart] = useState({ statementDate: todayIso(), statementBalance: '' });
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(false);
  const [confirm, setConfirm] = useState<null | { kind: 'discard' } | { kind: 'undo'; rec: Rec }>(null);

  useCommand('close', () => setWs(null));

  useEffect(() => {
    api.get('/accounts').then((r) => {
      const banks = r.data.filter((a: Account) => a.type === 'ASSET' && a.isActive && a.code !== '1100');
      setAccounts(banks);
      const cash = banks.find((a: Account) => a.code === '1000') ?? banks[0];
      if (cash) setAccountId(cash.id);
    }).catch((e) => setError(errMsg(e, 'Could not load the accounts')));
  }, []);

  const open = (w: Workspace) => {
    setWs(w);
    setTicked(new Set(w.lines.filter((l) => l.cleared).map((l) => l.lineId)));
  };

  const loadHistory = async (id: string) => {
    setHistory((await api.get('/reconciliations', { params: { accountId: id } })).data);
  };

  useEffect(() => {
    if (!accountId) return;
    // A reconciliation left in progress opens straight away.
    api.get('/reconciliations', { params: { accountId } })
      .then((r) => {
        setHistory(r.data);
        const draft = (r.data as Rec[]).find((x) => x.status === 'DRAFT');
        if (draft) return api.get(`/reconciliations/${draft.id}`).then((d) => open(d.data));
      })
      .catch((e) => setError(errMsg(e, 'Could not load the reconciliations')));
  }, [accountId]);

  const run = async (fn: () => Promise<void>, fallback: string) => {
    setBusy(true);
    setError('');
    try {
      await fn();
    } catch (e) {
      setError(errMsg(e, fallback));
    } finally {
      setBusy(false);
      setConfirm(null);
    }
  };

  const begin = (e: React.FormEvent) => {
    e.preventDefault();
    run(async () => {
      const r = await api.post('/reconciliations', {
        accountId, statementDate: start.statementDate, statementBalance: Number(start.statementBalance),
      });
      open(r.data);
      await loadHistory(accountId);
    }, 'Could not start the reconciliation');
  };

  const saveTicks = async () => {
    if (!ws) return;
    open((await api.put(`/reconciliations/${ws.id}/lines`, { lineIds: [...ticked] })).data);
  };

  const finish = () => run(async () => {
    if (!ws) return;
    await saveTicks();
    await api.post(`/reconciliations/${ws.id}/complete`);
    setWs(null);
    await loadHistory(accountId);
  }, 'Could not finish the reconciliation');

  const toggle = (id: string) => setTicked((t) => {
    const n = new Set(t);
    if (n.has(id)) n.delete(id); else n.add(id);
    return n;
  });
  const setAll = (ids: string[], on: boolean) => setTicked((t) => {
    const n = new Set(t);
    ids.forEach((id) => (on ? n.add(id) : n.delete(id)));
    return n;
  });

  // Worked out here as you tick, in cents; the server checks the same sum on Finish.
  const totals = useMemo(() => {
    if (!ws) return null;
    const cleared = ws.lines.filter((l) => ticked.has(l.lineId)).reduce((s, l) => s + cents(l.debit) - cents(l.credit), cents(ws.openingBalance));
    return { cleared: cleared / 100, difference: (cents(ws.statementBalance) - cleared) / 100 };
  }, [ws, ticked]);

  const readOnly = ws?.status === 'COMPLETED';
  const latestCompleted = history.find((h) => h.status === 'COMPLETED');
  const account = accounts.find((a) => a.id === accountId);

  return (
    <div>
      <PageHeader
        title="Bank Reconciliation"
        subtitle="Tick the entries your bank statement shows until the difference is zero, then finish."
      />

      {error && <div className="mb-3 px-3 py-2 bg-red-50 border border-red-200 text-red-700 text-sm rounded">{error}</div>}

      <div className="flex items-end gap-3 mb-4">
        <label className="text-xs font-medium text-gray-600">
          Account
          <select className="mt-1 w-72" value={accountId} disabled={!!ws && !readOnly} onChange={(e) => { setWs(null); setError(''); setAccountId(e.target.value); }}>
            {accounts.map((a) => <option key={a.id} value={a.id}>{a.code} — {a.name}</option>)}
          </select>
        </label>
      </div>

      {ws && totals ? (
        <div>
          <div className="grid grid-cols-2 lg:grid-cols-5 gap-2 mb-3">
            {[
              ['Statement date', ws.statementDate],
              ['Statement balance', fmt(ws.statementBalance)],
              ['Opening (last reconciled)', fmt(ws.openingBalance)],
              ['Cleared balance', fmt(totals.cleared)],
            ].map(([label, value]) => (
              <div key={label} className="bg-white border border-[#e0e0e0] rounded-md px-3 py-2">
                <p className="text-xs text-gray-600">{label}</p>
                <p className="text-base font-semibold tabular-nums">{value}</p>
              </div>
            ))}
            <div className={`border rounded-md px-3 py-2 ${totals.difference === 0 ? 'bg-emerald-50 border-emerald-200' : 'bg-amber-50 border-amber-200'}`}>
              <p className="flex items-center gap-1 text-xs text-gray-700">
                {totals.difference === 0 ? <CheckCircle2 size={13} className="text-emerald-600" /> : <AlertTriangle size={13} className="text-amber-600" />}
                Difference
              </p>
              <p className="text-base font-semibold tabular-nums" data-testid="rec-difference">{fmt(totals.difference)}</p>
            </div>
          </div>

          <div className="grid grid-cols-1 xl:grid-cols-2 gap-3 mb-3">
            <LineTable title="Deposits and other credits to the account" lines={ws.lines.filter((l) => l.debit > 0)}
              ticked={ticked} amount={(l) => l.debit} onToggle={toggle} onAll={setAll} readOnly={readOnly} />
            <LineTable title="Payments and withdrawals" lines={ws.lines.filter((l) => l.credit > 0)}
              ticked={ticked} amount={(l) => l.credit} onToggle={toggle} onAll={setAll} readOnly={readOnly} />
          </div>

          <div className="flex items-center gap-2">
            {readOnly ? (
              <button type="button" className="btn btn-secondary" onClick={() => setWs(null)}>Close</button>
            ) : (
              <>
                <button type="button" className="btn btn-primary" disabled={busy || totals.difference !== 0} onClick={finish}
                  title={totals.difference !== 0 ? 'The difference must be zero' : 'Mark the ticked entries as reconciled'}>
                  Finish
                </button>
                <button type="button" className="btn btn-secondary" disabled={busy}
                  onClick={() => run(saveTicks, 'Could not save')}>Save for later</button>
                <button type="button" className="btn btn-secondary" disabled={busy} onClick={() => setConfirm({ kind: 'discard' })}>Discard</button>
                <span className="text-xs text-gray-500 ml-2">
                  Bank charges or interest missing? Post them as a journal entry on the Ledger screen, then refresh (F5).
                </span>
              </>
            )}
          </div>
        </div>
      ) : (
        <div className="grid grid-cols-1 xl:grid-cols-3 gap-3">
          <form onSubmit={begin} className="card">
            <h3 className="text-sm font-semibold mb-1">Start a reconciliation</h3>
            <p className="text-xs text-gray-500 mb-3">
              {account ? `${account.code} — ${account.name}. ` : ''}Enter the closing date and balance from the bank statement.
            </p>
            <label className="block text-xs font-medium text-gray-600 mb-2">
              Statement date
              <input type="date" className="mt-1" required value={start.statementDate}
                onChange={(e) => setStart({ ...start, statementDate: e.target.value })} />
            </label>
            <label className="block text-xs font-medium text-gray-600 mb-3">
              Statement closing balance
              <input type="number" step="0.01" className="mt-1" required placeholder="Closing balance" value={start.statementBalance}
                onChange={(e) => setStart({ ...start, statementBalance: e.target.value })} />
            </label>
            <button type="submit" className="btn btn-primary" disabled={busy || !accountId}>Start</button>
          </form>

          <section className="xl:col-span-2 bg-white border border-[#e0e0e0] rounded-md overflow-hidden">
            <header className="px-3 py-2 bg-[#fafafa] border-b border-[#ececec]"><h3 className="text-sm font-semibold">History</h3></header>
            <table className="min-w-full text-sm">
              <thead>
                <tr className="bg-[#f5f5f5] border-b border-[#d9d9d9] text-xs text-gray-700">
                  <th className="px-3 py-1.5 text-left font-semibold">Statement date</th>
                  <th className="px-3 py-1.5 text-right font-semibold">Statement balance</th>
                  <th className="px-3 py-1.5 text-left font-semibold">Status</th>
                  <th className="px-3 py-1.5 text-left font-semibold">Completed by</th>
                  <th className="px-3 py-1.5 text-left font-semibold">Actions</th>
                </tr>
              </thead>
              <tbody>
                {history.map((h) => (
                  <tr key={h.id} className="border-b border-[#efefef] last:border-b-0">
                    <td className="px-3 py-1.5">{h.statementDate}</td>
                    <td className="px-3 py-1.5 text-right tabular-nums">{fmt(h.statementBalance)}</td>
                    <td className="px-3 py-1.5"><StatusBadge status={h.status} /></td>
                    <td className="px-3 py-1.5">{h.completedBy ?? '—'}</td>
                    <td className="px-3 py-1.5 space-x-3">
                      <button type="button" className="btn-link" onClick={() => run(async () => open((await api.get(`/reconciliations/${h.id}`)).data), 'Could not open it')}>
                        {h.status === 'DRAFT' ? 'Continue' : 'View'}
                      </button>
                      {h.id === latestCompleted?.id && (
                        <button type="button" className="btn-link" style={{ color: '#dc2626' }} onClick={() => setConfirm({ kind: 'undo', rec: h })}>Undo</button>
                      )}
                    </td>
                  </tr>
                ))}
                {history.length === 0 && (
                  <tr><td colSpan={5} className="px-3 py-6 text-center text-gray-500">This account has not been reconciled yet.</td></tr>
                )}
              </tbody>
            </table>
          </section>
        </div>
      )}

      {confirm && (
        <Dialog
          title={confirm.kind === 'discard' ? 'Discard reconciliation' : 'Undo reconciliation'}
          onClose={() => !busy && setConfirm(null)}
          buttons={
            <>
              <button type="button" className="btn btn-primary" disabled={busy} onClick={() =>
                confirm.kind === 'discard'
                  ? run(async () => { if (ws) await api.delete(`/reconciliations/${ws.id}`); setWs(null); await loadHistory(accountId); }, 'Could not discard it')
                  : run(async () => { open((await api.post(`/reconciliations/${confirm.rec.id}/undo`)).data); await loadHistory(accountId); }, 'Could not undo it')
              }>
                {confirm.kind === 'discard' ? 'Discard' : 'Undo'}
              </button>
              <button type="button" className="btn btn-secondary" disabled={busy} onClick={() => setConfirm(null)}>Cancel</button>
            </>
          }
        >
          <p className="text-sm text-gray-800">
            {confirm.kind === 'discard'
              ? 'Throw away this reconciliation and its ticks? Nothing in the ledger changes.'
              : `Reopen the reconciliation for ${confirm.rec.statementDate}? Its ticks are kept so you can correct them and finish again. Nothing in the ledger changes.`}
          </p>
        </Dialog>
      )}
    </div>
  );
}
