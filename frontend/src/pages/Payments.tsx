import { useEffect, useRef, useState } from 'react';
import api, { errMsg } from '../api/client';
import PageHeader from '../components/PageHeader';
import { useCommand } from '../lib/commands';
import Table from '../components/Table';
import Pagination from '../components/Pagination';
import StatusBadge from '../components/StatusBadge';
import { taxOf, useTaxCodes } from '../lib/taxes';
import { money, moneyIn } from '../lib/money';
import { fmtDate, region, todayIso } from '../lib/region';
import { useSearchParams } from 'react-router-dom';
import { toast } from '../lib/toast';
import Dialog from '../components/desktop/Dialog';
import TxnLink from '../components/TxnLink';

interface Payment {
  id: string; paymentNumber: string; type: string; date: string; amount: number; paymentMethod: string; status: string;
  withholdingAmount?: string; currency?: string | null; exchangeRate?: number; baseAmount?: string; exchangeDifference?: string;
  payableAccountId?: string | null; invoiceId?: string | null; billId?: string | null;
}

/** An invoice or bill as the payments list names it. */
interface DocRow { id: string; invoiceNumber?: string; billNumber?: string; billToName?: string | null; customer?: { name?: string } | null; vendor?: { name?: string } | null }

/** An open invoice or bill with what is still owed on it. */
interface OpenDoc { id: string; totalAmount: number | string; amountPaid?: number | string; amountCredited?: number | string }

/** Payment types and methods in plain words. */
const TYPE_LABEL: Record<string, string> = { AR: 'From a customer', AP: 'To a vendor' };
const METHOD_LABEL: Record<string, string> = { BANK_TRANSFER: 'Bank transfer', CASH: 'Cash', CHECK: 'Check' };
const blankForm = () => ({ type: 'AR', invoiceId: '', billId: '', payableAccountId: '', date: todayIso(), amount: '', paymentMethod: 'BANK_TRANSFER', paymentAccountId: '', notes: '', withholdingTaxCodeId: '', withholdingAmount: '', exchangeRate: '' });

/** D33: liabilities split as the balance sheet reads them. */
const NON_CURRENT = ['non_current_liability', 'long_term_liability', 'long_term_loan', 'non_current'];

