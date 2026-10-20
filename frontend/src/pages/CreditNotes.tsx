import { useEffect, useState } from 'react';
import api, { errMsg } from '../api/client';
import PageHeader from '../components/PageHeader';
import Table from '../components/Table';
import StatusBadge from '../components/StatusBadge';
import Dialog from '../components/desktop/Dialog';
import { useCommand } from '../lib/commands';
import { money } from '../lib/money';

/**
 * D21. AR: customer credit notes against posted invoices (revenue down, receivable down).
 * AP: vendor debit notes against posted bills (payable down, expense down).
 */
type Kind = 'AR' | 'AP';

interface Note {
  id: string;
  noteNumber: string;
  date: string;
  documentNumber: string | null;
  partyName: string | null;
  reason: string;
  amount: string;
  status: string;
}

interface Account { id: string; code: string; name: string; type: string; isActive: boolean }
interface DocLine { accountId: string | null }
interface Doc {
  id: string;
  number: string;
  party: string;
  date: string;
  balance: number | null;
  firstAccount: string | null;
}

const fmt = (v: number | string) => money(Number(v));
const todayIso = () => {
  const d = new Date();
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`;
};

const TEXT = {
  AR: {
    title: 'Credit Notes',
    subtitle: 'Reduce what a customer owes on a posted invoice — a discount, a return or an over-charge.',
    newLabel: '+ New Credit Note',
    docLabel: 'Invoice',
    partyLabel: 'Customer',
    accountLabel: 'Revenue account to reduce',
    path: '/credit-notes',
    empty: 'No credit notes yet.',
  },
  AP: {
    title: 'Debit Notes',
    subtitle: 'Reduce what you owe a vendor on a posted bill — goods returned or a price agreed down.',
    newLabel: '+ New Debit Note',
    docLabel: 'Bill',
    partyLabel: 'Vendor',
    accountLabel: 'Expense account to reduce',
    path: '/debit-notes',
    empty: 'No debit notes yet.',
  },
} as const;

const emptyForm = () => ({ documentId: '', date: todayIso(), amount: '', reason: '', accountId: '' });

export default function CreditNotes({ kind }: { kind: Kind }) {
  const t = TEXT[kind];
  const [rows, setRows] = useState<Note[]>([]);
  const [docs, setDocs] = useState<Doc[]>([]);
  const [accounts, setAccounts] = useState<Account[]>([]);
  const [showForm, setShowForm] = useState(false);
  const [form, setForm] = useState(emptyForm);
  const [error, setError] = useState('');
  const [saving, setSaving] = useState(false);
  const [voiding, setVoiding] = useState<Note | null>(null);

  useCommand('new', () => setShowForm(true));
  useCommand('close', () => setShowForm(false));

  const load = () =>
    api.get(t.path).then((r) => setRows(r.data)).catch((e) => setError(errMsg(e, 'Could not load the notes')));

  const loadDocs = () => {
    if (kind === 'AR') {
      api.get('/invoices', { params: { limit: 100 } }).then((r) =>
        setDocs(
          (r.data.data ?? r.data)
            .filter((i: { status: string }) => i.status === 'POSTED')
            .map((i: { id: string; invoiceNumber: string; customer?: { name: string }; billToName?: string; date: string;
              totalAmount: string; amountPaid?: string; amountCredited?: string; lines?: DocLine[] }) => ({
              id: i.id,
              number: i.invoiceNumber,
              party: i.customer?.name || i.billToName || 'one-off',
              date: i.date,
              balance: Number(i.totalAmount) - Number(i.amountPaid ?? 0) - Number(i.amountCredited ?? 0),
              firstAccount: i.lines?.find((l) => l.accountId)?.accountId ?? null,
            }))
            .filter((d: Doc) => (d.balance ?? 0) > 0),
        ),
      );
    } else {
      api.get('/bills', { params: { limit: 100 } }).then((r) =>
        setDocs(
          (r.data.data ?? r.data)
            .filter((b: { status: string }) => b.status === 'POSTED')
            .map((b: { id: string; billNumber: string; vendor?: { name: string }; date: string; lines?: DocLine[] }) => ({
              id: b.id,
              number: b.billNumber,
              party: b.vendor?.name ?? '',
              date: b.date,
              balance: null,
              firstAccount: b.lines?.find((l) => l.accountId)?.accountId ?? null,
            })),
        ),
      );
    }
  };

  useEffect(() => {
    load();
    loadDocs();
    api.get('/accounts').then((r) =>
      setAccounts(
        r.data.filter((a: Account) =>
          a.isActive && (kind === 'AR' ? a.type === 'REVENUE' : (a.type === 'EXPENSE' || a.type === 'ASSET')),
        ),
      ),
    );
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [kind]);

  // Choosing a document fills in what is owed and the account it was booked to.
  const chooseDoc = (id: string) => {
    const d = docs.find((x) => x.id === id);
    const fallback = kind === 'AR' ? '4000' : '6000';
    const account =
      accounts.find((a) => a.id === d?.firstAccount)?.id ?? accounts.find((a) => a.code === fallback)?.id ?? '';
    setForm((f) => ({
      ...f,
      documentId: id,
      amount: d?.balance != null ? d.balance.toFixed(2) : f.amount,
      accountId: account,
    }));
  };

  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    setError('');
    setSaving(true);
    try {
      await api.post(t.path, {
        [kind === 'AR' ? 'invoiceId' : 'billId']: form.documentId,
        date: form.date,
        amount: Number(form.amount),
        reason: form.reason,
        accountId: form.accountId,
      });
      setShowForm(false);
      setForm(emptyForm());
      load();
      loadDocs();
    } catch (err) {
      setError(errMsg(err, 'Could not save the note'));
    } finally {
      setSaving(false);
    }
  };

  const confirmVoid = async () => {
    if (!voiding) return;
    setSaving(true);
    setError('');
    try {
      await api.post(`${t.path}/${voiding.id}/void`);
      load();
      loadDocs();
    } catch (err) {
      setError(errMsg(err, 'Could not void the note'));
    } finally {
      setSaving(false);
      setVoiding(null);
    }
  };

  const chosen = docs.find((d) => d.id === form.documentId);

  return (
    <div>
      <PageHeader
        title={t.title}
        subtitle={t.subtitle}
        action={<button onClick={() => setShowForm((v) => !v)} className="btn btn-primary">{t.newLabel}</button>}
      />

      {error && <div className="mb-3 px-3 py-2 bg-red-50 border border-red-200 text-red-700 text-sm rounded">{error}</div>}

      {showForm && (
        <div className="card mb-4" style={{ maxWidth: 720 }}>
          <form onSubmit={submit} className="form-grid" style={{ gridTemplateColumns: '1fr 1fr' }}>
            <label className="text-xs font-medium text-gray-600" style={{ gridColumn: '1 / -1' }}>
              {t.docLabel}
              <select className="mt-1" required value={form.documentId} onChange={(e) => chooseDoc(e.target.value)}>
                <option value="">Select a posted {t.docLabel.toLowerCase()}</option>
                {docs.map((d) => (
                  <option key={d.id} value={d.id}>
                    {d.number} — {d.party}{d.balance != null ? ` · owed ${fmt(d.balance)}` : ''}
                  </option>
                ))}
              </select>
            </label>
            <label className="text-xs font-medium text-gray-600">
              Date
              <input type="date" className="mt-1" required min={chosen?.date} value={form.date}
                onChange={(e) => setForm({ ...form, date: e.target.value })} />
            </label>
            <label className="text-xs font-medium text-gray-600">
              Amount
              <input type="number" className="mt-1" required min="0.01" step="0.01" placeholder="Amount"
                max={chosen?.balance ?? undefined} value={form.amount}
                onChange={(e) => setForm({ ...form, amount: e.target.value })} />
            </label>
            <label className="text-xs font-medium text-gray-600" style={{ gridColumn: '1 / -1' }}>
              Reason
              <input className="mt-1" required maxLength={500} placeholder="e.g. 10% discount for late delivery"
                value={form.reason} onChange={(e) => setForm({ ...form, reason: e.target.value })} />
            </label>
            <label className="text-xs font-medium text-gray-600" style={{ gridColumn: '1 / -1' }}>
              {t.accountLabel}
              <select className="mt-1" required value={form.accountId} onChange={(e) => setForm({ ...form, accountId: e.target.value })}>
                <option value="">Select an account</option>
                {accounts.map((a) => <option key={a.id} value={a.id}>{a.code} — {a.name}</option>)}
              </select>
            </label>
            <div className="flex gap-2" style={{ gridColumn: '1 / -1' }}>
              <button type="submit" className="btn btn-primary" disabled={saving}>{saving ? 'Saving…' : 'Save Note'}</button>
              <button type="button" onClick={() => setShowForm(false)} className="btn btn-secondary">Cancel</button>
            </div>
          </form>
        </div>
      )}

      <Table
        data={rows}
        emptyMessage={t.empty}
        columns={[
          { header: '#', accessor: 'noteNumber' },
          { header: 'Date', accessor: 'date' },
          { header: t.docLabel, accessor: (r) => r.documentNumber ?? '—' },
          { header: t.partyLabel, accessor: (r) => r.partyName ?? '—' },
          { header: 'Reason', accessor: 'reason' },
          { header: 'Amount', accessor: (r) => fmt(r.amount) },
          { header: 'Status', accessor: (r) => <StatusBadge status={r.status} /> },
          { header: 'Actions', accessor: (r) => r.status !== 'VOID' && (
            <button onClick={() => setVoiding(r)} className="btn-link" style={{ color: '#dc2626' }}>Void</button>
          ) },
        ]}
      />

      {voiding && (
        <Dialog
          title={`Void ${voiding.noteNumber}`}
          onClose={() => !saving && setVoiding(null)}
          buttons={
            <>
              <button type="button" className="btn btn-primary" disabled={saving} onClick={confirmVoid}>Void note</button>
              <button type="button" className="btn btn-secondary" disabled={saving} onClick={() => setVoiding(null)}>Cancel</button>
            </>
          }
        >
          <p className="text-sm text-gray-800">
            Void <strong>{voiding.noteNumber}</strong> for <strong>{fmt(voiding.amount)}</strong> on {voiding.documentNumber}?
          </p>
          <p className="mt-2 text-sm text-gray-600">
            A reversing entry dated today is posted, and the amount is owed again on the {t.docLabel.toLowerCase()}.
            The note stays on this list, marked VOID.
          </p>
        </Dialog>
      )}
    </div>
  );
}
