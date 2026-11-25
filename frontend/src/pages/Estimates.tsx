import { useEffect, useRef, useState } from 'react';
import { useNavigate } from 'react-router-dom';
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
import { toast } from '../lib/toast';
import { fmtDate } from '../lib/region';

/**
 * D60: estimates / quotes. Edited on the same sheet as an invoice (lines, discounts, tax,
 * currency), printed as "Estimate", never posted. Draft → Sent → Accepted or Declined by hand;
 * "Turn into invoice" makes a DRAFT invoice once and links the two.
 */

interface Estimate {
  id: string;
  estimateNumber: string;
  customer?: { name: string } | null;
  billToName?: string | null;
  date: string;
  expiryDate?: string | null;
  totalAmount: number | string;
  currency?: string | null;
  status: string;
  invoiceNumber?: string | null;
  createdById?: string;
}

const STATUS_LABEL: Record<string, string> = {
  DRAFT: 'Draft', SENT: 'Sent', ACCEPTED: 'Accepted', DECLINED: 'Declined', CONVERTED: 'Invoiced',
};
const SETTABLE = ['DRAFT', 'SENT', 'ACCEPTED', 'DECLINED'];
type EstimateDoc = Parameters<typeof previewInvoice>[0];
type Row = Record<string, unknown> & { id: string; name?: string; type?: string };
const emptyFilters = { q: '', status: '' };
const today = () => new Date().toISOString().slice(0, 10);

