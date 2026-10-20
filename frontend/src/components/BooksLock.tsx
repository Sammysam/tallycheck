import { useEffect, useState } from 'react';
import { Lock, LockOpen } from 'lucide-react';
import api, { errMsg } from '../api/client';
import { useAuth } from '../contexts/AuthContext';
import Dialog from './desktop/Dialog';

const longDate = (d: string) =>
  new Date(`${d}T00:00:00`).toLocaleDateString('en-GB', { day: 'numeric', month: 'long', year: 'numeric' });

const todayIso = () => {
  const d = new Date();
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`;
};

/**
 * D19: the books lock date. Nothing can be posted on or before it — invoices, bills, payments,
 * expenses, payroll or journal entries. Only an administrator can move it.
 */
export default function BooksLock() {
  const { user } = useAuth();
  const isAdmin = user?.role === 'Admin';
  const [lockedThrough, setLockedThrough] = useState<string | null>(null);
  const [date, setDate] = useState('');
  const [loaded, setLoaded] = useState(false);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState('');
  const [confirmUnlock, setConfirmUnlock] = useState(false);

  useEffect(() => {
    api.get('/books/lock')
      .then((r) => {
        setLockedThrough(r.data.lockedThrough);
        setDate(r.data.lockedThrough ?? '');
      })
      .catch((e) => setError(errMsg(e, 'Could not read the lock date')))
      .finally(() => setLoaded(true));
  }, []);

  const save = async (value: string | null) => {
    setSaving(true);
    setError('');
    try {
      const r = await api.put('/books/lock', { lockedThrough: value });
      setLockedThrough(r.data.lockedThrough);
      setDate(r.data.lockedThrough ?? '');
    } catch (e) {
      setError(errMsg(e, 'Could not change the lock date'));
    } finally {
      setSaving(false);
      setConfirmUnlock(false);
    }
  };

  return (
    <div className="card mt-4" style={{ maxWidth: 520 }}>
      <h3 className="flex items-center gap-1.5 text-sm font-semibold text-gray-700 mb-1">
        {lockedThrough ? <Lock size={14} className="text-orange-600" /> : <LockOpen size={14} className="text-gray-400" />}
        Lock the books
      </h3>
      <p className="text-xs text-gray-500 mb-3">
        Once a period is closed, lock it. Nothing can then be posted on or before the lock date — invoices, bills,
        payments, expenses, payroll or journal entries. Mistakes are corrected with entries dated today.
      </p>

      {!loaded ? (
        <p className="text-sm text-gray-500">Loading…</p>
      ) : (
        <>
          <p className="text-sm mb-3" data-testid="lock-status">
            {lockedThrough ? (
              <>Locked up to and including <strong>{longDate(lockedThrough)}</strong>.</>
            ) : (
              <>The books are <strong>not locked</strong>. Any date can be posted.</>
            )}
          </p>

          {isAdmin ? (
            <form
              className="flex items-end gap-2"
              onSubmit={(e) => {
                e.preventDefault();
                if (date) save(date);
              }}
            >
              <label className="text-xs font-medium text-gray-600">
                Lock up to and including
                <input type="date" className="mt-1 w-44" value={date} max={todayIso()} required onChange={(e) => setDate(e.target.value)} />
              </label>
              <button type="submit" className="btn btn-primary" disabled={saving || !date || date === lockedThrough}>
                {lockedThrough ? 'Change lock date' : 'Lock'}
              </button>
              {lockedThrough && (
                <button type="button" className="btn btn-secondary" disabled={saving} onClick={() => setConfirmUnlock(true)}>
                  Unlock
                </button>
              )}
            </form>
          ) : (
            <p className="text-xs text-gray-500">Only an administrator can change the lock date.</p>
          )}
          {error && <p className="text-sm text-red-600 mt-2">{error}</p>}
        </>
      )}

      {confirmUnlock && lockedThrough && (
        <Dialog
          title="Unlock the books"
          onClose={() => !saving && setConfirmUnlock(false)}
          buttons={
            <>
              <button type="button" className="btn btn-primary" disabled={saving} onClick={() => save(null)}>Unlock</button>
              <button type="button" className="btn btn-secondary" disabled={saving} onClick={() => setConfirmUnlock(false)}>Cancel</button>
            </>
          }
        >
          <p className="text-sm text-gray-800">
            Unlocking lets anyone with access post into periods up to {longDate(lockedThrough)} again, which can change
            figures that may already be reported or audited. The change is recorded in the Activity Log.
          </p>
        </Dialog>
      )}
    </div>
  );
}
