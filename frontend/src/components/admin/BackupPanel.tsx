import { useCallback, useEffect, useState, type ReactNode } from 'react';
import { AlertTriangle, CheckCircle2, FolderOpen, HardDrive } from 'lucide-react';
import api, { errMsg } from '../../api/client';
import PasswordInput from '../PasswordInput';

/**
 * D36: backup and restore inside Settings (it used to be a separate plain page). Same server
 * calls and the same checks: an Administrator on the main PC only.
 */

interface Status {
  passwordSet: boolean;
  settings: { enabled: boolean; folder: string | null; keep: number; everyDays: number };
  /** D62: ISO time the next automatic backup is due, "now" when overdue, null when off. */
  nextBackupDue: string | null;
  /** D62: the desktop app can open the system's folder window. */
  canChooseFolder: boolean;
  status: { lastAttemptAt?: string | null; lastSuccessAt?: string | null; lastFile?: string | null; lastError?: string | null };
  stagedRestore: { createdAt: string } | null;
  defaultFolder: string | null;
  dataFolder: string;
}
type Msg = { text: string; ok: boolean } | null;

const when = (iso?: string | null) => (iso ? new Date(iso).toLocaleString() : '—');
const count = (n: number, word: string) => `${n} ${word}${n === 1 ? '' : 's'}`;
const size = (n: number) => (n > 1048576 ? `${(n / 1048576).toFixed(1)} MB` : `${Math.max(1, Math.round(n / 1024))} KB`);

function Card({ id, title, help, children }: { id: string; title: string; help?: string; children: ReactNode }) {
  return (
    <section id={id} className="card !mb-0">
      <h3 className="text-sm font-semibold text-gray-800">{title}</h3>
      {help && <p className="text-xs text-gray-500 mt-0.5 mb-3">{help}</p>}
      {children}
    </section>
  );
}

function Note({ id, msg }: { id: string; msg: Msg }) {
  if (!msg) return <p id={id} className="hidden" />;
  return <p id={id} className={`text-sm mt-2 whitespace-pre-line ${msg.ok ? 'text-green-700' : 'text-red-600'}`} role={msg.ok ? 'status' : 'alert'}>{msg.text}</p>;
}

/** Hours since an ISO time, or Infinity. */
const ageHours = (iso?: string | null) => (iso ? (Date.now() - new Date(iso).getTime()) / 3600000 : Infinity);

