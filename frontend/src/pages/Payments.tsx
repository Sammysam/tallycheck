import { useEffect, useState } from 'react';
import api, { errMsg } from '../api/client';
import PageHeader from '../components/PageHeader';
import { useCommand } from '../lib/commands';
import Table from '../components/Table';
import Pagination from '../components/Pagination';
import StatusBadge from '../components/StatusBadge';
import { taxOf, useTaxCodes } from '../lib/taxes';
import { money, moneyIn } from '../lib/money';
import { region } from '../lib/region';
import Dialog from '../components/desktop/Dialog';

interface Payment {
  id: string; paymentNumber: string; type: string; date: string; amount: number; paymentMethod: string; status: string;
  withholdingAmount?: string; currency?: string | null; exchangeRate?: number; baseAmount?: string; exchangeDifference?: string;
}

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
  const [form, setForm] = useState({ type: 'AR', invoiceId: '', billId: '', date: '', amount: '', paymentMethod: 'BANK_TRANSFER', paymentAccountId: '', notes: '', withholdingTaxCodeId: '', withholdingAmount: '', exchangeRate: '' });
  const [error, setError] = useState('');
  const [voiding, setVoiding] = useState<Payment | null>(null);
  const [busy, setBusy] = useState(false);
  // D27: withholding tax the payer holds back.
  const withholdingCodes = useTaxCodes().filter((c) => c.kind === 'WITHHOLDING' && c.isActive);

  /** Suggests what is withheld (rate × the document's amount before tax) and the money paid. */
  // D30: the chosen document's currency; a foreign one is paid at the day's rate.
  const chosenDoc = form.type === 'AR' ? invoices.find((i) => i.id === form.invoiceId) : bills.find((b) => b.id === form.billId);
  const docCurrency: string = chosenDoc?.currency ?? '';
  const chooseDoc = (field: 'invoiceId' | 'billId', id: string) => {
    const doc = (field === 'invoiceId' ? invoices : bills).find((d) => d.id === id);
    const cur = doc?.currency ?? '';
    setForm({ ...form, [field]: id, exchangeRate: cur ? String(region().lastRates[cur] ?? doc?.exchangeRate ?? '') : '' });
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
    api.get('/invoices', { params: { limit: 100 } }).then((r) => setInvoices((r.data.data ?? r.data).filter((i: any) => i.status === 'POSTED')));
    api.get('/bills',    { params: { limit: 100 } }).then((r) => setBills((r.data.data ?? r.data).filter((b: any) => b.status === 'POSTED')));
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
    api.get('/accounts').then((r) => setAccounts(r.data.filter((a: any) => a.type === 'ASSET')));
    loadDocuments();
  }, []);

  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    setError('');
    try {
      const payload = {
        ...form,
        amount: Number(form.amount),
        withholdingTaxCodeId: form.withholdingTaxCodeId || undefined,
        withholdingAmount: form.withholdingTaxCodeId ? Number(form.withholdingAmount) : undefined,
        exchangeRate: docCurrency && form.exchangeRate ? Number(form.exchangeRate) : undefined,
        invoiceId: form.type === 'AR' ? form.invoiceId : undefined,
        billId:    form.type === 'AP' ? form.billId    : undefined,
      };
      await api.post('/payments', payload);
      setShowForm(false);
      setForm({ type: 'AR', invoiceId: '', billId: '', date: '', amount: '', paymentMethod: 'BANK_TRANSFER', paymentAccountId: '', notes: '', withholdingTaxCodeId: '', withholdingAmount: '', exchangeRate: '' });
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
                <option value="AR">AR — Customer Payment</option>
                <option value="AP">AP — Vendor Payment</option>
              </select>
              {form.type === 'AR'
                ? <select required value={form.invoiceId} onChange={(e) => chooseDoc('invoiceId', e.target.value)}>
                    <option value="">Select Posted Invoice</option>
                    {invoices.map((i) => (
                      <option key={i.id} value={i.id}>
                        {i.invoiceNumber} — {i.customer?.name || i.billToName || 'one-off'} · balance {fmt(Number(i.totalAmount) - Number(i.amountPaid ?? 0) - Number(i.amountCredited ?? 0))}
                      </option>
                    ))}
                  </select>
                : <select required value={form.billId} onChange={(e) => chooseDoc('billId', e.target.value)}>
                    <option value="">Select Posted Bill</option>
                    {bills.map((b) => <option key={b.id} value={b.id}>{b.billNumber} — {b.vendor?.name}</option>)}
                  </select>
              }
            </div>
            <div className="form-grid" style={{ gridTemplateColumns: '1fr 1fr 1fr 1fr' }}>
              <input required type="date" value={form.date} onChange={(e) => setForm({ ...form, date: e.target.value })} />
              <input required type="number" min="0.01" step="0.01" placeholder="Amount" value={form.amount} onChange={(e) => setForm({ ...form, amount: e.target.value })} />
              <select value={form.paymentMethod} onChange={(e) => setForm({ ...form, paymentMethod: e.target.value })}>
                <option value="BANK_TRANSFER">Bank Transfer</option>
                <option value="CASH">Cash</option>
                <option value="CHECK">Check</option>
              </select>
              <select required value={form.paymentAccountId} onChange={(e) => setForm({ ...form, paymentAccountId: e.target.value })}>
                <option value="">Bank / Cash Account</option>
                {accounts.map((a) => <option key={a.id} value={a.id}>{a.code} – {a.name}</option>)}
              </select>
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
                    <>You receive about {money(Math.round(Number(form.amount) * Number(form.exchangeRate) * 100) / 100)}; any difference
                    from the invoice's rate ({chosenDoc?.exchangeRate}) is an exchange gain or loss.</>
                  )}
                </p>
              </div>
            )}
            {withholdingCodes.length > 0 && (
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
        { header: '#',      accessor: 'paymentNumber' },
        { header: 'Type',   accessor: 'type' },
        { header: 'Date',   accessor: 'date' },
        { header: 'Amount', accessor: (r) => moneyIn(Number(r.amount), r.currency) },
        { header: 'Method', accessor: 'paymentMethod' },
        { header: 'Withheld', accessor: (r) => (Number(r.withholdingAmount ?? 0) ? moneyIn(Number(r.withholdingAmount), r.currency) : '') },
        { header: 'In the books', accessor: (r) => (r.currency ? `${money(Number(r.baseAmount))} @ ${r.exchangeRate}` : '') },
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
