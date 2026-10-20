import { useEffect, useState } from 'react';
import { useNavigate, useParams } from 'react-router-dom';
import { Printer } from 'lucide-react';
import api, { errMsg } from '../api/client';
import PageHeader from '../components/PageHeader';
import { money } from '../lib/money';
import { COMPANY } from '../utils/invoicePdf';

/** D23: a customer's statement of account — print it, or "Save as PDF" from the print dialog. */

interface Line {
  date: string;
  type: 'INVOICE' | 'PAYMENT' | 'CREDIT_NOTE';
  number: string;
  reference: string | null;
  dueDate: string | null;
  charge: number;
  credit: number;
  balance: number;
}
interface Statement {
  customer: { name: string; email: string | null; phone: string | null; address: string | null };
  openingBalance: number;
  lines: Line[];
  totalCharges: number;
  totalCredits: number;
  closingBalance: number;
}

const LABEL = { INVOICE: 'Invoice', PAYMENT: 'Payment received', CREDIT_NOTE: 'Credit note' };
const isoDay = (d: Date) =>
  `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`;
const longDate = (d: string) => new Date(`${d}T00:00:00`).toLocaleDateString('en-GB', { day: 'numeric', month: 'long', year: 'numeric' });

export default function CustomerStatement() {
  const { id } = useParams();
  const navigate = useNavigate();
  const now = new Date();
  const [dateFrom, setDateFrom] = useState(`${now.getFullYear()}-01-01`);
  const [dateTo, setDateTo] = useState(isoDay(now));
  const [ran, setRan] = useState({ dateFrom, dateTo });
  const [data, setData] = useState<Statement | null>(null);
  const [error, setError] = useState('');

  useEffect(() => {
    api.get(`/customers/${id}/statement`, { params: ran })
      .then((r) => { setData(r.data); setError(''); })
      .catch((e) => setError(errMsg(e, 'Could not load the statement')));
  }, [id, ran]);

  const c = data?.customer;

  return (
    <div>
      <PageHeader
        title="Customer Statement"
        action={
          <div className="flex gap-2 items-center no-print">
            <input type="date" value={dateFrom} onChange={(e) => setDateFrom(e.target.value)} className="!w-40" aria-label="From" />
            <span className="text-gray-500 text-sm">to</span>
            <input type="date" value={dateTo} onChange={(e) => setDateTo(e.target.value)} className="!w-40" aria-label="To" />
            <button onClick={() => setRan({ dateFrom, dateTo })} className="btn btn-primary">Run</button>
            <button onClick={() => window.print()} className="btn btn-secondary" title="Print, or choose “Save as PDF” as the printer">
              <Printer size={14} /> Print / PDF
            </button>
            <button onClick={() => navigate(-1)} className="btn btn-secondary">Back</button>
          </div>
        }
      />
      {error && <div className="mb-3 px-3 py-2 bg-red-50 border border-red-200 text-red-700 text-sm rounded">{error}</div>}

      {data && c && (
        <div className="statement-sheet">
          <div className="flex items-start justify-between gap-6 mb-5">
            <div>
              <img src={COMPANY.logo} alt={COMPANY.name} className="h-9 w-auto mb-2" />
              <p className="text-xs text-gray-600 max-w-sm">{COMPANY.contactLine}</p>
              <p className="text-xs text-gray-600">{COMPANY.emailWebLine}</p>
            </div>
            <div className="text-right">
              <h2 className="text-xl font-bold tracking-wide">STATEMENT OF ACCOUNT</h2>
              <p className="text-sm text-gray-700 mt-1">{longDate(ran.dateFrom)} – {longDate(ran.dateTo)}</p>
              <p className="text-xs text-gray-500">Issued {longDate(isoDay(now))}</p>
            </div>
          </div>

          <div className="flex items-start justify-between gap-6 mb-4">
            <div className="text-sm">
              <p className="text-xs font-semibold text-gray-500 uppercase tracking-wide">To</p>
              <p className="font-semibold">{c.name}</p>
              {c.address && <p className="whitespace-pre-line text-gray-700">{c.address}</p>}
              {(c.email || c.phone) && <p className="text-gray-600 text-xs">{[c.email, c.phone].filter(Boolean).join(' · ')}</p>}
            </div>
            <div className="border border-[#d9d9d9] rounded px-4 py-2 text-right">
              <p className="text-xs text-gray-600">Amount due</p>
              <p className="text-lg font-bold tabular-nums" data-testid="amount-due">{money(data.closingBalance)}</p>
            </div>
          </div>

          <table className="report-table">
            <thead>
              <tr>
                <th>Date</th>
                <th>Details</th>
                <th>Number</th>
                <th>For</th>
                <th>Due</th>
                <th className="num">Charges</th>
                <th className="num">Payments &amp; credits</th>
                <th className="num">Balance</th>
              </tr>
            </thead>
            <tbody>
              <tr className="total">
                <td colSpan={7}>Balance brought forward</td>
                <td className="num">{money(data.openingBalance)}</td>
              </tr>
              {data.lines.map((l) => (
                <tr key={`${l.type}-${l.number}`}>
                  <td className="whitespace-nowrap">{l.date}</td>
                  <td>{LABEL[l.type]}</td>
                  <td className="whitespace-nowrap">{l.number}</td>
                  <td>{l.type === 'INVOICE' ? (l.reference ? `PO ${l.reference}` : '') : l.reference ?? ''}</td>
                  <td className="whitespace-nowrap">{l.dueDate ?? ''}</td>
                  <td className="num">{l.charge ? money(l.charge) : ''}</td>
                  <td className="num">{l.credit ? money(l.credit) : ''}</td>
                  <td className="num">{money(l.balance)}</td>
                </tr>
              ))}
              {data.lines.length === 0 && (
                <tr><td colSpan={8} className="empty">No invoices, payments or credits in this period.</td></tr>
              )}
              <tr className="total">
                <td colSpan={5}>Closing balance</td>
                <td className="num">{money(data.totalCharges)}</td>
                <td className="num">{money(data.totalCredits)}</td>
                <td className="num">{money(data.closingBalance)}</td>
              </tr>
            </tbody>
          </table>

          <div className="mt-5 text-xs text-gray-600 space-y-0.5">
            <p className="font-semibold text-gray-700">How to pay</p>
            {COMPANY.paymentOptions.map((p) => <p key={p}>{p}</p>)}
            <p className="pt-2 italic">This is a system-generated statement and no signature is required.</p>
          </div>
        </div>
      )}
    </div>
  );
}
