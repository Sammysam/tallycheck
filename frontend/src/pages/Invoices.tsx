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
  status: string;
  createdById?: string;
}

const emptyFilters = { q: '', status: '', dateFrom: '', dateTo: '' };

export default function Invoices() {
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
      alert(errMsg(err, 'Failed to load invoice'));
    }
  };

  /** Drafts only — the API refuses anything that has reached the ledger. */
  const remove = async (r: Invoice) => {
    if (!window.confirm(`Delete draft ${r.invoiceNumber}? This cannot be undone.`)) return;
    try {
      await api.delete(`/invoices/${r.id}`);
      load();
    } catch (err: any) {
      alert(errMsg(err, 'Failed to delete invoice'));
    }
  };

  const duplicate = async (id: string) => {
    try {
      const { data } = await api.post(`/invoices/${id}/duplicate`);
      setPage(1);
      await load(1);
      alert(`Copied to draft ${data.invoiceNumber}`);
    } catch (err: any) {
      alert(errMsg(err, 'Failed to duplicate invoice'));
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
    if (!window.confirm(warn)) return;
    try {
      await api.patch(`/invoices/${r.id}/status`, { status });
      load();
    } catch (err: any) {
      alert(errMsg(err, 'Failed to change status'));
    }
  };

  const action = async (id: string, ep: string) => {
    try { await api.post(`/invoices/${id}/${ep}`); load(); }
    catch (err: any) { alert(errMsg(err, 'Action failed')); }
  };

  const downloadPdf = async (id: string) => {
    try {
      const { data } = await api.get(`/invoices/${id}`);
      openInvoicePdf(data, settings);
    } catch (err: any) {
      alert(errMsg(err, 'Failed to load invoice'));
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
      alert(errMsg(err, 'Failed to load invoice'));
    }
  };

  const printPdf = async (id: string) => {
    const tab = openInvoiceTab();
    try {
      const { data } = await api.get(`/invoices/${id}`);
      printInvoice(data, settings, tab);
    } catch (err: any) {
      tab?.close();
      alert(errMsg(err, 'Failed to load invoice'));
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
      alert(errMsg(err, 'Failed to update PO No'));
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

  const fmt = (v: number) => new Intl.NumberFormat('en-LK', { style: 'currency', currency: 'LKR' }).format(Number(v));
  // What is still owed: total less payments and credit notes (D21).
  const balanceOf = (r: Invoice) => Number(r.totalAmount) - Number(r.amountPaid ?? 0) - Number(r.amountCredited ?? 0);
  const filtersActive = filters.q !== '' || filters.status !== '' || filters.dateFrom !== '' || filters.dateTo !== '';

  return (
    <div>
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
                  placeholder="e.g. Samir Husain"
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
            { header: '#',        accessor: 'invoiceNumber' },
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
            { header: 'Due',      accessor: (r) => r.dueDate ?? '—' },
            { header: 'Amount',   accessor: (r) => fmt(r.totalAmount) },
            { header: 'Paid',     accessor: (r) => (Number(r.amountPaid ?? 0) > 0 ? fmt(Number(r.amountPaid)) : '—') },
            { header: 'Balance',  accessor: (r) => (r.status === 'VOID' ? '—' : fmt(balanceOf(r))) },
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
                ...(r.status === 'DRAFT'  ? [{ label: 'Post',   onSelect: () => action(r.id, 'post') }] : []),
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
