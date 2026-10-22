import { useEffect, useState } from 'react';
import { Bot, ChevronLeft, Copy, KeyRound, ShieldCheck } from 'lucide-react';
import api, { errMsg } from '../../api/client';
import ConfirmDialog from '../ConfirmDialog';
import Dialog from '../desktop/Dialog';
import { toast } from '../../lib/toast';
import { fmtDate } from '../../lib/region';

/**
 * D41: Claude (MCP). A key lets Claude, on this PC, read the books as the user who made it —
 * and, only if allowed, create DRAFT invoices, bills, customers and vendors. Posting, paying,
 * voiding, payroll, employees, users and settings stay out of reach.
 */

interface Key {
  id: string; name: string; username?: string | null; prefix: string; canWrite: boolean;
  createdAt: string; lastUsedAt?: string | null; revokedAt?: string | null;
}
interface Setup { mcpPath?: string | null; mcpFound: boolean; apiUrl: string }

async function copy(text: string, what: string) {
  try {
    await navigator.clipboard.writeText(text);
    toast(`${what} copied.`);
  } catch {
    toast('Select the text and press Ctrl+C to copy it.', 'info');
  }
}

export default function ClaudePanel({ onBack }: { onBack: () => void }) {
  const [keys, setKeys] = useState<Key[]>([]);
  const [setup, setSetup] = useState<Setup | null>(null);
  const [creating, setCreating] = useState(false);
  const [name, setName] = useState('Claude');
  const [canWrite, setCanWrite] = useState(false);
  const [made, setMade] = useState<{ key: string; name: string } | null>(null);
  const [revoking, setRevoking] = useState<Key | null>(null);
  const [busy, setBusy] = useState(false);

  const load = () => api.get('/api-keys').then((r) => setKeys(r.data)).catch(() => {});
  useEffect(() => {
    load();
    api.get('/api-keys/setup').then((r) => setSetup(r.data)).catch(() => {});
  }, []);

  const create = async () => {
    setBusy(true);
    try {
      const r = await api.post('/api-keys', { name, canWrite });
      setMade({ key: r.data.key, name: r.data.name });
      setCreating(false);
      load();
    } catch (e) {
      toast(errMsg(e, 'Could not make the key'), 'error');
    } finally {
      setBusy(false);
    }
  };
  const toggle = async (k: Key) => {
    try {
      await api.patch(`/api-keys/${k.id}`, { canWrite: !k.canWrite });
      toast(k.canWrite ? `${k.name} may now only read.` : `${k.name} may now create drafts.`);
      load();
    } catch (e) { toast(errMsg(e, 'Could not change the key'), 'error'); }
  };
  const revoke = async () => {
    if (!revoking) return;
    try {
      await api.delete(`/api-keys/${revoking.id}`);
      toast(`${revoking.name} revoked. Claude can no longer use it.`);
      load();
    } catch (e) { toast(errMsg(e, 'Could not revoke the key'), 'error'); }
    setRevoking(null);
  };

  const exe = setup?.mcpPath ?? 'C:\\Users\\<you>\\AppData\\Local\\Cenango Accounting\\cenango-mcp.exe';
  const desktopConfig = (key: string) => JSON.stringify({
    mcpServers: { 'cenango-accounting': { command: exe, env: { CENANGO_API_KEY: key, CENANGO_API_URL: setup?.apiUrl ?? 'http://127.0.0.1:47800' } } },
  }, null, 2);
  const codeCommand = (key: string) =>
    `claude mcp add cenango-accounting -e CENANGO_API_KEY=${key} -e CENANGO_API_URL=${setup?.apiUrl ?? 'http://127.0.0.1:47800'} -- "${exe}"`;
  const live = keys.filter((k) => !k.revokedAt);

  return (
    <div className="space-y-4">
      <button type="button" className="btn-link text-sm inline-flex items-center gap-1" onClick={onBack}><ChevronLeft size={14} /> All integrations</button>

      <section className="card !mb-0">
        <h3 className="text-sm font-semibold text-gray-800 flex items-center gap-2 mb-1"><Bot size={16} className="text-orange-600" /> Claude, the AI assistant</h3>
        <p className="text-xs text-gray-700 mb-2">Ask Claude about your books in plain words — “Who owes us more than 60 days?”, “Why was September's profit lower?”, “Draft an invoice for ABC Traders for 2 days of consulting”.</p>
        <ul className="text-xs text-gray-700 space-y-1 mb-2">
          <li className="flex gap-1.5"><ShieldCheck size={13} className="text-green-700 shrink-0 mt-0.5" /> Claude reads with the permissions of the user who made the key, and only on this PC.</li>
          <li className="flex gap-1.5"><ShieldCheck size={13} className="text-green-700 shrink-0 mt-0.5" /> Read-only by default. If you allow it, Claude may create <b>drafts</b> only — a person checks and posts them. Claude can never post, pay, void or delete.</li>
          <li className="flex gap-1.5"><ShieldCheck size={13} className="text-green-700 shrink-0 mt-0.5" /> Payroll, employees, users, settings and the activity log are closed to Claude. Everything it does is in the activity log as “… via {'<key name>'}”.</li>
        </ul>
        <p className="text-[11px] text-amber-800 bg-amber-50 border border-amber-200 rounded px-2 py-1.5">Privacy: what Claude reads to answer a question is sent to Claude (Anthropic) over the internet. The rest of the app stays offline. Revoke the key to stop it at any time.</p>
      </section>

      <section className="card !mb-0">
        <div className="flex items-center mb-2">
          <h3 className="text-sm font-semibold text-gray-800">Keys</h3>
          <button className="btn btn-primary ml-auto" onClick={() => { setName('Claude'); setCanWrite(false); setCreating(true); }}>+ New key</button>
        </div>
        {keys.length === 0 ? <p className="text-sm text-gray-500">No keys yet.</p> : (
          <table className="min-w-full text-sm" data-testid="api-keys">
            <thead><tr className="text-left text-xs text-gray-500 border-b">
              <th className="py-1.5 pr-3">Name</th><th className="pr-3">Acts as</th><th className="pr-3">May</th><th className="pr-3">Made</th><th className="pr-3">Last used</th><th />
            </tr></thead>
            <tbody>
              {keys.map((k) => (
                <tr key={k.id} className={`border-b border-[#f3f3f3] last:border-0 ${k.revokedAt ? 'text-gray-400' : ''}`}>
                  <td className="py-1.5 pr-3"><span className="inline-flex items-center gap-1.5"><KeyRound size={13} /> {k.name}</span> <span className="text-xs text-gray-400">{k.prefix}…</span></td>
                  <td className="pr-3">{k.username ?? '—'}</td>
                  <td className="pr-3">{k.revokedAt ? 'Revoked' : k.canWrite ? 'Read + create drafts' : 'Read only'}</td>
                  <td className="pr-3">{fmtDate(k.createdAt.slice(0, 10))}</td>
                  <td className="pr-3">{k.lastUsedAt ? new Date(k.lastUsedAt).toLocaleString() : 'Never'}</td>
                  <td className="text-right whitespace-nowrap">
                    {!k.revokedAt && <>
                      <button className="btn-link text-xs mr-3" onClick={() => toggle(k)}>{k.canWrite ? 'Make read-only' : 'Allow drafts'}</button>
                      <button className="btn-link text-xs text-red-600" onClick={() => setRevoking(k)}>Revoke</button>
                    </>}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
        {live.length > 0 && !setup?.mcpFound && <p className="text-[11px] text-amber-800 mt-2">The Claude connector program (cenango-mcp.exe) was not found next to the app. It is installed with the next version of the app.</p>}
      </section>

      {creating && (
        <Dialog title="New key for Claude" width={460} onClose={() => !busy && setCreating(false)}
          buttons={<>
            <button className="btn btn-primary" disabled={busy || !name.trim()} onClick={create}>{busy ? 'Making…' : 'Make key'}</button>
            <button className="btn btn-secondary" disabled={busy} onClick={() => setCreating(false)}>Cancel</button>
          </>}>
          <label className="text-xs font-medium text-gray-600 block mb-3">Name
            <input className="mt-1 w-full" value={name} onChange={(e) => setName(e.target.value)} placeholder="e.g. Claude on the office PC" aria-label="Key name" autoFocus />
          </label>
          <label className="flex items-start gap-2 text-sm">
            <input type="checkbox" className="mt-1" checked={canWrite} onChange={(e) => setCanWrite(e.target.checked)} aria-label="Allow drafts" />
            <span>Allow Claude to create <b>drafts</b> (invoices, bills, customers, vendors)<span className="block text-xs text-gray-500">Drafts are checked and posted by a person. Leave off for questions and reports only.</span></span>
          </label>
          <p className="text-xs text-gray-500 mt-3">The key acts as you, with your role's permissions.</p>
        </Dialog>
      )}

      {made && (
        <Dialog title={`Key “${made.name}” is ready`} width={640} onClose={() => setMade(null)}
          buttons={<button className="btn btn-primary" onClick={() => setMade(null)}>I have copied it</button>}>
          <p className="text-sm text-amber-800 mb-2">Copy it now — it is not shown again. Anyone with this key can read your books on this PC.</p>
          <div className="flex gap-2 mb-4">
            <input className="flex-1 font-mono text-xs" readOnly value={made.key} aria-label="New key" data-testid="new-key" onFocus={(e) => e.target.select()} />
            <button className="btn btn-secondary inline-flex items-center gap-1" onClick={() => copy(made.key, 'Key')}><Copy size={13} /> Copy</button>
          </div>
          <h4 className="text-xs font-semibold text-gray-700 uppercase tracking-wide mb-1">Claude Desktop</h4>
          <p className="text-xs text-gray-600 mb-1">Settings → Developer → Edit Config, add this to <code>claude_desktop_config.json</code>, then restart Claude Desktop:</p>
          <div className="relative mb-3">
            <pre className="text-[11px] bg-gray-50 border rounded p-2 overflow-auto max-h-40">{desktopConfig(made.key)}</pre>
            <button className="btn-link text-xs absolute top-1 right-2" onClick={() => copy(desktopConfig(made.key), 'Claude Desktop settings')}>Copy</button>
          </div>
          <h4 className="text-xs font-semibold text-gray-700 uppercase tracking-wide mb-1">Claude Code</h4>
          <div className="relative">
            <pre className="text-[11px] bg-gray-50 border rounded p-2 overflow-auto whitespace-pre-wrap break-all">{codeCommand(made.key)}</pre>
            <button className="btn-link text-xs absolute top-1 right-2" onClick={() => copy(codeCommand(made.key), 'Command')}>Copy</button>
          </div>
          <p className="text-[11px] text-gray-500 mt-2">The app must be open on this PC while Claude uses it.</p>
        </Dialog>
      )}

      {revoking && (
        <ConfirmDialog title="Revoke key" confirmLabel="Revoke" danger onConfirm={revoke} onCancel={() => setRevoking(null)}>
          <p>Revoke <b>{revoking.name}</b>? Claude stops working with it at once. This cannot be undone; make a new key if needed.</p>
        </ConfirmDialog>
      )}
    </div>
  );
}
