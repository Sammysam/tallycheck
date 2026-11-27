import { useEffect, useState } from 'react';
import Dialog from './Dialog';
import { errMsg } from '../../api/client';
import { checkForUpdate, installUpdate, type UpdateStatus } from '../../lib/updates';

/** D45: Help → Check for updates, and the status bar's "Update available". D78: a Microsoft Store copy
 *  asks the Store and installs through it. */
const STORE_NAME = /Mac/i.test(navigator.userAgent) ? 'the App Store' : 'the Microsoft Store';

export default function UpdateDialog({ initial, onClose }: { initial: UpdateStatus | null; onClose: () => void }) {
  const [status, setStatus] = useState<UpdateStatus | null>(initial);
  const [checking, setChecking] = useState(!initial);
  const [installing, setInstalling] = useState(false);
  const [restarting, setRestarting] = useState('');
  const [error, setError] = useState('');

  useEffect(() => {
    if (initial) return;
    checkForUpdate()
      .then(setStatus)
      .catch((e) => setError(errMsg(e, 'Could not check for updates')))
      .finally(() => setChecking(false));
  }, [initial]);

  const install = async () => {
    setInstalling(true);
    setError('');
    try {
      const r = await installUpdate();
      setRestarting(r.installing.version);
    } catch (e) {
      setError(errMsg(e, 'The update could not be installed'));
      setInstalling(false);
    }
  };

  const available = status?.available ?? null;
  const store = status?.channel === 'store';
  const close = <button type="button" className="btn btn-secondary" onClick={onClose} disabled={installing}>Close</button>;

  if (restarting) {
    return (
      <Dialog title="Updating TallyCheck" onClose={() => undefined} buttons={null}>
        <p className="text-sm text-gray-700">
          {store
            ? `TallyCheck ${restarting} is downloaded from the Microsoft Store. TallyCheck will now close, install the update and open again by itself (if it does not, open it from the Start menu). Your data is kept.`
            : `TallyCheck ${restarting} is downloaded and checked. TallyCheck will now close, install the update and open again by itself. Your data is kept.`}
        </p>
      </Dialog>
    );
  }

  return (
    <Dialog
      title="Check for updates"
      onClose={installing ? () => undefined : onClose}
      width={460}
      buttons={
        available ? (
          <>
            <button type="button" className="btn btn-primary" onClick={install} disabled={installing}>
              {installing ? (store ? 'Downloading from the Store…' : 'Downloading…') : `Update to ${available.version}`}
            </button>
            {!installing && <button type="button" className="btn btn-secondary" onClick={onClose}>Later</button>}
          </>
        ) : close
      }
    >
      <div className="text-sm text-gray-700 space-y-2">
        {checking && <p>Checking for a newer version…</p>}
        {status && <p>You have TallyCheck <b>{status.current}</b>.</p>}
        {store && !status?.supported && (
          <p>This copy of TallyCheck is updated by {STORE_NAME}, which installs new versions by itself.</p>
        )}
        {store && status?.supported && (
          <p className="text-xs text-gray-500">This copy is updated through the Microsoft Store. Only the Store is contacted; your books stay on this PC.</p>
        )}
        {status && status.channel !== 'store' && !status.supported && (
          <p>This copy of TallyCheck is not set up to update itself. Install a newer version from its installer.</p>
        )}
        {status?.supported && status.error && <p className="text-amber-700">{status.error}</p>}
        {status?.supported && !status.error && !available && <p>You have the newest version.</p>}
        {available && (
          <>
            <p>
              <b>TallyCheck {available.version}</b> is available
              {available.date ? ` (published ${available.date.slice(0, 10)})` : ''}.
            </p>
            {store && !available.notes && (
              <p className="text-xs text-gray-500">The Store may ask you to confirm the download. After the update, Help → What's new lists the changes.</p>
            )}
            {available.notes && (
              <div className="border border-[#e5e5e5] bg-[#fafafa] rounded p-2 max-h-48 overflow-auto whitespace-pre-wrap text-xs">
                {available.notes}
              </div>
            )}
            <p className="text-xs text-gray-500">
              TallyCheck closes while it updates and opens again by itself. Your data is kept, and a copy of the database is
              made before the new version changes it. Make a backup first if you have not today (File → Backup and restore).
            </p>
          </>
        )}
        {error && <p className="text-red-600">{error}</p>}
      </div>
    </Dialog>
  );
}
