import { useEffect, useState } from 'react';
import { Link, useNavigate, useParams } from 'react-router-dom';
import api, { errMsg } from '../api/client';
import PageHeader from '../components/PageHeader';
import StatusBadge from '../components/StatusBadge';
import Dialog from '../components/desktop/Dialog';
import { money, moneyIn } from '../lib/money';
import { fmtDate } from '../lib/region';

/**
 * D35: one transaction in full — the journal entry with every line, and the document behind it
 * (invoice, bill, expense, payment, note, payroll run). Opened from the ledger, account
 * activity (every report drills down to it) and the document lists.
 */

interface Line {
  id: string; accountId: string; description?: string | null; debit: string; credit: string; foreignAmount?: string;
  account?: { id: string; code: string; name: string; currency?: string | null };
}
interface Entry {
  id: string; entryNumber: string; date: string; description: string; reference?: string | null; status: string;
  sourceType?: string | null; sourceId?: string | null; reversalOfId?: string | null; lines: Line[]; createdAt: string;
}
// Documents of several shapes (invoice, bill, payment…) pass through here.
// eslint-disable-next-line @typescript-eslint/no-explicit-any
type Doc = Record<string, any>;

const KIND: Record<string, { label: string; list: string }> = {
  INVOICE: { label: 'Invoice', list: '/invoices' },
  BILL: { label: 'Bill', list: '/bills' },
  EXPENSE: { label: 'Expense', list: '/expenses' },
  PAYMENT_AR: { label: 'Customer payment', list: '/payments' },
  PAYMENT_AP: { label: 'Vendor payment', list: '/payments' },
  PAYMENT_LIABILITY: { label: 'Payable account payment', list: '/payments' },
  STOCK_ADJUSTMENT: { label: 'Stock adjustment', list: '/items' },
  CREDIT_NOTE: { label: 'Credit note', list: '/credit-notes' },
  DEBIT_NOTE: { label: 'Debit note', list: '/debit-notes' },
  PAYROLL: { label: 'Payroll run', list: '/payroll' },
  PAYROLL_PAYMENT: { label: 'Salary payment', list: '/payroll' },
  PAYROLL_REMITTANCE: { label: 'Statutory remittance', list: '/payroll' },
  REVALUATION: { label: 'Currency revaluation', list: '/accounts' },
};

/** Loads the document an entry came from, where the app can show it. */
async function loadDocument(entry: Entry): Promise<Doc | null> {
  const id = entry.sourceId;
  if (!id) return null;
  switch (entry.sourceType) {
    case 'INVOICE': return (await api.get(`/invoices/${id}`)).data;
    case 'BILL': return (await api.get(`/bills/${id}`)).data;
    case 'EXPENSE': return (await api.get(`/expenses/${id}`)).data;
    case 'PAYMENT_LIABILITY': return (await api.get(`/payments/${id}`)).data;
    case 'PAYMENT_AR':
    case 'PAYMENT_AP': {
      // The payment's own id is not on the entry (its source is the paid document): find it.
      for (let page = 1; page <= 50; page++) {
        const r = await api.get('/payments', { params: { page, limit: 100 } });
        const rows: Doc[] = Array.isArray(r.data) ? r.data : r.data.data;
        const hit = rows.find((p) => p.journalEntryId === entry.id || p.journalEntryId === entry.reversalOfId);
        if (hit || rows.length < 100) return hit ?? null;
      }
      return null;
    }
    case 'CREDIT_NOTE':
    case 'DEBIT_NOTE': {
      const r = await api.get(entry.sourceType === 'CREDIT_NOTE' ? '/credit-notes' : '/debit-notes');
      const rows: Doc[] = Array.isArray(r.data) ? r.data : r.data.data;
      return rows.find((n) => n.id === id || n.journalEntryId === entry.id) ?? null;
    }
    default: return null;
  }
}

function DocLines({ doc }: { doc: Doc }) {
  const lines: Doc[] = doc.lines ?? [];
  if (!lines.length) return null;
  const cur = doc.currency ?? null;
  return (
    <table className="report-table mt-3">
      <thead>
        <tr><th>Description</th><th className="num">Qty</th><th className="num">Unit price</th><th className="num">Amount</th></tr>
      </thead>
      <tbody>
        {lines.map((l) => (
          <tr key={l.id}>
            <td>{l.description}</td>
            <td className="num">{Number(l.quantity)}</td>
            <td className="num">{moneyIn(Number(l.unitPrice), cur)}</td>
            <td className="num">{moneyIn(Number(l.amount), cur)}</td>
          </tr>
        ))}
        {Number(doc.taxAmount) > 0 && (
          <tr><td colSpan={3}>Tax</td><td className="num">{moneyIn(Number(doc.taxAmount), cur)}</td></tr>
        )}
        <tr className="total"><td colSpan={3}>Total</td><td className="num">{moneyIn(Number(doc.totalAmount), cur)}</td></tr>
      </tbody>
    </table>
  );
}