export default function Payments() {
  const [rows, setRows] = useState<Payment[]>([]);
  const [page, setPage] = useState(1);
  const [totalPages, setTotalPages] = useState(1);
  const [total, setTotal] = useState(0);
  const limit = 10;

  const [accounts, setAccounts] = useState<any[]>([]);
  const [invoices, setInvoices] = useState<any[]>([]);
  const [bills, setBills] = useState<any[]>([]);
  const [showForm, setShowForm] = useState(false);
  // Ctrl+N / Esc and the toolbar, via the desktop shell.
  useCommand('new', () => setShowForm(true));
  useCommand('close', () => setShowForm(false));
  const [form, setForm] = useState(blankForm);
  // Every invoice and bill (paid ones too), to say on the list what each payment was for.
  const [allInvoices, setAllInvoices] = useState<DocRow[]>([]);
  const [allBills, setAllBills] = useState<DocRow[]>([]);
  const [params] = useSearchParams();
  // The invoice / bill to open the form for, from the address (read once).
  const wanted = useRef({ invoice: params.get('invoice'), bill: params.get('bill') });
  const [error, setError] = useState('');
  const [voiding, setVoiding] = useState<Payment | null>(null);
  const [busy, setBusy] = useState(false);
  // D27: withholding tax the payer holds back.
  const withholdingCodes = useTaxCodes().filter((c) => c.kind === 'WITHHOLDING' && c.isActive);

  /** Suggests what is withheld (rate × the document's amount before tax) and the money paid. */
  // D30: the chosen document's currency; a foreign one is paid at the day's rate.
  const chosenDoc = form.type === 'AR' ? invoices.find((i) => i.id === form.invoiceId) : bills.find((b) => b.id === form.billId);
  const isAccount = form.type === 'ACCOUNT';
  // Money accounts only: cash and bank (10xx) or accounts named or typed as bank / cash.
  const bankAccounts = accounts.filter((a) => a.type === 'ASSET' && a.isActive !== false
    && (String(a.code).startsWith('10') || /bank|cash|petty|paypal|stripe|clearing/i.test(`${a.name} ${a.subtype ?? ''}`)));
  const liabilities = accounts.filter((a) => a.type === 'LIABILITY' && !a.currency);
  // D34: a payable account paid from a USD account is entered in USD at the day's rate.
  const bankCurrency: string = accounts.find((a) => a.id === form.paymentAccountId)?.currency ?? '';
  const docCurrency: string = isAccount ? bankCurrency : chosenDoc?.currency ?? '';
  const chooseDoc = (field: 'invoiceId' | 'billId', id: string) => {
    const doc = (field === 'invoiceId' ? invoices : bills).find((d) => d.id === id);
    const cur = doc?.currency ?? '';
    // What is still owed on it, as the amount to receive / pay.
    const left = doc ? Number(doc.totalAmount) - Number(doc.amountPaid ?? 0) - Number(doc.amountCredited ?? 0) : 0;
    setForm({ ...form, [field]: id, amount: doc && left > 0 ? left.toFixed(2) : form.amount, exchangeRate: cur ? String(region().lastRates[cur] ?? doc?.exchangeRate ?? '') : '' });
  };

  const chooseWithholding = (codeId: string) => {
    const code = withholdingCodes.find((c) => c.id === codeId);
    const doc = form.type === 'AR' ? invoices.find((i) => i.id === form.invoiceId) : bills.find((b) => b.id === form.billId);
    if (!code || !doc) {
      setForm({ ...form, withholdingTaxCodeId: codeId, withholdingAmount: codeId ? form.withholdingAmount : '' });
      return;
    }
    const total = Number(doc.totalAmount);
    const balance = total - Number(doc.amountPaid ?? 0) - Number(doc.amountCredited ?? 0);
    const base = (total - Number(doc.taxAmount ?? 0)) * (total > 0 ? Math.min(1, balance / total) : 0);
    const withheld = taxOf(base, code.rate, false);
    setForm({
      ...form,
      withholdingTaxCodeId: codeId,
      withholdingAmount: withheld.toFixed(2),
      amount: (Math.round((balance - withheld) * 100) / 100).toFixed(2),
    });
  };

  const load = (p = page) =>
    api.get('/payments', { params: { page: p, limit } }).then((r) => {
      setRows(r.data.data);
      setTotal(r.data.total);
      setTotalPages(r.data.totalPages);
    });

  useEffect(() => { load(); }, [page]);
  const loadDocuments = () => {
    // "Receive payment" on an invoice / "Pay bill" on a bill opens the form for that document.
    const openFor = (kind: 'invoice' | 'bill', docs: OpenDoc[]) => {
      const id = wanted.current[kind];
      if (!id) return;
      wanted.current[kind] = null;
      const doc = docs.find((d) => d.id === id);
      if (doc) {
        const left = Number(doc.totalAmount) - Number(doc.amountPaid ?? 0) - Number(doc.amountCredited ?? 0);
        setForm({ ...blankForm(), type: kind === 'invoice' ? 'AR' : 'AP', invoiceId: kind === 'invoice' ? id : '', billId: kind === 'bill' ? id : '', amount: left > 0 ? left.toFixed(2) : '' });
        setShowForm(true);
      }
    };
    api.get('/invoices', { params: { limit: 100 } }).then((r) => {
      const all = r.data.data ?? r.data;
      setAllInvoices(all);
      const posted = all.filter((i: any) => i.status === 'POSTED');
      setInvoices(posted);
      openFor('invoice', posted);
    });
    api.get('/bills',    { params: { limit: 100 } }).then((r) => {
      const all = r.data.data ?? r.data;
      setAllBills(all);
      const open = all.filter((b: any) => b.status === 'POSTED');
      setBills(open);
      openFor('bill', open);
    });
  };

  // Reverses the payment in the ledger; its invoice or bill is owed again (D18).
  const confirmVoid = async () => {
    if (!voiding) return;
    setBusy(true);
    setError('');
    try {
      await api.post(`/payments/${voiding.id}/void`);
      setVoiding(null);
      load();
      loadDocuments();
    } catch (err) {
      setVoiding(null);
      setError(errMsg(err, 'Could not void the payment'));
    } finally {
      setBusy(false);
    }
  };

  useEffect(() => {
    api.get('/accounts').then((r) => setAccounts(r.data));
    loadDocuments();
  }, []);


  /** What a payment was for: the invoice / bill number and who. */
  const forWhat = (r: Payment) => {
    if (r.invoiceId) { const d = allInvoices.find((x) => x.id === r.invoiceId); return d ? `${d.invoiceNumber} · ${d.customer?.name || d.billToName || 'customer'}` : 'Invoice'; }
    if (r.billId) { const d = allBills.find((x) => x.id === r.billId); return d ? `${d.billNumber} · ${d.vendor?.name ?? 'vendor'}` : 'Bill'; }
    return '';
  };

  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    setError('');
    try {
      const { payableAccountId, ...rest } = form;
      const payload = {
        ...rest,
        type: isAccount ? 'AP' : form.type,
        payableAccountId: isAccount ? payableAccountId : undefined,
        amount: Number(form.amount),
        withholdingTaxCodeId: form.withholdingTaxCodeId || undefined,
        withholdingAmount: form.withholdingTaxCodeId ? Number(form.withholdingAmount) : undefined,
        exchangeRate: docCurrency && form.exchangeRate ? Number(form.exchangeRate) : undefined,
        invoiceId: form.type === 'AR' ? form.invoiceId : undefined,
        billId:    form.type === 'AP' ? form.billId    : undefined,
      };
      await api.post('/payments', payload);
      setShowForm(false);
      setForm(blankForm());
      toast('Payment recorded');
      loadDocuments();
      load(1); setPage(1);
    } catch (err: any) {
      setError(err?.response?.data?.message ?? 'Failed to record payment');
    }
  };

  const fmt = (v: number) => money(Number(v));

  return (
    <div>
      <PageHeader title="Payments" action={<button onClick={() => setShowForm(!showForm)} className="btn btn-primary">+ Record Payment</button>} />

      {error && <div className="mb-4 px-4 py-2.5 bg-red-50 border border-red-200 text-red-700 text-sm rounded-lg">{error}</div>}

      {showForm && (
        <div className="card mb-5">
          <h3 className="text-sm font-semibold text-gray-700 mb-3">Record Payment</h3>
          <form onSubmit={submit} className="space-y-3">
            <div className="form-grid" style={{ gridTemplateColumns: '1fr 1fr' }}>
              <select value={form.type} onChange={(e) => setForm({ ...form, type: e.target.value, invoiceId: '', billId: '' })}>
                <option value="AR">Money received from a customer (invoice)</option>
                <option value="AP">Money paid to a vendor (bill)</option>
                <option value="ACCOUNT">Pay a liability account (e.g. a loan or tax)</option>
              </select>
              {isAccount
                ? <select required value={form.payableAccountId} onChange={(e) => setForm({ ...form, payableAccountId: e.target.value })} aria-label="Payable account">
                    <option value="">Select payable account</option>
                    <optgroup label="Current liabilities">
                      {liabilities.filter((a) => !NON_CURRENT.includes(a.subtype ?? '')).map((a) => <option key={a.id} value={a.id}>{a.code} – {a.name}</option>)}
                    </optgroup>
                    <optgroup label="Non-current liabilities">
                      {liabilities.filter((a) => NON_CURRENT.includes(a.subtype ?? '')).map((a) => <option key={a.id} value={a.id}>{a.code} – {a.name}</option>)}
                    </optgroup>
                  </select>
                : form.type === 'AR'
                ? <select required value={form.invoiceId} onChange={(e) => chooseDoc('invoiceId', e.target.value)}>
                    <option value="">Which invoice is being paid?</option>
                    {invoices.map((i) => (
                      <option key={i.id} value={i.id}>
                        {i.invoiceNumber} — {i.customer?.name || i.billToName || 'one-off'} · balance {fmt(Number(i.totalAmount) - Number(i.amountPaid ?? 0) - Number(i.amountCredited ?? 0))}
                      </option>
                    ))}
                  </select>
                : <select required value={form.billId} onChange={(e) => chooseDoc('billId', e.target.value)}>
                    <option value="">Which bill are you paying?</option>
                    {bills.map((b) => <option key={b.id} value={b.id}>{b.billNumber} — {b.vendor?.name}</option>)}
                  </select>
              }
            </div>
            <div className="form-grid" style={{ gridTemplateColumns: '1fr 1fr 1fr 1fr' }}>
              <label className="block"><span className="text-xs font-medium text-gray-500">Date</span>
                <input required type="date" value={form.date} onChange={(e) => setForm({ ...form, date: e.target.value })} aria-label="Payment date" /></label>
              <label className="block"><span className="text-xs font-medium text-gray-500">Amount</span>
                <input required type="number" min="0.01" step="0.01" placeholder="Amount" value={form.amount} onChange={(e) => setForm({ ...form, amount: e.target.value })} aria-label="Amount" /></label>
              <label className="block"><span className="text-xs font-medium text-gray-500">How</span>
              <select value={form.paymentMethod} onChange={(e) => setForm({ ...form, paymentMethod: e.target.value })} aria-label="Payment method">
                <option value="BANK_TRANSFER">Bank Transfer</option>
                <option value="CASH">Cash</option>
                <option value="CHECK">Check</option>
              </select></label>
              <label className="block"><span className="text-xs font-medium text-gray-500">Into / out of which account</span>
              <select required value={form.paymentAccountId} onChange={(e) => setForm({ ...form, paymentAccountId: e.target.value })} aria-label="Bank or cash account">
                <option value="">Choose a bank or cash account</option>
                {bankAccounts.map((a) => <option key={a.id} value={a.id}>{a.code} – {a.name}{a.currency ? ` (${a.currency})` : ''}</option>)}
              </select></label>
            </div>
            {docCurrency && (
              <div className="form-grid items-end" style={{ gridTemplateColumns: '1fr 2fr' }}>
                <label className="text-xs font-medium text-gray-600">
                  Rate today: 1 {docCurrency} = how many {region().currency}?
                  <input className="mt-1" required type="number" min="0.00000001" step="any" placeholder="Exchange rate"
                    value={form.exchangeRate} onChange={(e) => setForm({ ...form, exchangeRate: e.target.value })} aria-label="Payment exchange rate" />
                </label>
                <p className="text-xs text-gray-600 pb-2">
                  The amount is in {docCurrency}. {Number(form.amount) > 0 && Number(form.exchangeRate) > 0 && (
                    isAccount
                      ? <>That is {money(Math.round(Number(form.amount) * Number(form.exchangeRate) * 100) / 100)} off the payable account.</>
                      : <>You receive about {money(Math.round(Number(form.amount) * Number(form.exchangeRate) * 100) / 100)}; any difference
                        from the invoice's rate ({chosenDoc?.exchangeRate}) is an exchange gain or loss.</>
                  )}
                </p>
              </div>
            )}
            {withholdingCodes.length > 0 && !isAccount && (
              <div className="form-grid items-end" style={{ gridTemplateColumns: '1fr 1fr 2fr' }}>
                <label className="text-xs font-medium text-gray-600">
                  {form.type === 'AR' ? 'Tax the customer withheld' : 'Tax we withhold'}
                  <select className="mt-1" value={form.withholdingTaxCodeId} onChange={(e) => chooseWithholding(e.target.value)} aria-label="Withholding tax">
                    <option value="">None</option>
                    {withholdingCodes.map((c) => <option key={c.id} value={c.id}>{c.name}</option>)}
                  </select>
                </label>
                {form.withholdingTaxCodeId ? (
                  <label className="text-xs font-medium text-gray-600">
                    Amount withheld
                    <input className="mt-1" required type="number" min="0.01" step="0.01" placeholder="Amount withheld" value={form.withholdingAmount}
                      onChange={(e) => setForm({ ...form, withholdingAmount: e.target.value })} />
                  </label>
                ) : <span />}
                {form.withholdingTaxCodeId && Number(form.amount) > 0 && (
                  <p className="text-xs text-gray-600 pb-2">
                    Settles {fmt(Number(form.amount) + (Number(form.withholdingAmount) || 0))}: {fmt(Number(form.amount))} paid
                    + {fmt(Number(form.withholdingAmount) || 0)} withheld for the tax office.
                  </p>
                )}
              </div>
            )}
            <div className="flex gap-2">
              <button type="submit" className="btn btn-primary">Record Payment</button>
              <button type="button" onClick={() => setShowForm(false)} className="btn btn-secondary">Cancel</button>
            </div>
          </form>
        </div>
      )}

      <Table data={rows} columns={[
        { header: '#',      accessor: (r) => <TxnLink entryId={(r as { journalEntryId?: string }).journalEntryId}>{r.paymentNumber}</TxnLink> },
        { header: 'Type',   accessor: (r) => {
          if (!r.payableAccountId) return TYPE_LABEL[r.type] ?? r.type;
          const a = accounts.find((x) => x.id === r.payableAccountId);
          return <span title="Paid a payable account">AP · {a ? `${a.code} ${a.name}` : 'payable account'}</span>;
        } },
        { header: 'For',    accessor: (r) => forWhat(r) },
        { header: 'Date',   accessor: (r) => fmtDate(r.date) },
        { header: 'Amount', accessor: (r) => moneyIn(Number(r.amount), r.currency) },
        { header: 'Method', accessor: (r) => METHOD_LABEL[r.paymentMethod] ?? r.paymentMethod },
        { header: 'Withheld', accessor: (r) => (Number(r.withholdingAmount ?? 0) ? moneyIn(Number(r.withholdingAmount), r.currency) : '') },
        { header: 'In the books', accessor: (r) => (r.currency || Number(r.exchangeRate ?? 1) !== 1 ? `${money(Number(r.baseAmount))} @ ${r.exchangeRate}` : '') },
        { header: 'Exchange', accessor: (r) => (Number(r.exchangeDifference ?? 0) ? money(-Number(r.exchangeDifference) * (r.type === 'AR' ? 1 : -1)) : '') },
        { header: 'Status', accessor: (r) => <StatusBadge status={r.status ?? 'POSTED'} /> },
        { header: 'Actions', accessor: (r) => r.status !== 'VOID' && (
          <button onClick={() => setVoiding(r)} className="btn-link" style={{ color: '#dc2626' }}>Void</button>
        ) },
      ]} />
      <Pagination page={page} totalPages={totalPages} total={total} limit={limit} onPage={(p) => { setPage(p); load(p); }} />

      {voiding && (
        <Dialog
          title="Void payment"
          onClose={() => !busy && setVoiding(null)}
          buttons={
            <>
              <button type="button" className="btn btn-primary" disabled={busy} onClick={confirmVoid}>
                {busy ? 'Voiding…' : 'Void payment'}
              </button>
              <button type="button" className="btn btn-secondary" disabled={busy} onClick={() => setVoiding(null)}>Cancel</button>
            </>
          }
        >
          <p className="text-sm text-gray-800">
            Void <strong>{voiding.paymentNumber}</strong> for <strong>{fmt(voiding.amount)}</strong>?
          </p>
          <ul className="mt-2 text-sm text-gray-600 list-disc pl-5 space-y-0.5">
            <li>A reversing entry dated today is posted to the ledger.</li>
            <li>The {voiding.type === 'AR' ? 'invoice' : 'bill'} it paid is owed again.</li>
            <li>The payment stays on this list, marked VOID. This cannot be undone.</li>
          </ul>
        </Dialog>
      )}
    </div>
  );
}
