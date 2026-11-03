import { useState } from 'react';
import { Copy, Download, Printer, KeyRound } from 'lucide-react';

/**
 * D49: shows a password recovery key once, with ways to keep it (copy, save as a file, print)
 * and a tick box the person must tick before going on.
 */
export default function RecoveryKeyCard({
  recoveryKey,
  username,
  onDone,
  doneLabel = 'Continue',
}: {
  recoveryKey: string;
  username: string;
  onDone: () => void;
  doneLabel?: string;
}) {
  const [saved, setSaved] = useState(false);
  const [copied, setCopied] = useState(false);

  const text = [
    'TallyCheck password recovery key',
    '',
    `Username:      ${username}`,
    `Recovery key:  ${recoveryKey}`,
    `Made on:       ${new Date().toLocaleString()}`,
    '',
    'If you forget your password, click "Forgot password?" on the TallyCheck sign-in screen',
    'and enter your username and this key. Each key works once; TallyCheck then gives you a new one.',
    'Keep this somewhere safe and private, away from the computer.',
  ].join('\r\n');

  const copy = async () => {
    try {
      await navigator.clipboard.writeText(recoveryKey);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch {
      /* the key is on screen to write down */
    }
  };

  const download = () => {
    const url = URL.createObjectURL(new Blob([text], { type: 'text/plain;charset=utf-8' }));
    const a = document.createElement('a');
    a.href = url;
    a.download = `TallyCheck recovery key - ${username}.txt`;
    a.click();
    URL.revokeObjectURL(url);
  };

  const print = () => {
    // A hidden frame, so printing works the same in the desktop window and in a browser.
    const frame = document.createElement('iframe');
    frame.style.position = 'fixed';
    frame.style.width = '0';
    frame.style.height = '0';
    frame.style.border = '0';
    document.body.appendChild(frame);
    const w = frame.contentWindow;
    if (!w) return;
    const esc = (s: string) => s.replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[c] ?? c);
    w.document.write(
      `<html><head><title>TallyCheck recovery key</title></head><body style="font-family:Arial,sans-serif;padding:32px">` +
        `<h2>TallyCheck password recovery key</h2><p>Username: <b>${esc(username)}</b></p>` +
        `<p style="font-family:Consolas,monospace;font-size:22px;letter-spacing:2px;border:2px dashed #ea580c;padding:14px;display:inline-block">${esc(recoveryKey)}</p>` +
        `<p>If you forget your password, click <b>Forgot password?</b> on the sign-in screen and enter your username and this key.<br/>` +
        `Each key works once; TallyCheck then gives you a new one. Keep this paper somewhere safe and private.</p></body></html>`,
    );
    w.document.close();
    w.focus();
    w.print();
    setTimeout(() => frame.remove(), 1000);
  };

  const btn = 'flex-1 h-8 inline-flex items-center justify-center gap-1.5 text-xs border border-gray-300 rounded hover:bg-gray-50';

  return (
    <div>
      <div className="mb-4 flex items-start gap-3">
        <KeyRound className="text-orange-600 shrink-0 mt-0.5" size={20} />
        <div>
          <h1 className="text-lg font-semibold text-gray-900">Save your recovery key</h1>
          <p className="text-sm text-gray-500 mt-1">
            If you ever forget your password, this key lets you set a new one. It is shown only now.
          </p>
        </div>
      </div>
      <div className="text-center font-mono text-[17px] tracking-wider font-semibold text-gray-900 bg-orange-50 border-2 border-dashed border-orange-400 rounded-lg py-3 select-all">
        {recoveryKey}
      </div>
      <p className="text-xs text-gray-500 mt-1.5 text-center">for user <b>{username}</b></p>
      <div className="flex gap-2 mt-4">
        <button type="button" className={btn} onClick={copy}><Copy size={13} />{copied ? 'Copied' : 'Copy'}</button>
        <button type="button" className={btn} onClick={download}><Download size={13} />Save as file</button>
        <button type="button" className={btn} onClick={print}><Printer size={13} />Print</button>
      </div>
      <p className="text-xs text-gray-500 mt-3">
        Keep it somewhere safe and private, for example printed in the office safe or in a password manager —
        not on a note stuck to this computer.
      </p>
      <label className="flex items-center gap-2 mt-4 text-sm text-gray-700">
        <input type="checkbox" checked={saved} onChange={(e) => setSaved(e.target.checked)} className="accent-orange-600" />
        I have saved my recovery key
      </label>
      <button
        type="button"
        disabled={!saved}
        onClick={onDone}
        className="mt-4 w-full h-8 px-4 bg-orange-600 border border-orange-700 text-white text-sm rounded hover:bg-orange-700 disabled:opacity-50 disabled:cursor-not-allowed"
      >
        {doneLabel}
      </button>
    </div>
  );
}
