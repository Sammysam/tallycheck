import { useEffect, useRef, useState } from 'react';
import api, { errMsg } from '../api/client';
import PageHeader from '../components/PageHeader';
import { useCommand } from '../lib/commands';
import Table from '../components/Table';
import StatusBadge from '../components/StatusBadge';
import Pagination from '../components/Pagination';
import ActionMenu from '../components/ActionMenu';
import { openInvoicePdf, openInvoiceTab, previewInvoice, printInvoice, type InvoiceSettings } from '../utils/invoicePdf';
import InvoiceSheetForm, { type SheetPayload } from '../components/InvoiceSheetForm';
import { useAuth } from '../contexts/AuthContext';
import { moneyIn } from '../lib/money';
import { useTaxCodes } from '../lib/taxes';
import TxnLink from '../components/TxnLink';
import { useNavigate } from 'react-router-dom';
import RecurringScheduleForm from '../components/RecurringScheduleForm';
import { nextAfter, type Schedule } from '../lib/recurring';
import { toast } from '../lib/toast';
import { fmtDate } from '../lib/region';
import { askConfirm } from '../lib/confirm';
import Dialog from '../components/desktop/Dialog';

interface Invoice {
  id: string;
  invoiceNumber: string;
  poNumber?: string;
  customer?: { name: string } | null;
  billToName?: string | null;
  date: string;
  dueDate: string;
  totalAmount: number;
  amountPaid?: number;
  amountCredited?: number;
  /** D30: null for the home currency. */
  currency?: string | null;
  status: string;
  createdById?: string;
}

const emptyFilters = { q: '', status: '', dateFrom: '', dateTo: '' };

