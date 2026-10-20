import { useEffect, useMemo, useState } from 'react';
import { openInvoicePdf, previewInvoice, printInvoice, type InvoiceSettings } from '../utils/invoicePdf';
import { amountInWords } from '../utils/amountInWords';
import { errMsg } from '../api/client';
import { amount, currencyDigits } from '../lib/money';
import { company, region } from '../lib/region';
import { lineCodes, lineParts, summarize, useTaxCodes } from '../lib/taxes';
import CurrencyPicker from './CurrencyPicker';
import { moneyIn } from '../lib/money';

/**
 * Invoice editor rendered as the invoice document itself — same layout, fonts
 * and column order as the Preview / PDF output, but every value is editable.
 * What you type here is what the printed invoice looks like.
 *
 * Used for both raising a new invoice and editing a DRAFT: a draft has not
 * reached the ledger, so it stays fully editable until it is posted.
 *
 * Bill To, the payment options and the signatory are saved onto the invoice,
 * so later edits to the customer record or company settings never rewrite an
 * invoice that was already raised.
 */

export interface SheetLine {
  _key: string;
  description: string;
  quantity: number;
  unitPrice: number;
  accountId: string;
  /** D26: '' for no tax. */
  taxCodeId: string;
}

export interface SheetPayload {
  /** Omitted for a one-off invoice — the typed Bill To name stands alone. */
  customerId?: string;
  poNumber?: string;
  date: string;
  dueDate?: string;
  paymentTerms?: string;
  billToName?: string;
  billToAddress?: string;
  paymentOptions?: string[];
  signatoryName?: string;
  signatoryTitle?: string;
  lines: { description: string; quantity?: number; unitPrice?: number; accountId?: string; taxCodeId?: string }[];
  pricesIncludeTax: boolean;
  /** D30: omitted for the home currency. */
  currency?: string;
  exchangeRate?: number;
}

interface Props {
  customers: any[];
  accounts: any[];
  settings: InvoiceSettings;
  /** A DRAFT invoice to edit. Omit to raise a new one. */
  initial?: any;
  /** Should reject with a message when saving fails. */
  onSubmit: (payload: SheetPayload) => Promise<void>;
  onCancel: () => void;
}

const TERMS = [
  { value: '', label: 'Custom due date' },
  { value: 'DUE_ON_RECEIPT', label: 'Due on receipt' },
  { value: 'NET_15', label: 'Net 15' },
  { value: 'NET_30', label: 'Net 30' },
  { value: 'NET_60', label: 'Net 60' },
];
const TERM_DAYS: Record<string, number> = { DUE_ON_RECEIPT: 0, NET_15: 15, NET_30: 30, NET_60: 60 };

const uid = () => `${Date.now()}-${Math.random().toString(36).slice(2)}`;
const newLine = (accountId = '', taxCodeId = ''): SheetLine => ({ _key: uid(), description: '', quantity: 1, unitPrice: 0, accountId, taxCodeId });

/** The account new lines start on: 4000 Revenue if it exists, else the first one. */
const defaultAccountId = (accounts: any[]) =>
  accounts.find((a) => a.code === '4000')?.id ?? accounts[0]?.id ?? '';
const keyed = (text: string) => ({ _key: uid(), text });

const num = (v: number) =>
  amount(v);

const fmtDate = (d?: string) => {
  if (!d) return '';
  const date = new Date(d);
  if (isNaN(date.getTime())) return d;
  return date.toLocaleDateString(region().locale, { day: '2-digit', month: 'short', year: 'numeric' });
};

const today = () => new Date().toISOString().slice(0, 10);
// To the cent, or to a whole unit for a currency without decimals (D29) — as the server rounds.
const round2 = (n: number) => (currencyDigits() === 0 ? Math.round(n) : Number(n.toFixed(2)));

/** date + n days, as yyyy-mm-dd. */
const addDays = (date: string, days: number) => {
  const d = new Date(date);
  if (isNaN(d.getTime())) return '';
  d.setDate(d.getDate() + days);
  return d.toISOString().slice(0, 10);
};