function DocSummary({ entry, doc }: { entry: Entry; doc: Doc }) {
  const kind = entry.sourceType ?? '';
  const cur = doc.currency ?? null;
  const rows: [string, React.ReactNode][] = [];
  const add = (label: string, value: unknown) => { if (value !== undefined && value !== null && value !== '') rows.push([label, value as React.ReactNode]); };
  if (kind === 'INVOICE') {
    add('Invoice', doc.invoiceNumber);
    add('Customer', doc.billToName || doc.customer?.name);
    add('Date', fmtDate(doc.date));
    add('Due', doc.dueDate ? fmtDate(doc.dueDate) : '');
    add('PO number', doc.poNumber);
    add('Paid', moneyIn(Number(doc.amountPaid ?? 0), cur));
    if (cur) add('Exchange rate', `1 ${cur} = ${doc.exchangeRate}`);
  } else if (kind === 'BILL') {
    add('Bill', doc.billNumber);
    add('Vendor', doc.vendor?.name);
    add('Date', fmtDate(doc.date));
    add('Due', doc.dueDate ? fmtDate(doc.dueDate) : '');
    if (cur) add('Exchange rate', `1 ${cur} = ${doc.exchangeRate}`);
  } else if (kind === 'EXPENSE') {
    add('Expense', doc.expenseNumber);
    add('Description', doc.description);
    add('Date', fmtDate(doc.date));
    add('Amount', money(Number(doc.amount)));
  } else if (kind.startsWith('PAYMENT')) {
    add('Payment', doc.paymentNumber);
    add('Date', fmtDate(doc.date));
    add('Amount', moneyIn(Number(doc.amount), cur));
    add('Method', doc.paymentMethod);
    if (Number(doc.withholdingAmount ?? 0)) add('Tax withheld', moneyIn(Number(doc.withholdingAmount), cur));
    if (doc.exchangeRate && Number(doc.exchangeRate) !== 1) add('Exchange rate', doc.exchangeRate);
    if (Number(doc.exchangeDifference ?? 0)) add('Exchange difference', money(Number(doc.exchangeDifference)));
    add('Notes', doc.notes);
  } else if (kind.endsWith('_NOTE')) {
    add(kind === 'CREDIT_NOTE' ? 'Credit note' : 'Debit note', doc.noteNumber);
    add('Date', fmtDate(doc.date));
    add('Reason', doc.reason);
    add('Amount', moneyIn(Number(doc.amount), cur));
  }
  if (!rows.length) return null;
  return (
    <dl className="grid gap-x-6 gap-y-1 text-sm" style={{ gridTemplateColumns: 'max-content 1fr' }}>
      {rows.map(([k, v]) => (
        <div key={k} className="contents">
          <dt className="text-gray-500">{k}</dt>
          <dd className="text-gray-900">{v}</dd>
        </div>
      ))}
    </dl>
  );
}