export default function Invoices() {
  // Tax code names for printed invoices (D26).
  useTaxCodes();
  const { hasPermission, user } = useAuth();
  const [rows, setRows] = useState<Invoice[]>([]);
  const [page, setPage] = useState(1);
  const [totalPages, setTotalPages] = useState(1);
  const [total, setTotal] = useState(0);
  const limit = 10;

  const [customers, setCustomers] = useState<any[]>([]);
  const [accounts, setAccounts] = useState<any[]>([]);
  const [showForm, setShowForm] = useState(false);
  /** The DRAFT being edited, or null when raising a new invoice. */
  const [editDraft, setEditDraft] = useState<any | null>(null);
  const [error, setError] = useState('');
  const [settings, setSettings] = useState<InvoiceSettings>({});
  // D61: the invoice being made recurring, and drafts made by recurring schedules still to post.
  const [recurringFrom, setRecurringFrom] = useState<Invoice | null>(null);
  const [waiting, setWaiting] = useState(0);
  const navigate = useNavigate();
  // D75: an invoice refused for short stock, and the product it was refused for.
  const [stockProblem, setStockProblem] = useState<{ invoiceId: string; item: string; message: string } | null>(null);
  const [fixing, setFixing] = useState(false);

  // List filters — all optional, combined with AND by the API.
  const [filters, setFilters] = useState(emptyFilters);
  const firstRun = useRef(true);

  // Inline PO No editing
  const [editingPo, setEditingPo] = useState<string | null>(null);
  const [poDraft, setPoDraft] = useState('');
  const cancelledRef = useRef(false);

  // Invoice signatory (company-wide — applies to every invoice PDF)
  const [showSignatory, setShowSignatory] = useState(false);
  const [sigForm, setSigForm] = useState({ signatoryName: '', signatoryTitle: '' });
  const [sigSaving, setSigSaving] = useState(false);
  const [sigSaved, setSigSaved] = useState(false);
  const [sigError, setSigError] = useState('');

  const load = (p = page, f = filters) =>
    api
      .get('/invoices', {
        params: {
          page: p,
          limit,
          ...(f.q.trim() ? { q: f.q.trim() } : {}),
          ...(f.status ? { status: f.status } : {}),
          ...(f.dateFrom ? { dateFrom: f.dateFrom } : {}),
          ...(f.dateTo ? { dateTo: f.dateTo } : {}),
        },
      })
      .then((r) => {
        setRows(r.data.data);
        setTotal(r.data.total);
        setTotalPages(r.data.totalPages);
        api.get('/recurring-invoices/waiting').then((w) => setWaiting(w.data.drafts)).catch(() => setWaiting(0));
      });

  useEffect(() => { load(); }, [page]);

  // Typing in the filters re-queries from page 1, once the typing settles.
  useEffect(() => {
    if (firstRun.current) { firstRun.current = false; return; }
    const t = setTimeout(() => { setPage(1); load(1, filters); }, 300);
    return () => clearTimeout(t);
  }, [filters]);

  useEffect(() => {
    api.get('/customers').then((r) => setCustomers(r.data));
    api.get('/accounts').then((r) => setAccounts(r.data.filter((a: any) => a.type === 'REVENUE')));
    api.get('/settings')
      .then((r) => {
        setSettings(r.data);
        setSigForm({
          signatoryName: r.data.signatoryName ?? '',
          signatoryTitle: r.data.signatoryTitle ?? '',
        });
      })
      .catch(() => {});
  }, []);

  const closeForm = () => { setShowForm(false); setEditDraft(null); };
  // Ctrl+N / Esc and the toolbar, via the desktop shell.
  useCommand('new', () => { setEditDraft(null); setShowForm(true); setShowSignatory(false); });
  useCommand('close', closeForm);

  // Saves the sheet form. Throws back to the form so it can show the message.
  const saveInvoice = async (payload: SheetPayload) => {
    setError('');
    if (editDraft) {
      await api.patch(`/invoices/${editDraft.id}`, payload);
      closeForm();
      load();
      return;
    }
    await api.post('/invoices', payload);
    closeForm();
    setPage(1);
    load(1);
  };

  /** Opens the sheet on an existing DRAFT — needs the full record, lines included. */
  const openEdit = async (id: string) => {
    try {
      const { data } = await api.get(`/invoices/${id}`);
      setEditDraft(data);
      setShowSignatory(false);
      setShowForm(true);
    } catch (err: any) {
      toast(errMsg(err, 'Failed to load invoice'), 'error');
    }
  };

  /** Drafts only — the API refuses anything that has reached the ledger. */
  const remove = async (r: Invoice) => {
    if (!(await askConfirm(`Delete draft ${r.invoiceNumber}? This cannot be undone.`))) return;
    try {
      await api.delete(`/invoices/${r.id}`);
      load();
    } catch (err: any) {
      toast(errMsg(err, 'Failed to delete invoice'), 'error');
    }
  };

  const duplicate = async (id: string) => {
    try {
      const { data } = await api.post(`/invoices/${id}/duplicate`);
      setPage(1);
      await load(1);
      toast(`Copied to draft ${data.invoiceNumber}`);
    } catch (err: any) {
      toast(errMsg(err, 'Failed to duplicate invoice'), 'error');
    }
  };

  /**
   * Statuses a row can be moved to by hand. PAID is absent on purpose — it is
   * set by recording a payment, so the ledger and the label never disagree.
   */
  const statusOptions = (status: string) => {
    if (status === 'DRAFT')  return ['DRAFT', 'POSTED', 'VOID'];
    if (status === 'POSTED') return ['POSTED', 'DRAFT', 'VOID'];
    return [];
  };

  const changeStatus = async (r: Invoice, status: string) => {
    if (status === r.status) return;
    const warn =
      status === 'VOID'  ? `Void ${r.invoiceNumber}? Its ledger entry is reversed.`
      : status === 'DRAFT' ? `Return ${r.invoiceNumber} to draft? Its ledger entry is reversed so you can edit it.`
      : `Post ${r.invoiceNumber}? This writes it to the ledger.`;
    if (!(await askConfirm(warn))) return;
    try {
      await api.patch(`/invoices/${r.id}/status`, { status });
      toast(status === 'POSTED' ? `${r.invoiceNumber} posted to the books` : status === 'VOID' ? `${r.invoiceNumber} voided` : `${r.invoiceNumber} is a draft again`);
      load();
    } catch (err: any) {
      toast(errMsg(err, 'Failed to change status'), 'error');
    }
  };

  const makeRecurring = async (s: Schedule) => {
    if (!recurringFrom) return;
    const { data } = await api.post('/recurring-invoices', { invoiceId: recurringFrom.id, ...s, endDate: s.endDate || undefined });
    setRecurringFrom(null);
    toast(data.invoicesMade > 0
      ? `${data.name} is recurring — ${data.invoicesMade} draft invoice(s) made for dates that have come`
      : `${data.name} is recurring — first invoice ${fmtDate(data.nextDate)}`);
    load();
  };

  const action = async (id: string, ep: string) => {
    try {
      await api.post(`/invoices/${id}/${ep}`);
      toast(ep === 'post' ? 'Invoice posted to the books' : ep === 'void' ? 'Invoice voided' : 'Done');
      load();
    }
    catch (err: any) {
      const message = errMsg(err, 'Action failed');
      // D75: a stock refusal gets the app's own box, with a one-click way out.
      const short = ep === 'post' ? /^Not enough stock of (.+?): /.exec(message) : null;
      if (short) setStockProblem({ invoiceId: id, item: short[1], message });
      else toast(message, 'error');
    }
  };

  /** D75: stops counting the product the post was refused for, then posts again. */
  const stopCounting = async () => {
    if (!stockProblem) return;
    setFixing(true);
    try {
      const { data: list } = await api.get('/items');
      const item = (list as { id: string; name: string; type: string }[]).find((i) => i.name === stockProblem.item && i.type === 'INVENTORY');
      if (!item) throw new Error(`Could not find ${stockProblem.item} under Products & Services`);
      await api.patch(`/items/${item.id}`, { type: 'NON_INVENTORY' });
      await api.post(`/invoices/${stockProblem.invoiceId}/post`);
      toast(`${stockProblem.item} is no longer counted. Invoice posted to the books`);
      setStockProblem(null);
      load();
    } catch (err) {
      setStockProblem((p) => (p ? { ...p, message: errMsg(err, err instanceof Error ? err.message : 'Could not change the product') } : p));
    } finally {
      setFixing(false);
    }
  };

  const downloadPdf = async (id: string) => {
    try {
      const { data } = await api.get(`/invoices/${id}`);
      openInvoicePdf(data, settings);
    } catch (err: any) {
      toast(errMsg(err, 'Failed to load invoice'), 'error');
    }
  };

  // The tab is opened up front, while the click is still the browser's idea of
  // a user gesture — opening it after the fetch gets caught by the pop-up blocker.
  const previewPdf = async (id: string) => {
    const tab = openInvoiceTab();
    try {
      const { data } = await api.get(`/invoices/${id}`);
      previewInvoice(data, settings, tab);
    } catch (err: any) {
      tab?.close();
      toast(errMsg(err, 'Failed to load invoice'), 'error');
    }
  };

  const printPdf = async (id: string) => {
    const tab = openInvoiceTab();
    try {
      const { data } = await api.get(`/invoices/${id}`);
      printInvoice(data, settings, tab);
    } catch (err: any) {
      tab?.close();
      toast(errMsg(err, 'Failed to load invoice'), 'error');
    }
  };

  const startPoEdit = (r: Invoice) => {
    cancelledRef.current = false;
    setPoDraft(r.poNumber ?? '');
    setEditingPo(r.id);
  };

  // Called on blur (Enter blurs the field, Escape cancels first).
  const commitPo = async (r: Invoice) => {
    setEditingPo(null);
    if (cancelledRef.current) { cancelledRef.current = false; return; }
    const next = poDraft.trim();
    if (next === (r.poNumber ?? '')) return;
    try {
      await api.patch(`/invoices/${r.id}`, { poNumber: next });
      load();
    } catch (err: any) {
      toast(errMsg(err, 'Failed to update PO No'), 'error');
    }
  };

  const saveSignatory = async (e: React.FormEvent) => {
    e.preventDefault();
    setSigError('');
    setSigSaved(false);
    setSigSaving(true);
    try {
      const { data } = await api.patch('/settings', sigForm);
      // Update the in-memory copy so the very next Preview/PDF uses it — no reload.
      setSettings(data);
      setSigSaved(true);
    } catch (err: any) {
      setSigError(errMsg(err, 'Failed to save signatory'));
    } finally {
      setSigSaving(false);
    }
  };

  // What is still owed: total less payments and credit notes (D21).
  const balanceOf = (r: Invoice) => Number(r.totalAmount) - Number(r.amountPaid ?? 0) - Number(r.amountCredited ?? 0);
  const filtersActive = filters.q !== '' || filters.status !== '' || filters.dateFrom !== '' || filters.dateTo !== '';

  return (
    <div>
      {stockProblem && (
        <Dialog title="Not enough stock to post this invoice" width={520} onClose={() => !fixing && setStockProblem(null)}
          buttons={<>
            <button className="btn btn-primary" disabled={fixing} onClick={stopCounting}>{fixing ? 'Posting…' : 'Stop counting it and post'}</button>
            <button className="btn btn-secondary" disabled={fixing} onClick={() => navigate('/items')}>Open Products &amp; Services</button>
            <button className="btn btn-secondary" disabled={fixing} onClick={() => setStockProblem(null)}>Close</button>
          </>}>
          <p className="text-sm text-gray-800">{stockProblem.message}</p>
          <p className="text-sm text-gray-600 mt-3">TallyCheck is counting how many <strong>{stockProblem.item}</strong> you have, and the count is lower than this invoice. If you don’t need TallyCheck to count it (for example you make it yourself), stop counting it and the invoice is posted. If you do, record the bill that brought it in, or adjust the stock, then post again.</p>
        </Dialog>
      )}
      <PageHeader
        title={showForm ? (editDraft ? `Edit ${editDraft.invoiceNumber}` : 'New Invoice') : 'Invoices'}
        action={
          showForm ? (
            <button onClick={closeForm} className="btn btn-secondary">← Back to invoices</button>
          ) : (
            <div className="flex gap-2">
              {hasPermission('settings') && (
                <button onClick={() => { setShowSignatory(!showSignatory); setSigSaved(false); }} className="btn btn-secondary">
                  Signatory
                </button>
              )}
              <button onClick={() => { setEditDraft(null); setShowForm(true); setShowSignatory(false); }} className="btn btn-primary">
                + New Invoice
              </button>
            </div>
          )
        }
      />

      {error && <div className="mb-4 px-4 py-2.5 bg-red-50 border border-red-200 text-red-700 text-sm rounded-lg">{error}</div>}

      {!showForm && waiting > 0 && (
        <div className="mb-3 px-4 py-2.5 bg-amber-50 border border-amber-200 text-amber-800 text-sm rounded-lg flex items-center gap-3">
          <span>{waiting} invoice{waiting === 1 ? '' : 's'} made by recurring schedules {waiting === 1 ? 'is' : 'are'} waiting as draft{waiting === 1 ? '' : 's'} — check and post {waiting === 1 ? 'it' : 'them'}.</span>
          <button className="btn-link" onClick={() => setFilters({ ...emptyFilters, status: 'DRAFT' })}>Show drafts</button>
          <button className="btn-link" onClick={() => navigate('/recurring-invoices')}>Recurring invoices</button>
        </div>
      )}

      {!showForm && recurringFrom && (
        <RecurringScheduleForm
          key={recurringFrom.id}
          title={`Make ${recurringFrom.invoiceNumber} recurring`}
          note="The invoice is copied as it is now (customer, lines, prices, tax). Later changes to it don't change the schedule."
          submitLabel="Make recurring"
          initial={{
            name: recurringFrom.customer?.name || recurringFrom.billToName || recurringFrom.invoiceNumber,
            frequency: 'MONTHLY', every: 1, startDate: nextAfter(recurringFrom.date, 'MONTHLY'), endDate: '',
          }}
          onSubmit={makeRecurring}
          onCancel={() => setRecurringFrom(null)}
        />
      )}

      {!showForm && !hasPermission('invoices:all') && (
        <p className="mb-3 text-xs text-gray-500">Showing only the invoices you created.</p>
      )}

      {showSignatory && !showForm && hasPermission('settings') && (
        <div className="card mb-5" style={{ maxWidth: 520 }}>
          <h3 className="text-sm font-semibold text-gray-700 mb-1">Invoice signatory</h3>
          <p className="text-xs text-gray-500 mb-3">
            The default signatory for new invoices. Each invoice keeps whoever signed it.
          </p>
          <form onSubmit={saveSignatory}>
            <div className="form-grid mb-3" style={{ gridTemplateColumns: '1fr 1fr' }}>
              <label className="text-xs font-medium text-gray-600">
                Signatory name
                <input
                  required
                  className="mt-1"
                  placeholder="e.g. Jane Smith"
                  value={sigForm.signatoryName}
                  onChange={(e) => { setSigSaved(false); setSigForm({ ...sigForm, signatoryName: e.target.value }); }}
                />
              </label>
              <label className="text-xs font-medium text-gray-600">
                Signatory title
                <input
                  required
                  className="mt-1"
                  placeholder="e.g. Director"
                  value={sigForm.signatoryTitle}
                  onChange={(e) => { setSigSaved(false); setSigForm({ ...sigForm, signatoryTitle: e.target.value }); }}
                />
              </label>
            </div>
            {sigError && <p className="text-sm text-red-600 mb-2">{sigError}</p>}
            {sigSaved && <p className="text-sm text-green-600 mb-2">Saved — new invoices will start with it.</p>}
            <div className="flex gap-2">
              <button type="submit" className="btn btn-primary" disabled={sigSaving}>
                {sigSaving ? 'Saving…' : 'Save'}
              </button>
              <button type="button" onClick={() => setShowSignatory(false)} className="btn btn-secondary">Close</button>
            </div>
          </form>
        </div>
      )}

      {showForm ? (
        <InvoiceSheetForm
          customers={customers}
          accounts={accounts}
          settings={settings}
          initial={editDraft ?? undefined}
          onSubmit={saveInvoice}
          onCancel={closeForm}
        />
      ) : (
        <>
          <div className="card mb-4">
            <div className="form-grid items-end" style={{ gridTemplateColumns: '2fr 1fr 1fr 1fr auto' }}>
              <label className="text-xs font-medium text-gray-600">
                Search
                <input
                  className="mt-1"
                  placeholder="Invoice no, PO no or customer"
                  value={filters.q}
                  onChange={(e) => setFilters({ ...filters, q: e.target.value })}
                />
              </label>
              <label className="text-xs font-medium text-gray-600">
                Status
                <select className="mt-1" value={filters.status} onChange={(e) => setFilters({ ...filters, status: e.target.value })}>
                  <option value="">All</option>
                  <option value="DRAFT">Draft</option>
                  <option value="POSTED">Posted</option>
                  <option value="PAID">Paid</option>
                  <option value="VOID">Void</option>
                </select>
              </label>
              <label className="text-xs font-medium text-gray-600">
                From
                <input type="date" className="mt-1" value={filters.dateFrom} onChange={(e) => setFilters({ ...filters, dateFrom: e.target.value })} />
              </label>
              <label className="text-xs font-medium text-gray-600">
                To
                <input type="date" className="mt-1" value={filters.dateTo} onChange={(e) => setFilters({ ...filters, dateTo: e.target.value })} />
              </label>
              <button
                type="button"
                className="btn btn-secondary"
                disabled={!filtersActive}
                onClick={() => setFilters(emptyFilters)}
              >
                Clear
              </button>
            </div>
          </div>

          <Table data={rows} columns={[
            { header: '#',        accessor: (r) => <TxnLink entryId={(r as { journalEntryId?: string }).journalEntryId}>{r.invoiceNumber}</TxnLink> },
            { header: 'PO No',    accessor: (r) => {
              if (r.status === 'VOID') return r.poNumber || '—';
              if (editingPo === r.id) {
                return (
                  <input
                    autoFocus
                    value={poDraft}
                    placeholder="PO No"
                    style={{ width: 110, height: '1.75rem' }}
                    onChange={(e) => setPoDraft(e.target.value)}
                    onBlur={() => commitPo(r)}
                    onKeyDown={(e) => {
                      if (e.key === 'Enter') e.currentTarget.blur();
                      if (e.key === 'Escape') { cancelledRef.current = true; e.currentTarget.blur(); }
                    }}
                  />
                );
              }
              return (
                <button onClick={() => startPoEdit(r)} className="btn-link" title="Click to edit PO No">
                  {r.poNumber || 'add'}
                </button>
              );
            }},
            { header: 'Customer', accessor: (r) => r.customer?.name || r.billToName || '—' },
            { header: 'Date',     accessor: 'date' },
            { header: 'Due',      accessor: (r) => (r.dueDate ? fmtDate(r.dueDate) : '—') },
            { header: 'Amount',   accessor: (r) => moneyIn(Number(r.totalAmount), r.currency) },
            { header: 'Paid',     accessor: (r) => (Number(r.amountPaid ?? 0) > 0 ? moneyIn(Number(r.amountPaid), r.currency) : '—') },
            { header: 'Balance',  accessor: (r) => (r.status === 'VOID' ? '—' : moneyIn(balanceOf(r), r.currency)) },
            { header: 'Status',   accessor: (r) => {
              const options = statusOptions(r.status);
              if (!options.length) return <StatusBadge status={r.status} />;
              return (
                <select
                  value={r.status}
                  onChange={(e) => changeStatus(r, e.target.value)}
                  style={{ width: 104, height: '1.75rem', fontSize: '0.75rem' }}
                  title="Change the status — the ledger entry follows"
                >
                  {options.map((o) => <option key={o} value={o}>{o}</option>)}
                </select>
              );
            }},
            { header: '', accessor: (r) => (
              <ActionMenu items={[
                ...(r.status === 'DRAFT' && (r.createdById === user?.id || user?.role === 'Admin') ? [{ label: 'Edit',     onSelect: () => openEdit(r.id) }] : []),
                { label: 'Preview',  onSelect: () => previewPdf(r.id) },
                { label: 'Print',    onSelect: () => printPdf(r.id) },
                { label: 'Download', onSelect: () => downloadPdf(r.id) },
                { label: 'Copy',     onSelect: () => duplicate(r.id) },
                ...(r.status !== 'VOID' ? [{ label: 'Make recurring', onSelect: () => { setRecurringFrom(r); window.scrollTo(0, 0); } }] : []),
                ...(r.status === 'DRAFT'  ? [{ label: 'Post',   onSelect: () => action(r.id, 'post') }] : []),
                ...(r.status === 'POSTED' ? [{ label: 'Receive payment', onSelect: () => navigate(`/payments?invoice=${r.id}`) }] : []),
                ...(r.status === 'POSTED' ? [{ label: 'Void',   onSelect: () => action(r.id, 'void'), danger: true }] : []),
                ...(r.status === 'DRAFT' && user?.role === 'Admin' ? [{ label: 'Delete', onSelect: () => remove(r), danger: true }] : []),
              ]} />
            )},
          ]} />
          <Pagination page={page} totalPages={totalPages} total={total} limit={limit} onPage={(p) => { setPage(p); load(p); }} />
        </>
      )}
    </div>
  );
}