export default function InvoiceSheetForm({ customers, accounts, settings, initial, onSubmit, onCancel }: Props) {
  const co = company();
  const editing = Boolean(initial);

  const [customerId, setCustomerId] = useState<string>(initial?.customerId ?? '');
  const [billToName, setBillToName] = useState<string>(initial?.billToName ?? initial?.customer?.name ?? '');
  const [billToAddress, setBillToAddress] = useState<string>(initial?.billToAddress ?? initial?.customer?.address ?? '');
  const [poNumber, setPoNumber] = useState<string>(initial?.poNumber ?? '');
  const [date, setDate] = useState<string>(initial?.date ?? today());
  const [dueDate, setDueDate] = useState<string>(initial?.dueDate ?? '');
  const [terms, setTerms] = useState<string>(initial?.paymentTerms ?? '');
  const fallbackAccount = defaultAccountId(accounts);
  const [lines, setLines] = useState<SheetLine[]>(
    initial?.lines?.length
      ? initial.lines.map((l: any) => ({
          _key: uid(),
          description: l.description,
          quantity: Number(l.quantity),
          unitPrice: Number(l.unitPrice),
          accountId: l.accountId,
          taxCodeId: l.taxCodeId ?? '',
        }))
      : [newLine(fallbackAccount, region().defaultSalesTax)],
  );
  const [pricesIncludeTax, setPricesIncludeTax] = useState<boolean>(Boolean(initial?.pricesIncludeTax));
  // D30: the invoice's currency ('' = home) and exchange rate.
  const [currency, setCurrency] = useState<string>(initial?.currency ?? '');
  const [rate, setRate] = useState<string>(initial?.currency ? String(initial.exchangeRate) : '');
  const taxCodes = useTaxCodes();
  const [payOptions, setPayOptions] = useState(() =>
    (initial?.paymentOptions
      ? String(initial.paymentOptions).split(/\r?\n/).filter(Boolean)
      : company().paymentOptions
    ).map(keyed),
  );
  const [sigName, setSigName] = useState<string>(
    initial?.signatoryName ?? settings.signatoryName?.trim() ?? '',
  );
  const [sigTitle, setSigTitle] = useState<string>(
    initial?.signatoryTitle ?? settings.signatoryTitle?.trim() ?? '',
  );
  const [error, setError] = useState('');
  const [saving, setSaving] = useState(false);

  // `accounts` arrives from the API a beat after the first render, so a line
  // created before then starts blank — fill those in once the list lands.
  useEffect(() => {
    if (!fallbackAccount) return;
    setLines((ls) => (ls.some((l) => !l.accountId)
      ? ls.map((l) => (l.accountId ? l : { ...l, accountId: fallbackAccount }))
      : ls));
  }, [fallbackAccount]);

  // Blank quantity means one of these; blank price means the line has no money on it.
  const lineAmount = (l: SheetLine) => round2((Number(l.quantity) || 1) * (Number(l.unitPrice) || 0));

  // D26: subtotal, tax per code and total — the same arithmetic the server uses.
  const summary = useMemo(
    () => summarize(lines.map((l) => ({ amount: lineAmount(l), taxCodeId: l.taxCodeId })), pricesIncludeTax),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [lines, pricesIncludeTax, taxCodes],
  );
  const total = summary.total;
  const anyTax = lines.some((l) => l.taxCodeId);

  const setLine = (i: number, field: keyof SheetLine, val: any) =>
    setLines((ls) => ls.map((l, idx) => (idx === i ? { ...l, [field]: val } : l)));

  const removeLine = (i: number) => setLines((ls) => (ls.length === 1 ? ls : ls.filter((_, idx) => idx !== i)));

  /**
   * Picking a customer refills Bill To — the fields stay editable afterwards.
   * Clearing it back to "one-off" keeps whatever is already typed there.
   */
  const pickCustomer = (id: string) => {
    setCustomerId(id);
    if (!id) return;
    const c = customers.find((x) => x.id === id);
    setBillToName(c?.name ?? '');
    setBillToAddress(c?.address ?? '');
  };

  /** Terms drive the due date; typing a due date by hand switches back to Custom. */
  const pickTerms = (value: string) => {
    setTerms(value);
    if (value) setDueDate(addDays(date, TERM_DAYS[value]));
  };

  const changeDate = (value: string) => {
    setDate(value);
    if (terms) setDueDate(addDays(value, TERM_DAYS[terms]));
  };

  const changeDueDate = (value: string) => {
    setDueDate(value);
    setTerms('');
  };

  const setPayOption = (i: number, text: string) =>
    setPayOptions((os) => os.map((o, idx) => (idx === i ? { ...o, text } : o)));

  /** The exact shape the PDF generator prints, built from unsaved state. */
  const asPdfInvoice = () => ({
    invoiceNumber: initial?.invoiceNumber ?? 'DRAFT',
    poNumber,
    date,
    dueDate,
    totalAmount: total,
    taxAmount: summary.tax,
    pricesIncludeTax,
    currency: currency || null,
    billToName,
    billToAddress,
    paymentOptions: payOptions.map((o) => o.text.trim()).filter(Boolean).join('\n'),
    signatoryName: sigName,
    signatoryTitle: sigTitle,
    lines: lines.map((l, i) => ({
      description: l.description,
      quantity: l.quantity,
      unitPrice: l.unitPrice,
      amount: lineAmount(l),
      taxCodeId: l.taxCodeId || null,
      taxAmount: summary.lineTax[i],
      taxes: lineParts(lineAmount(l), l.taxCodeId, pricesIncludeTax),
    })),
  });

  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    setError('');
    setSaving(true);
    try {
      await onSubmit({
        customerId: customerId || undefined,
        date,
        dueDate: dueDate || undefined,
        paymentTerms: terms || undefined,
        poNumber: poNumber.trim() || undefined,
        billToName: billToName.trim() || undefined,
        billToAddress: billToAddress.trim() || undefined,
        paymentOptions: payOptions.map((o) => o.text.trim()).filter(Boolean),
        signatoryName: sigName.trim() || undefined,
        signatoryTitle: sigTitle.trim() || undefined,
        // A blank account must go over as undefined — '' fails UUID validation.
        lines: lines.map(({ _key, accountId, quantity, unitPrice, taxCodeId, ...l }) => ({
          ...l,
          accountId: accountId || undefined,
          quantity: quantity || undefined,
          unitPrice: unitPrice || undefined,
          taxCodeId: taxCodeId || undefined,
        })),
        pricesIncludeTax,
        currency: currency || undefined,
        exchangeRate: currency ? Number(rate) : undefined,
      });
    } catch (err: any) {
      setError(errMsg(err, 'Failed to save invoice'));
    } finally {
      setSaving(false);
    }
  };

  return (
    <form onSubmit={submit}>
      {error && (
        <div className="mb-4 px-4 py-2.5 bg-red-50 border border-red-200 text-red-700 text-sm rounded-lg">{error}</div>
      )}

      <div className="inv-editor">
        {/* ── Bookkeeping panel — none of this prints on the invoice ── */}
        <aside className="inv-side">
          <div className="inv-side-title">Bookkeeping</div>
          <p className="inv-side-note">Not printed — it only tells the ledger where this invoice belongs.</p>

          <label className="inv-side-field">
            Customer record
            <select
              value={customerId}
              onChange={(e) => pickCustomer(e.target.value)}
              title="Optional — links the invoice to a customer record. Leave as one-off to just type a name on the invoice."
            >
              <option value="">One-off — no customer record</option>
              {customers.map((c) => <option key={c.id} value={c.id}>{c.name}</option>)}
            </select>
            <span className="inv-side-hint">Picking one fills in Bill To on the invoice.</span>
          </label>

          <label className="inv-side-field">
            Payment terms
            <select value={terms} onChange={(e) => pickTerms(e.target.value)}>
              {TERMS.map((t) => <option key={t.value} value={t.value}>{t.label}</option>)}
            </select>
            <span className="inv-side-hint">Sets the due date on the invoice.</span>
          </label>

          <div className="inv-side-title" style={{ marginTop: '1rem' }}>Revenue accounts</div>
          <p className="inv-side-note">Where each line is credited. Can wait — it is only needed when you post.</p>
          {lines.map((line, i) => (
            <label className="inv-side-field" key={line._key}>
              Line {i + 1}
              {line.description && <span className="inv-side-hint">{line.description}</span>}
              <select
                value={line.accountId}
                onChange={(e) => setLine(i, 'accountId', e.target.value)}
              >
                <option value="">Select revenue account…</option>
                {accounts.map((a) => <option key={a.id} value={a.id}>{a.code} – {a.name}</option>)}
              </select>
              {taxCodes.length > 0 && (
                <select
                  style={{ marginTop: 4 }}
                  aria-label={`Tax for line ${i + 1}`}
                  value={line.taxCodeId}
                  onChange={(e) => setLine(i, 'taxCodeId', e.target.value)}
                >
                  <option value="">No tax</option>
                  {lineCodes(taxCodes, line.taxCodeId).map((c) => (
                    <option key={c.id} value={c.id}>{c.name}</option>
                  ))}
                </select>
              )}
            </label>
          ))}
          {taxCodes.length > 0 && (
            <label className="inv-side-field" style={{ display: 'flex', gap: 6, alignItems: 'center' }}>
              <input type="checkbox" checked={pricesIncludeTax} onChange={(e) => setPricesIncludeTax(e.target.checked)} />
              Prices include tax
            </label>
          )}
          <div className="inv-side-title" style={{ marginTop: '1rem' }}>Currency</div>
          <CurrencyPicker currency={currency} rate={rate} onChange={(c, r) => { setCurrency(c); setRate(r); }} />
          {currency && Number(rate) > 0 && (
            <span className="inv-side-hint" style={{ whiteSpace: 'normal' }}>
              Booked as about {moneyIn(Math.round(total * Number(rate) * 100) / 100)} in the books.
            </span>
          )}
        </aside>

        <div className="inv-sheet-wrap">
        <div className="inv-sheet">
          {/* ── Letterhead ─────────────────────────────────────── */}
          <div className="logo-row">{co.logo ? <img src={co.logo} alt="logo" /> : <div className="co-name">{co.name}</div>}</div>
          <div className="rule" />
          <div className="contact">{co.contactLine}{co.emailWebLine && <><br />{co.emailWebLine}</>}{co.taxNumber && <><br />Tax No: {co.taxNumber}</>}</div>
          <div className="rule" />

          <h1>{anyTax ? 'Tax Invoice' : 'Invoice'}</h1>

          {/* ── Remit To / invoice meta ────────────────────────── */}
          <div className="meta">
            <div>
              <div className="label">Remit To:</div>
              <div>{co.name}</div>
              {co.addressLines.map((l) => <div key={l}>{l}</div>)}
            </div>
            <div className="meta-right">
              <div className="meta-row">
                <span className="meta-key">Date:</span>
                <input required type="date" className="inv-input" value={date} onChange={(e) => changeDate(e.target.value)} />
              </div>
              <div className="meta-row">
                <span className="meta-key"><strong>Invoice No:</strong></span>
                {editing
                  ? <span>{initial.invoiceNumber}</span>
                  : <span className="auto">auto-generated on save</span>}
              </div>
              <div className="meta-row">
                <span className="meta-key"><strong>PO No:</strong></span>
                <input className="inv-input" placeholder="optional" value={poNumber} onChange={(e) => setPoNumber(e.target.value)} />
              </div>
              <div className="meta-row">
                <span className="meta-key"><strong>Due Date:</strong></span>
                <input type="date" className="inv-input" value={dueDate} onChange={(e) => changeDueDate(e.target.value)} />
              </div>
            </div>
          </div>

          {/* ── Bill To: pick the customer, then edit how it prints ── */}
          <div className="block bill-block">
            <div className="label">Bill To:</div>
            <input
              required
              className="inv-input bill-to"
              placeholder="Name as it should print"
              value={billToName}
              onChange={(e) => setBillToName(e.target.value)}
            />
            <textarea
              rows={3}
              className="inv-input"
              placeholder="Address as it should print (one line per row)"
              value={billToAddress}
              onChange={(e) => setBillToAddress(e.target.value)}
            />
          </div>

          {/* ── Line items ─────────────────────────────────────── */}
          <table>
            <thead>
              <tr>
                <th style={{ width: 80 }}>Date</th>
                <th className="c-desc">Description</th>
                <th style={{ width: 62 }}>Qty</th>
                <th style={{ width: 110 }}>Unit Price ({currency || region().currencySymbol})</th>
                <th style={{ width: 105 }}>Price ({currency || region().currencySymbol})</th>
                <th className="gutter" />
              </tr>
            </thead>
            <tbody>
              {lines.map((line, i) => (
                <tr key={line._key}>
                  <td className="c-date">{i === 0 ? fmtDate(date) : ''}</td>
                  <td className="c-desc">
                    <input
                      required
                      className="inv-input"
                      placeholder="Item description"
                      value={line.description}
                      onChange={(e) => setLine(i, 'description', e.target.value)}
                    />
                  </td>
                  <td className="c-num">
                    <input
                      type="number" min="0.001" step="any"
                      className="inv-input right"
                      placeholder="1"
                      value={line.quantity || ''}
                      onChange={(e) => setLine(i, 'quantity', Number(e.target.value))}
                    />
                  </td>
                  <td className="c-num">
                    <input
                      type="number" min="0" step="0.01"
                      className="inv-input right"
                      value={line.unitPrice || ''}
                      onChange={(e) => setLine(i, 'unitPrice', Number(e.target.value))}
                    />
                  </td>
                  <td className="c-num">{lineAmount(line) ? num(lineAmount(line)) : ''}</td>
                  <td className="gutter">
                    {lines.length > 1 && (
                      <button type="button" className="row-del" title="Remove line" onClick={() => removeLine(i)}>×</button>
                    )}
                  </td>
                </tr>
              ))}
            </tbody>
            <tfoot>
              <tr>
                <td colSpan={3} style={{ border: 'none' }}>
                  <button type="button" className="btn-link" onClick={() => setLines([...lines, newLine(fallbackAccount, region().defaultSalesTax)])}>+ Add line</button>
                </td>
                <td className="total-label">{anyTax ? 'Subtotal' : 'Total'}</td>
                <td className="total-val">{num(anyTax ? summary.subtotal : total)}</td>
                <td className="gutter" />
              </tr>
              {anyTax && summary.byCode.map((t) => (
                <tr key={t.id}>
                  <td colSpan={3} style={{ border: 'none' }} />
                  <td className="total-label">{t.name}</td>
                  <td className="total-val">{num(t.tax)}</td>
                  <td className="gutter" />
                </tr>
              ))}
              {anyTax && (
                <tr>
                  <td colSpan={3} style={{ border: 'none' }} />
                  <td className="total-label">Total</td>
                  <td className="total-val" data-testid="invoice-total">{num(total)}</td>
                  <td className="gutter" />
                </tr>
              )}
            </tfoot>
          </table>

          <div className="words"><strong>Amount in words:</strong> {currency ? amountInWords(total, currency, 'Cents') : amountInWords(total, region().currencyWord, region().currencyMinorWord)}</div>

          {/* ── Payment options — editable per invoice ─────────── */}
          <div className="pay">
            <div className="label">Payment options</div>
            {payOptions.map((o, i) => (
              <div className="pay-row" key={o._key}>
                <textarea
                  rows={1}
                  className="inv-input pay-opt"
                  placeholder={`Payment option ${i + 1}`}
                  value={o.text}
                  onChange={(e) => setPayOption(i, e.target.value)}
                />
                <button
                  type="button"
                  className="row-del"
                  title="Remove option"
                  onClick={() => setPayOptions((os) => os.filter((_, idx) => idx !== i))}
                >
                  ×
                </button>
              </div>
            ))}
            <button
              type="button"
              className="btn-link"
              onClick={() => setPayOptions([...payOptions, keyed('')])}
            >
              + Add payment option
            </button>
          </div>

          {/* ── Signatory — editable per invoice ───────────────── */}
          <div className="sign">
            <div className="thanks">Thanking You</div>
            <input
              className="inv-input sig-name"
              placeholder="Signatory name"
              value={sigName}
              onChange={(e) => setSigName(e.target.value)}
            />
            <input
              className="inv-input sig-title"
              placeholder="Signatory title"
              value={sigTitle}
              onChange={(e) => setSigTitle(e.target.value)}
            />
          </div>

          {co.footerNote && <div className="sysnote">{co.footerNote}</div>}
        </div>

        {/* ── Actions ──────────────────────────────────────────── */}
        <div className="inv-actions">
          <button type="submit" className="btn btn-primary" disabled={saving}>
            {saving ? 'Saving…' : editing ? 'Save Changes' : 'Save Invoice'}
          </button>
          <button type="button" onClick={() => previewInvoice(asPdfInvoice(), settings)} className="btn btn-secondary">
            Preview
          </button>
          <button type="button" onClick={() => printInvoice(asPdfInvoice(), settings)} className="btn btn-secondary">
            Print
          </button>
          <button type="button" onClick={() => openInvoicePdf(asPdfInvoice(), settings)} className="btn btn-secondary">
            Download
          </button>
          <button type="button" onClick={onCancel} className="btn btn-secondary">Cancel</button>
          <span className="text-xs text-gray-500">
            {editing
              ? 'Still a draft — post it from the list to hit the ledger.'
              : 'Saves as a draft — post it from the list to hit the ledger.'}
          </span>
        </div>
        </div>
      </div>
    </form>
  );
}