export default function Estimates() {
  useTaxCodes();
  const { user } = useAuth();
  const navigate = useNavigate();
  const [rows, setRows] = useState<Estimate[]>([]);
  const [page, setPage] = useState(1);
  const [totalPages, setTotalPages] = useState(1);
  const [total, setTotal] = useState(0);
  const limit = 10;
  const [customers, setCustomers] = useState<Row[]>([]);
  const [accounts, setAccounts] = useState<Row[]>([]);
  const [settings, setSettings] = useState<InvoiceSettings>({});
  const [showForm, setShowForm] = useState(false);
  const [editing, setEditing] = useState<(Row & { estimateNumber: string }) | null>(null);
  const [filters, setFilters] = useState(emptyFilters);
  const firstRun = useRef(true);

  const load = (p = page, f = filters) =>
    api
      .get('/estimates', {
        params: { page: p, limit, ...(f.q.trim() ? { q: f.q.trim() } : {}), ...(f.status ? { status: f.status } : {}) },
      })
      .then((r) => {
        setRows(r.data.data);
        setTotal(r.data.total);
        setTotalPages(r.data.totalPages);
      })
      .catch((err) => toast(errMsg(err, 'Could not load the estimates'), 'error'));

  // eslint-disable-next-line react-hooks/exhaustive-deps
  useEffect(() => { load(); }, [page]);
  useEffect(() => {
    if (firstRun.current) { firstRun.current = false; return; }
    const t = setTimeout(() => { setPage(1); load(1, filters); }, 300);
    return () => clearTimeout(t);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [filters]);
  useEffect(() => {
    api.get('/customers').then((r) => setCustomers(r.data)).catch(() => setCustomers([]));
    api.get('/accounts').then((r) => setAccounts(r.data.filter((a: Row) => a.type === 'REVENUE'))).catch(() => setAccounts([]));
    api.get('/settings').then((r) => setSettings(r.data)).catch(() => {});
  }, []);

  const closeForm = () => { setShowForm(false); setEditing(null); };
  const openNew = () => { setEditing(null); setShowForm(true); };
  useCommand('new', openNew);
  useCommand('close', closeForm);

  const save = async (payload: SheetPayload) => {
    if (editing) {
      await api.put(`/estimates/${editing.id}`, payload);
      toast(`${editing.estimateNumber} saved`);
      closeForm();
      load();
      return;
    }
    const { data } = await api.post('/estimates', payload);
    toast(`${data.estimateNumber} saved`);
    closeForm();
    setPage(1);
    load(1);
  };

  const openEdit = async (id: string) => {
    try {
      const { data } = await api.get(`/estimates/${id}`);
      setEditing(data);
      setShowForm(true);
    } catch (err) {
      toast(errMsg(err, 'Could not open the estimate'), 'error');
    }
  };

  const changeStatus = async (r: Estimate, status: string) => {
    if (status === r.status) return;
    try {
      await api.patch(`/estimates/${r.id}/status`, { status });
      toast(`${r.estimateNumber} marked ${STATUS_LABEL[status].toLowerCase()}`);
      load();
    } catch (err) {
      toast(errMsg(err, 'Could not change the status'), 'error');
    }
  };

  const convert = async (r: Estimate) => {
    if (!window.confirm(`Turn ${r.estimateNumber} into a draft invoice dated today? You can check it before posting.`)) return;
    try {
      const { data } = await api.post(`/estimates/${r.id}/convert`);
      toast(`${r.estimateNumber} is now draft invoice ${data.invoice.invoiceNumber}`);
      load();
    } catch (err) {
      toast(errMsg(err, 'Could not turn the estimate into an invoice'), 'error');
    }
  };

  const remove = async (r: Estimate) => {
    if (!window.confirm(`Delete ${r.estimateNumber}? This cannot be undone.`)) return;
    try {
      await api.delete(`/estimates/${r.id}`);
      toast(`${r.estimateNumber} deleted`);
      load();
    } catch (err) {
      toast(errMsg(err, 'Could not delete the estimate'), 'error');
    }
  };

  const withEstimate = async (id: string, then: (data: EstimateDoc) => void, tab?: Window | null) => {
    try {
      const { data } = await api.get(`/estimates/${id}`);
      then({ ...data, docType: 'estimate' });
    } catch (err) {
      tab?.close();
      toast(errMsg(err, 'Could not open the estimate'), 'error');
    }
  };
  // The tab opens on the click itself so pop-up blockers allow it.
  const preview = (id: string) => { const tab = openInvoiceTab(); withEstimate(id, (d) => previewInvoice(d, settings, tab), tab); };
  const print = (id: string) => { const tab = openInvoiceTab(); withEstimate(id, (d) => printInvoice(d, settings, tab), tab); };
  const download = (id: string) => withEstimate(id, (d) => openInvoicePdf(d, settings));

  const mine = (r: Estimate) => r.createdById === user?.id || user?.role === 'Admin';
  const expired = (r: Estimate) => Boolean(r.expiryDate) && (r.status === 'DRAFT' || r.status === 'SENT') && String(r.expiryDate) < today();

  return (
    <div>
      <PageHeader
        title={showForm ? (editing ? `Edit ${editing.estimateNumber}` : 'New Estimate') : 'Estimates'}
        action={showForm
          ? <button onClick={closeForm} className="btn btn-secondary">← Back to estimates</button>
          : <button onClick={openNew} className="btn btn-primary">+ New Estimate</button>}
      />

      {showForm ? (
        <InvoiceSheetForm
          kind="estimate"
          customers={customers}
          accounts={accounts}
          settings={settings}
          initial={editing ?? undefined}
          onSubmit={save}
          onCancel={closeForm}
        />
      ) : (
        <>
          <p className="mb-3 text-xs text-gray-500">
            Quotes for customers. Nothing here reaches the ledger until an accepted estimate is turned into an invoice and posted.
          </p>
          <div className="card mb-4">
            <div className="form-grid items-end" style={{ gridTemplateColumns: '2fr 1fr auto' }}>
              <label className="text-xs font-medium text-gray-600">
                Search
                <input className="mt-1" placeholder="Estimate no or customer" value={filters.q}
                  onChange={(e) => setFilters({ ...filters, q: e.target.value })} />
              </label>
              <label className="text-xs font-medium text-gray-600">
                Status
                <select className="mt-1" value={filters.status} onChange={(e) => setFilters({ ...filters, status: e.target.value })}>
                  <option value="">All</option>
                  {Object.entries(STATUS_LABEL).map(([v, l]) => <option key={v} value={v}>{l}</option>)}
                </select>
              </label>
              <button type="button" className="btn btn-secondary" disabled={!filters.q && !filters.status} onClick={() => setFilters(emptyFilters)}>
                Clear
              </button>
            </div>
          </div>

          <Table data={rows} columns={[
            { header: '#', accessor: 'estimateNumber' },
            { header: 'Customer', accessor: (r) => r.customer?.name || r.billToName || '—' },
            { header: 'Date', accessor: 'date' },
            { header: 'Valid until', accessor: (r) => r.expiryDate
              ? <span className={expired(r) ? 'text-amber-700' : undefined} title={expired(r) ? 'Expired' : undefined}>{fmtDate(r.expiryDate)}{expired(r) ? ' (expired)' : ''}</span>
              : '—' },
            { header: 'Amount', accessor: (r) => moneyIn(Number(r.totalAmount), r.currency) },
            { header: 'Status', accessor: (r) => (r.status === 'CONVERTED' || !mine(r)
              ? <StatusBadge status={r.status} />
              : (
                <select value={r.status} onChange={(e) => changeStatus(r, e.target.value)} aria-label={`Status of ${r.estimateNumber}`}
                  style={{ width: 110, height: '1.75rem', fontSize: '0.75rem' }}>
                  {SETTABLE.map((s) => <option key={s} value={s}>{STATUS_LABEL[s]}</option>)}
                </select>
              )) },
            { header: 'Invoice', accessor: (r) => (r.invoiceNumber
              ? <button className="btn-link" onClick={() => navigate('/invoices')} title="Open the invoices list">{r.invoiceNumber}</button>
              : '—') },
            { header: '', accessor: (r) => (
              <ActionMenu items={[
                ...((r.status === 'DRAFT' || r.status === 'SENT') && mine(r) ? [{ label: 'Edit', onSelect: () => openEdit(r.id) }] : []),
                { label: 'Preview', onSelect: () => preview(r.id) },
                { label: 'Print', onSelect: () => print(r.id) },
                { label: 'Download', onSelect: () => download(r.id) },
                ...(r.status !== 'CONVERTED' && r.status !== 'DECLINED' ? [{ label: 'Turn into invoice', onSelect: () => convert(r) }] : []),
                ...(r.status !== 'CONVERTED' && mine(r) ? [{ label: 'Delete', onSelect: () => remove(r), danger: true }] : []),
              ]} />
            ) },
          ]} />
          <Pagination page={page} totalPages={totalPages} total={total} limit={limit} onPage={(p) => { setPage(p); load(p); }} />
        </>
      )}
    </div>
  );
}