export default function BackupPanel() {
  const [s, setS] = useState<Status | null>(null);
  const [loadError, setLoadError] = useState('');
  const [pw, setPw] = useState({ current: '', next: '', again: '' });
  const [pwMsg, setPwMsg] = useState<Msg>(null);
  const [folder, setFolder] = useState('');
  const [bkMsg, setBkMsg] = useState<Msg>(null);
  const [daily, setDaily] = useState({ enabled: false, folder: '', keep: 14, everyDays: 1 });
  const [dlMsg, setDlMsg] = useState<Msg>(null);
  const [rs, setRs] = useState({ file: '', password: '', confirm: '' });
  const [rsMsg, setRsMsg] = useState<Msg>(null);
  const [busy, setBusy] = useState('');

  const load = useCallback(() => api.get('/system/backup').then((r) => {
    const st: Status = r.data;
    setS(st);
    setFolder((f) => f || st.settings.folder || st.defaultFolder || '');
    setDaily({ enabled: st.settings.enabled, folder: st.settings.folder || st.defaultFolder || '', keep: st.settings.keep, everyDays: st.settings.everyDays || 1 });
  }).catch((e) => setLoadError(errMsg(e, 'Backup is not available'))), []);
  useEffect(() => { load(); }, [load]);

  const act = async (name: string, set: (m: Msg) => void, busyText: string, work: () => Promise<string>) => {
    setBusy(name);
    set({ text: busyText, ok: true });
    try {
      set({ text: await work(), ok: true });
      load();
    } catch (e) {
      set({ text: errMsg(e, 'That did not work'), ok: false });
    } finally {
      setBusy('');
    }
  };

  /** D62: the system's own folder window (desktop app); returns the folder, or null if closed. */
  const chooseFolder = async (current: string, set: (m: Msg) => void): Promise<string | null> => {
    try {
      const { data } = await api.post('/system/backup/choose-folder', { start: current || undefined });
      return data.folder ?? null;
    } catch (e) {
      set({ text: errMsg(e, 'Could not open the folder window'), ok: false });
      return null;
    }
  };

  if (loadError) {
    return (
      <div className="card text-sm text-gray-700">
        <p className="font-medium text-gray-900 mb-1">Backup and restore</p>
        <p>{loadError}. Sign in as an Administrator on the main PC to use it.</p>
      </div>
    );
  }
  if (!s) return <p className="text-sm text-gray-500">Loading…</p>;

  const lastOk = s.status.lastSuccessAt;
  const every = s.settings.everyDays || 1;
  const autoOff = !s.settings.enabled;
  // Late when older than the chosen gap plus a little slack (the check runs hourly).
  const stale = ageHours(lastOk) > every * 24 + 2;
  const warn = !s.passwordSet || stale || autoOff;
  const days = (n: number) => (n === 1 ? 'day' : `${n} days`);

  return (
    <div className="space-y-4" style={{ maxWidth: 820 }}>
      <div className={`rounded-md border px-4 py-3 flex items-start gap-3 ${warn ? 'border-amber-200 bg-amber-50' : 'border-green-200 bg-green-50'}`} id="status-card">
        {warn ? <AlertTriangle size={18} className="text-amber-600 mt-0.5" /> : <CheckCircle2 size={18} className="text-green-600 mt-0.5" />}
        <div className="text-sm">
          <p className="font-medium text-gray-900">
            {!s.passwordSet ? 'Backups are off until you set a backup password.'
              : !lastOk ? 'No backup has been made yet.'
                : stale ? `The last backup was ${when(lastOk)} — more than ${days(every)} ago.`
                  : `Last backup ${when(lastOk)}.`}
          </p>
          <p className="text-xs text-gray-600 mt-0.5">
            {autoOff
              ? <strong className="text-amber-800">Automatic backups are off — your books are not being backed up automatically. </strong>
              : <>Automatic backup: every {days(every)}, into {s.settings.folder}{s.nextBackupDue && s.nextBackupDue !== 'now' ? `, next after ${when(s.nextBackupDue)}` : s.nextBackupDue === 'now' ? ', due now' : ''} · </>}
            Data folder: {s.dataFolder}
            {s.status.lastError && <><br /><span className="text-red-700">Last problem ({when(s.status.lastAttemptAt)}): {s.status.lastError}</span></>}
          </p>
        </div>
      </div>

      {s.stagedRestore && (
        <div className="rounded-md border border-blue-200 bg-blue-50 px-4 py-3 text-sm flex items-center gap-3">
          <span className="flex-1">A restore of the backup made {when(s.stagedRestore.createdAt)} is ready. Close and reopen the app to apply it.</span>
          <button className="btn btn-secondary" onClick={async () => { await api.delete('/system/restore/stage'); load(); }}>Cancel the restore</button>
        </div>
      )}

      <Card id="password-card" title="Backup password" help="Every backup is encrypted with this password. Keep it somewhere safe — a backup cannot be opened without it.">
        <div className="grid gap-3" style={{ gridTemplateColumns: s.passwordSet ? '1fr 1fr 1fr' : '1fr 1fr' }}>
          {s.passwordSet && (
            <label className="text-xs font-medium text-gray-600">Current password
              <div className="mt-1"><PasswordInput placeholder="Current password" minLength={0} value={pw.current} onChange={(v) => setPw({ ...pw, current: v })} ariaLabel="pw-current" /></div>
            </label>
          )}
          <label className="text-xs font-medium text-gray-600">{s.passwordSet ? 'New password' : 'Password'}
            <input id="pw-new" className="mt-1" type="password" value={pw.next} onChange={(e) => setPw({ ...pw, next: e.target.value })} />
          </label>
          <label className="text-xs font-medium text-gray-600">Type it again
            <input id="pw-again" className="mt-1" type="password" value={pw.again} onChange={(e) => setPw({ ...pw, again: e.target.value })} />
          </label>
        </div>
        <button id="pw-save" className="btn btn-primary mt-3" disabled={busy === 'pw' || !pw.next}
          onClick={() => {
            if (pw.next !== pw.again) { setPwMsg({ text: 'The two new passwords are different.', ok: false }); return; }
            act('pw', setPwMsg, 'Saving…', async () => {
              await api.post('/system/backup/password', { password: pw.next, currentPassword: pw.current });
              setPw({ current: '', next: '', again: '' });
              return 'Backup password saved. Write it down and keep it safe.';
            });
          }}>{s.passwordSet ? 'Change password' : 'Set password'}</button>
        <Note id="pw-msg" msg={pwMsg} />
      </Card>

      <Card id="backup-card" title="Back up now" help="One encrypted file with the whole database and every employee photo, document and payslip. It is checked after writing.">
        <label className="text-xs font-medium text-gray-600 block">Folder
          <span className="mt-1 flex gap-2">
            <input id="bk-folder" value={folder} onChange={(e) => setFolder(e.target.value)} placeholder="e.g. a USB drive, E:\\Backups" />
            {s.canChooseFolder && (
              <button type="button" className="btn btn-secondary whitespace-nowrap inline-flex items-center gap-1.5" disabled={busy !== ''}
                onClick={async () => { const f = await chooseFolder(folder, setBkMsg); if (f) setFolder(f); }}>
                <FolderOpen size={14} /> Choose folder…
              </button>
            )}
          </span>
        </label>
        <button id="bk-run" className="btn btn-primary mt-3 inline-flex items-center gap-1.5" disabled={busy === 'bk' || !s.passwordSet}
          onClick={() => act('bk', setBkMsg, 'Backing up and checking the backup…', async () => {
            const r = (await api.post('/system/backup/run', { folder })).data;
            let text = `Backup written and verified:\n${r.file} (${size(r.size)}, ${count(r.files, 'stored file')})`;
            if (r.missingFiles?.length) text += `\nWarning: ${r.missingFiles.length} files the records refer to were already missing.`;
            return text;
          })}><HardDrive size={14} /> {busy === 'bk' ? 'Backing up…' : 'Back up now'}</button>
        <Note id="bk-msg" msg={bkMsg} />
      </Card>

      <Card id="daily-card" title="Automatic backup" help="While TallyCheck is open it makes a full backup into this folder (subfolder “scheduled”) whenever the last one is older than the days you choose, and keeps the newest copies. If the PC was off, the backup is made soon after TallyCheck opens.">
        <label className="flex items-center gap-3 text-sm cursor-pointer select-none">
          <button type="button" role="switch" aria-checked={daily.enabled} aria-label="Automatic backups"
            onClick={() => setDaily({ ...daily, enabled: !daily.enabled })}
            className={`relative inline-flex h-5 w-9 shrink-0 rounded-full transition-colors ${daily.enabled ? 'bg-green-600' : 'bg-gray-300'}`}>
            <span className={`absolute top-0.5 h-4 w-4 rounded-full bg-white shadow transition-all ${daily.enabled ? 'left-[18px]' : 'left-0.5'}`} />
          </button>
          <span className="font-medium">{daily.enabled ? 'On' : 'Off'}</span>
          {!daily.enabled && <span className="text-xs text-amber-700">Turn on so a copy of the books is made automatically.</span>}
        </label>
        <div className="grid gap-3 mt-3" style={{ gridTemplateColumns: '150px 1fr 120px' }}>
          <label className="text-xs font-medium text-gray-600">Back up every
            <span className="mt-1 flex items-center gap-2">
              <input type="number" min={1} max={30} value={daily.everyDays} aria-label="Back up every (days)" style={{ width: 70 }}
                onChange={(e) => setDaily({ ...daily, everyDays: Math.max(1, Math.min(30, Number(e.target.value) || 1)) })} />
              <span className="text-gray-700">{daily.everyDays === 1 ? 'day' : 'days'}</span>
            </span>
          </label>
          <label className="text-xs font-medium text-gray-600">Folder
            <span className="mt-1 flex gap-2">
              <input value={daily.folder} onChange={(e) => setDaily({ ...daily, folder: e.target.value })} aria-label="Automatic backup folder" />
              {s.canChooseFolder && (
                <button type="button" className="btn btn-secondary whitespace-nowrap inline-flex items-center gap-1.5" disabled={busy !== ''}
                  onClick={async () => { const f = await chooseFolder(daily.folder, setDlMsg); if (f) setDaily((d) => ({ ...d, folder: f })); }}>
                  <FolderOpen size={14} /> Choose folder…
                </button>
              )}
            </span>
          </label>
          <label className="text-xs font-medium text-gray-600">Keep (copies)
            <input className="mt-1" type="number" min={1} max={365} value={daily.keep} onChange={(e) => setDaily({ ...daily, keep: Number(e.target.value) })} aria-label="Copies to keep" />
          </label>
        </div>
        <p className="text-xs text-gray-500 mt-2">
          Older automatic copies beyond {daily.keep} are deleted, newest kept. Backups made with “Back up now” are never deleted.
          A folder on another computer (\\server\share) is not allowed; a USB drive or a second disk is a good choice.
        </p>
        <button className="btn btn-secondary mt-3" disabled={busy === 'dl'}
          onClick={() => act('dl', setDlMsg, 'Saving…', async () => {
            await api.put('/system/backup/settings', { enabled: daily.enabled, folder: daily.folder, keep: daily.keep, everyDays: daily.everyDays });
            return daily.enabled
              ? `Saved. TallyCheck backs up every ${daily.everyDays === 1 ? 'day' : `${daily.everyDays} days`} while it is open.`
              : 'Saved. Automatic backups are off.';
          })}>Save automatic backup settings</button>
        <Note id="dl-msg" msg={dlMsg} />
      </Card>

      <Card id="restore-card" title="Restore" help="The backup is checked completely first. The current data is replaced when the app next starts, and kept in a “pre-restore” folder inside the data folder.">
        <div className="grid gap-3" style={{ gridTemplateColumns: '2fr 1fr' }}>
          <label className="text-xs font-medium text-gray-600">Backup file (full path to the .qbak file)
            <input id="rs-file" className="mt-1" value={rs.file} onChange={(e) => setRs({ ...rs, file: e.target.value })} />
          </label>
          <label className="text-xs font-medium text-gray-600">Its password (if different)
            <input id="rs-password" className="mt-1" type="password" value={rs.password} onChange={(e) => setRs({ ...rs, password: e.target.value })} />
          </label>
        </div>
        <div className="flex flex-wrap items-end gap-2 mt-3">
          <button id="rs-verify" className="btn btn-secondary" disabled={busy === 'rs' || !rs.file}
            onClick={() => act('rs', setRsMsg, 'Checking every entry of the backup…', async () => {
              const m = (await api.post('/system/restore/verify', { file: rs.file, password: rs.password || undefined })).data;
              return `The backup is complete and readable.\nMade ${when(m.createdAt)} by version ${m.appVersion}; ${count(m.counts.employees, 'employee')}, `
                + `${count(m.counts.journal_entries, 'journal entry').replace('entrys', 'entries')}, ${count(m.counts.invoices, 'invoice')}.`;
            })}>Check backup</button>
          <label className="text-xs font-medium text-gray-600">Type RESTORE to confirm
            <input id="rs-confirm" className="mt-1 !w-40" value={rs.confirm} onChange={(e) => setRs({ ...rs, confirm: e.target.value })} />
          </label>
          <button id="rs-stage" className="btn btn-danger" disabled={busy === 'rs' || rs.confirm !== 'RESTORE' || !rs.file}
            onClick={() => act('rs', setRsMsg, 'Checking and unpacking the backup…', async () => {
              await api.post('/system/restore/stage', { file: rs.file, password: rs.password || undefined, confirm: rs.confirm });
              setRs({ ...rs, confirm: '' });
              return 'Ready. Close and reopen the app to finish the restore.';
            })}>Restore at next start</button>
        </div>
        <Note id="rs-msg" msg={rsMsg} />
      </Card>
    </div>
  );
}
