import { useState } from 'react';
import { errMsg } from '../api/client';
import { UNIT, nextAfter, repeatsLabel, type Schedule } from '../lib/recurring';
import { fmtDate } from '../lib/region';

/**
 * D61: the schedule of a recurring invoice — how often, from when, until when. Used by
 * "Make recurring" on an invoice and by Edit on the Recurring Invoices screen.
 */

export default function RecurringScheduleForm({ initial, title, note, submitLabel, onSubmit, onCancel }: {
  initial: Schedule;
  title: string;
  note?: string;
  submitLabel: string;
  /** Should reject with the server's message when saving fails. */
  onSubmit: (s: Schedule) => Promise<void>;
  onCancel: () => void;
}) {
  const [s, setS] = useState<Schedule>(initial);
  const [error, setError] = useState('');
  const [saving, setSaving] = useState(false);
  const set = (patch: Partial<Schedule>) => setS((x) => ({ ...x, ...patch }));

  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    setError('');
    setSaving(true);
    try {
      await onSubmit(s);
    } catch (err) {
      setError(errMsg(err, 'Could not save the schedule'));
    } finally {
      setSaving(false);
    }
  };

  return (
    <form onSubmit={submit} className="card mb-4" style={{ maxWidth: 560 }} aria-label={title}>
      <h3 className="text-sm font-semibold text-gray-800 mb-1">{title}</h3>
      {note && <p className="text-xs text-gray-500 mb-3">{note}</p>}
      <div className="form-grid mb-3" style={{ gridTemplateColumns: '1fr 1fr' }}>
        <label className="text-xs font-medium text-gray-600" style={{ gridColumn: '1 / -1' }}>
          Name
          <input className="mt-1" value={s.name} placeholder="e.g. Office rent — Lanka Rentals" onChange={(e) => set({ name: e.target.value })} />
        </label>
        <label className="text-xs font-medium text-gray-600">
          Repeat every
          <span className="mt-1 flex gap-2">
            <input type="number" min={1} max={99} aria-label="Repeat every" value={s.every} style={{ width: 70 }}
              onChange={(e) => set({ every: Math.max(1, Math.min(99, Number(e.target.value) || 1)) })} />
            <select aria-label="Frequency" value={s.frequency} onChange={(e) => set({ frequency: e.target.value as Schedule['frequency'] })}>
              {(Object.keys(UNIT) as Schedule['frequency'][]).map((f) => (
                <option key={f} value={f}>{s.every === 1 ? UNIT[f][0] : UNIT[f][1]}</option>
              ))}
            </select>
          </span>
        </label>
        <label className="text-xs font-medium text-gray-600">
          First invoice on
          <input type="date" required className="mt-1" aria-label="First invoice on" value={s.startDate} onChange={(e) => set({ startDate: e.target.value })} />
        </label>
        <label className="text-xs font-medium text-gray-600">
          Last invoice on or before (optional)
          <input type="date" className="mt-1" aria-label="End date" value={s.endDate} min={s.startDate} onChange={(e) => set({ endDate: e.target.value })} />
        </label>
        <p className="text-xs text-gray-500 self-end">
          {repeatsLabel(s.frequency, s.every)}{s.startDate ? `: ${fmtDate(s.startDate)}, then ${fmtDate(nextAfter(s.startDate, s.frequency, s.every))}` : ''}…
        </p>
      </div>
      <p className="text-xs text-gray-500 mb-3">
        Each invoice is made as a <strong>draft</strong> on its date while TallyCheck is open, for someone to check and post.
        Dates missed while the app was closed are made when it next opens, each with its own date.
      </p>
      {error && <p className="text-sm text-red-600 mb-2">{error}</p>}
      <div className="flex gap-2">
        <button type="submit" className="btn btn-primary" disabled={saving}>{saving ? 'Saving…' : submitLabel}</button>
        <button type="button" className="btn btn-secondary" onClick={onCancel}>Cancel</button>
      </div>
    </form>
  );
}
