import { useCallback, useEffect, useState, type ReactNode } from 'react';
import { AlertTriangle, CheckCircle2, HardDrive } from 'lucide-react';
import api, { errMsg } from '../../api/client';
import PasswordInput from '../PasswordInput';

/**
 * D36: backup and restore inside Settings (it used to be a separate plain page). Same server
 * calls and the same checks: an Administrator on the main PC only.
 */

interface Status {
  passwordSet: boolean;
  settings: { enabled: boolean; folder: string | null; keep: number };
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
  const [daily, setDaily] = useState({ enabled: false, folder: '', keep: 14 });
  const [dlMsg, setDlMsg] = useState<Msg>(null);
  const [rs, setRs] = useState({ file: '', password: '', confirm: '' });
  const [rsMsg, setRsMsg] = useState<Msg>(null);
  const [busy, setBusy] = useState('');

  const load = useCallback(() => api.get('/system/backup').then((r) => {
    const st: Status = r.data;
    setS(st);
    setFolder((f) => f || st.settings.folder || st.defaultFolder || '');
    setDaily({ enabled: st.settings.enabled, folder: st.settings.folder || st.defaultFolder || '', keep: st.settings.keep });
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
  const stale = ageHours(lastOk) > 26;

  return (
    <div className="space-y-4" style={{ maxWidth: 820 }}>
      <div className={`rounded-md border px-4 py-3 flex items-start gap-3 ${!s.passwordSet || stale ? 'border-amber-200 bg-amber-50' : 'border-green-200 bg-green-50'}`} id="status-card">
        {!s.passwordSet || stale ? <AlertTriangle size={18} className="text-amber-600 mt-0.5" /> : <CheckCircle2 size={18} className="text-green-600 mt-0.5" />}
        <div className="text-sm">
          <p className="font-medium text-gray-900">
            {!s.passwordSet ? 'Backups are off until you set a backup password.'
              : !lastOk ? 'No backup has been made yet.'
                : stale ? `The last backup was ${when(lastOk)} — more than a day ago.`
                  : `Last backup ${when(lastOk)}.`}
          </p>
          <p className="text-xs text-gray-600 mt-0.5">
            Daily backup: {s.settings.enabled ? `on, into ${s.settings.folder}` : 'off'} · Data folder: {s.dataFolder}
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
          <input id="bk-folder" className="mt-1" value={folder} onChange={(e) => setFolder(e.target.value)} placeholder="e.g. a USB drive, E:\\Backups" />
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

      <Card id="daily-card" title="Daily backup" help="While the app is running it backs up once a day into this folder (subfolder “scheduled”) and keeps the newest copies.">
        <label className="flex items-center gap-2 text-sm">
          <input type="checkbox" checked={daily.enabled} onChange={(e) => setDaily({ ...daily, enabled: e.target.checked })} aria-label="Make a daily backup" /> Make a daily backup
        </label>
        <div className="grid gap-3 mt-2" style={{ gridTemplateColumns: '1fr 120px' }}>
          <label className="text-xs font-medium text-gray-600">Folder
            <input className="mt-1" value={daily.folder} onChange={(e) => setDaily({ ...daily, folder: e.target.value })} aria-label="Daily backup folder" />
          </label>
          <label className="text-xs font-medium text-gray-600">Keep (copies)
            <input className="mt-1" type="number" min={1} max={365} value={daily.keep} onChange={(e) => setDaily({ ...daily, keep: Number(e.target.value) })} aria-label="Copies to keep" />
          </label>
        </div>
        <button className="btn btn-secondary mt-3" disabled={busy === 'dl'}
          onClick={() => act('dl', setDlMsg, 'Saving…', async () => {
            await api.put('/system/backup/settings', { enabled: daily.enabled, folder: daily.folder, keep: daily.keep });
            return 'Saved.';
          })}>Save daily backup settings</button>
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