export default function TransactionDetail() {
  const { id } = useParams();
  const navigate = useNavigate();
  const [entry, setEntry] = useState<Entry | null>(null);
  const [doc, setDoc] = useState<Doc | null>(null);
  const [error, setError] = useState('');
  const [reversing, setReversing] = useState(false);
  const [busy, setBusy] = useState(false);

  const reverse = async () => {
    setBusy(true);
    try {
      const r = await api.post(`/ledger/entries/${id}/reverse`);
      setReversing(false);
      navigate(`/ledger/entries/${r.data.id}`, { replace: true });
    } catch (e) {
      setError(errMsg(e, 'Could not reverse the entry'));
      setReversing(false);
    } finally {
      setBusy(false);
    }
  };

  useEffect(() => {
    api.get(`/ledger/entries/${id}`)
      .then(async (r) => {
        const e: Entry = r.data;
        setEntry(e);
        setError('');
        setDoc(await loadDocument(e).catch(() => null));
      })
      .catch((e) => setError(errMsg(e, 'Could not load the transaction')));
  }, [id]);

  const kind = entry?.sourceType ? KIND[entry.sourceType] : null;
  const debits = entry?.lines.reduce((s, l) => s + Number(l.debit), 0) ?? 0;
  const credits = entry?.lines.reduce((s, l) => s + Number(l.credit), 0) ?? 0;
  const payrollRun = entry?.sourceType?.startsWith('PAYROLL') ? entry.sourceId : null;

  return (
    <div>
      <PageHeader
        title={entry ? `${entry.entryNumber} — ${entry.description}` : 'Transaction'}
        subtitle={entry ? `${fmtDate(entry.date)} · ${kind?.label ?? 'Journal entry'}${entry.reference ? ` · ${entry.reference}` : ''}` : undefined}
        action={
          <div className="flex gap-2 no-print">
            {/* Entries a document posted are undone from that document (D18). */}
            {entry && !entry.sourceType && !entry.reversalOfId && entry.status === 'POSTED' && (
              <button onClick={() => setReversing(true)} className="btn btn-secondary">Reverse this entry</button>
            )}
            <button onClick={() => navigate(-1)} className="btn btn-secondary">Back</button>
          </div>
        }
      />
      {error && <div className="mb-3 px-3 py-2 bg-red-50 border border-red-200 text-red-700 text-sm rounded">{error}</div>}
      {entry && (
        <div className="space-y-4">
          <div className="card">
            <div className="flex items-center gap-3 mb-3">
              <h3 className="text-sm font-semibold text-gray-700">{kind?.label ?? 'Manual journal entry'}</h3>
              <StatusBadge status={entry.status} />
              {entry.reversalOfId && (
                <Link className="btn-link text-xs" to={`/ledger/entries/${entry.reversalOfId}`}>Reverses an earlier entry →</Link>
              )}
              {kind && <Link className="btn-link text-xs ml-auto no-print" to={payrollRun ? `/payroll/${payrollRun}` : kind.list}>Open {kind.label.toLowerCase()} screen →</Link>}
            </div>
            {doc ? <><DocSummary entry={entry} doc={doc} /><DocLines doc={doc} /></>
              : <p className="text-sm text-gray-500">{entry.sourceType ? 'Posted by the screen above.' : 'Entered by hand on the Ledger screen.'}</p>}
          </div>

          <div className="card">
            <h3 className="text-sm font-semibold text-gray-700 mb-2">Ledger lines</h3>
            <table className="report-table">
              <thead>
                <tr><th>Account</th><th>Note</th><th className="num">Debit</th><th className="num">Credit</th></tr>
              </thead>
              <tbody>
                {entry.lines.map((l) => (
                  <tr key={l.id}>
                    <td>
                      <Link className="report-link" to={`/ledger/account/${l.accountId}`}>{l.account?.code} {l.account?.name}</Link>
                      {l.foreignAmount && l.account?.currency && (
                        <span className="text-xs text-gray-500"> · {moneyIn(Math.abs(Number(l.foreignAmount)), l.account.currency)}</span>
                      )}
                    </td>
                    <td className="text-gray-500">{l.description ?? ''}</td>
                    <td className="num">{Number(l.debit) ? money(Number(l.debit)) : ''}</td>
                    <td className="num">{Number(l.credit) ? money(Number(l.credit)) : ''}</td>
                  </tr>
                ))}
                <tr className="total">
                  <td colSpan={2}>Total</td>
                  <td className="num">{money(debits)}</td>
                  <td className="num">{money(credits)}</td>
                </tr>
              </tbody>
            </table>
          </div>
        </div>
      )}
      {reversing && entry && (
        <Dialog
          title="Reverse journal entry"
          onClose={() => !busy && setReversing(false)}
          buttons={
            <>
              <button type="button" className="btn btn-primary" disabled={busy} onClick={reverse}>{busy ? 'Reversing…' : 'Reverse entry'}</button>
              <button type="button" className="btn btn-secondary" disabled={busy} onClick={() => setReversing(false)}>Cancel</button>
            </>
          }
        >
          <p className="text-sm text-gray-800">Reverse <strong>{entry.entryNumber}</strong> ({entry.description})?</p>
          <p className="mt-2 text-sm text-gray-600">A new entry with the opposite amounts is posted today. The original stays in the ledger.</p>
        </Dialog>
      )}
    </div>
  );
}
