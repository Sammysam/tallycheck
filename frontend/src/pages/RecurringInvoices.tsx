import { useEffect, useState } from 'react';
import { useNavigate } from 'react-router-dom';
import api, { errMsg } from '../api/client';
import PageHeader from '../components/PageHeader';
import Table from '../components/Table';
import StatusBadge from '../components/StatusBadge';
import ActionMenu from '../components/ActionMenu';
import RecurringScheduleForm from '../components/RecurringScheduleForm';
import { repeatsLabel, type Schedule } from '../lib/recurring';
import { moneyIn } from '../lib/money';
import { fmtDate } from '../lib/region';
import { askConfirm } from '../lib/confirm';
import { toast } from '../lib/toast';

/**
 * D61: recurring invoice schedules. Made from an invoice ("Make recurring" on the Invoices
 * screen); each due date becomes a DRAFT invoice. Pause, resume, change or stop them here.
 */

interface Recurring {
  id: string;
  name: string;
  customerName?: string | null;
  currency?: string | null;
  amount: string;
  hasTax: boolean;
  frequency: Schedule['frequency'];
  every: number;
  startDate: string;
  endDate?: string | null;
  nextDate?: string | null;
  status: 'ACTIVE' | 'PAUSED' | 'ENDED';
  lastError?: string | null;
  invoicesMade: number;
  draftsWaiting: number;
}

export default function RecurringInvoices() {
  const navigate = useNavigate();
  const [rows, setRows] = useState<Recurring[]>([]);
  const [editing, setEditing] = useState<Recurring | null>(null);

  const load = () =>
    api.get('/recurring-invoices').then((r) => setRows(r.data)).catch((err) => toast(errMsg(err, 'Could not load the schedules'), 'error'));
  useEffect(() => { load(); }, []);

  const act = async (r: Recurring, what: 'pause' | 'resume') => {
    try {
      const { data } = await api.post(`/recurring-invoices/${r.id}/${what}`);
      toast(what === 'pause' ? `${r.name} paused` : `${r.name} resumed${data.nextDate ? ` — next invoice ${fmtDate(data.nextDate)}` : ''}`);
      load();
    } catch (err) {
      toast(errMsg(err, 'Could not change the schedule'), 'error');
    }
  };

  const stop = async (r: Recurring) => {
    if (!(await askConfirm(`Stop "${r.name}"? No more invoices will be made. The ${r.invoicesMade} invoice(s) it made stay as they are.`))) return;
    try {
      await api.delete(`/recurring-invoices/${r.id}`);
      toast(`${r.name} stopped`);
      load();
    } catch (err) {
      toast(errMsg(err, 'Could not stop the schedule'), 'error');
    }
  };

  const save = async (s: Schedule) => {
    if (!editing) return;
    await api.patch(`/recurring-invoices/${editing.id}`, { ...s, endDate: s.endDate || undefined });
    toast(`${s.name || editing.name} saved`);
    setEditing(null);
    load();
  };

  return (
    <div>
      <PageHeader
        title="Recurring Invoices"
        action={<button className="btn btn-secondary" onClick={() => navigate('/invoices')}>Go to invoices</button>}
      />
      <p className="mb-3 text-xs text-gray-500">
        To add one, open Invoices and choose <strong>Make recurring</strong> on an invoice. Each date makes a draft invoice to check and post.
      </p>

      {editing && (
        <RecurringScheduleForm
          key={editing.id}
          title={`Change "${editing.name}"`}
          note="Dates already made are not made again. The invoice itself (customer, lines, prices) stays as it was copied."
          submitLabel="Save schedule"
          initial={{ name: editing.name, frequency: editing.frequency, every: editing.every, startDate: editing.startDate, endDate: editing.endDate ?? '' }}
          onSubmit={save}
          onCancel={() => setEditing(null)}
        />
      )}

      <Table data={rows} columns={[
        { header: 'Name', accessor: (r) => (
          <span>
            {r.name}
            {r.lastError && <span className="block text-xs text-red-600" title={r.lastError}>Could not make the last invoice: {r.lastError}</span>}
          </span>
        ) },
        { header: 'Customer', accessor: (r) => r.customerName || '—' },
        { header: 'Amount', accessor: (r) => `${moneyIn(Number(r.amount), r.currency)}${r.hasTax ? ' + tax' : ''}` },
        { header: 'Repeats', accessor: (r) => repeatsLabel(r.frequency, r.every) },
        { header: 'Next invoice', accessor: (r) => (r.status === 'ACTIVE' && r.nextDate ? fmtDate(r.nextDate) : '—') },
        { header: 'Ends', accessor: (r) => (r.endDate ? fmtDate(r.endDate) : 'Never') },
        { header: 'Made', accessor: (r) => (
          <span>{r.invoicesMade}{r.draftsWaiting > 0 && <span className="text-xs text-amber-700"> ({r.draftsWaiting} to post)</span>}</span>
        ) },
        { header: 'Status', accessor: (r) => <StatusBadge status={r.status} /> },
        { header: '', accessor: (r) => (
          <ActionMenu items={[
            { label: 'Change schedule', onSelect: () => setEditing(r) },
            ...(r.status === 'ACTIVE' ? [{ label: 'Pause', onSelect: () => act(r, 'pause') }] : []),
            ...(r.status === 'PAUSED' ? [{ label: 'Resume', onSelect: () => act(r, 'resume') }] : []),
            { label: 'Stop', onSelect: () => stop(r), danger: true },
          ]} />
        ) },
      ]} />
    </div>
  );
}
