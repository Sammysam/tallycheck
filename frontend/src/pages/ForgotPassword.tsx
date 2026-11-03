import { useEffect, useState, type FormEvent } from 'react';
import { useNavigate } from 'react-router-dom';
import { KeyRound, MonitorCheck } from 'lucide-react';
import AuthShell, { PasswordInput } from '../components/AuthShell';
import RecoveryKeyCard from '../components/RecoveryKeyCard';
import api, { errMsg } from '../api/client';

type Way = 'key' | 'windows';

/**
 * D49: reset a forgotten password with the recovery key, or — in the desktop app on the main
 * PC — by confirming with Windows (Windows Hello or the Windows password).
 */
export default function ForgotPassword() {
  const [way, setWay] = useState<Way>('key');
  const [windowsOk, setWindowsOk] = useState(false);
  const [username, setUsername] = useState('');
  const [recoveryKey, setRecoveryKey] = useState('');
  const [password, setPassword] = useState('');
  const [confirm, setConfirm] = useState('');
  const [error, setError] = useState('');
  const [loading, setLoading] = useState(false);
  const [newKey, setNewKey] = useState('');
  const navigate = useNavigate();

  useEffect(() => {
    api.get<{ windows: boolean }>('/auth/recovery/options')
      .then(({ data }) => setWindowsOk(data.windows))
      .catch(() => {});
  }, []);

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    setError('');
    if (password !== confirm) {
      setError('The two passwords do not match');
      return;
    }
    setLoading(true);
    try {
      const { data } = way === 'key'
        ? await api.post<{ recoveryKey: string }>('/auth/recovery/key', { username, recoveryKey, password })
        : await api.post<{ recoveryKey: string }>('/auth/recovery/windows', { username, password });
      setNewKey(data.recoveryKey);
    } catch (err) {
      setError(errMsg(err, 'The password could not be reset'));
    } finally {
      setLoading(false);
    }
  };

  if (newKey) {
    return (
      <AuthShell>
        <p className="text-sm text-green-700 bg-green-50 px-3 py-2 rounded-lg mb-4">
          Your password has been changed. Your old recovery key no longer works — here is your new one.
        </p>
        <RecoveryKeyCard recoveryKey={newKey} username={username} doneLabel="Go to sign in" onDone={() => navigate('/login', { replace: true })} />
      </AuthShell>
    );
  }

  const input =
    'w-full px-3 py-2 text-sm border border-gray-300 rounded-lg focus:outline-none focus:ring-2 focus:ring-orange-500 focus:border-transparent';
  const tab = (w: Way) =>
    `flex-1 h-8 inline-flex items-center justify-center gap-1.5 text-xs rounded ${
      way === w ? 'bg-white shadow-sm text-gray-900 font-medium' : 'text-gray-500 hover:text-gray-700'
    }`;

  return (
    <AuthShell>
      <div className="mb-4">
        <h1 className="text-lg font-semibold text-gray-900">Forgot your password?</h1>
        <p className="text-sm text-gray-500 mt-1">Set a new password for your TallyCheck user.</p>
      </div>

      {windowsOk && (
        <div className="flex gap-1 p-1 bg-gray-100 rounded-lg mb-4">
          <button type="button" className={tab('key')} onClick={() => { setWay('key'); setError(''); }}>
            <KeyRound size={13} /> Recovery key
          </button>
          <button type="button" className={tab('windows')} onClick={() => { setWay('windows'); setError(''); }}>
            <MonitorCheck size={13} /> Windows sign-in
          </button>
        </div>
      )}

      <form onSubmit={submit} className="space-y-3.5">
        {way === 'windows' && (
          <p className="text-xs text-gray-600 bg-gray-50 px-3 py-2 rounded-lg">
            Windows will ask you to confirm it's you — with your Windows PIN, fingerprint, face or Windows password.
            This works only on the main PC, for the Windows user that runs TallyCheck.
          </p>
        )}
        <div>
          <label className="block text-xs font-medium text-gray-600 mb-1.5">Username</label>
          <input type="text" required value={username} onChange={(e) => setUsername(e.target.value)} className={input} autoFocus />
        </div>
        {way === 'key' && (
          <div>
            <label className="block text-xs font-medium text-gray-600 mb-1.5">Recovery key</label>
            <input
              type="text"
              required
              value={recoveryKey}
              onChange={(e) => setRecoveryKey(e.target.value)}
              className={`${input} font-mono tracking-wide`}
              placeholder="TCRK-XXXX-XXXX-XXXX-XXXX-XXXX"
              autoComplete="off"
              spellCheck={false}
            />
          </div>
        )}
        <div>
          <label className="block text-xs font-medium text-gray-600 mb-1.5">New password (at least 8 characters)</label>
          <PasswordInput required minLength={8} value={password} onChange={(e) => setPassword(e.target.value)} className={input} />
        </div>
        <div>
          <label className="block text-xs font-medium text-gray-600 mb-1.5">Confirm new password</label>
          <PasswordInput required minLength={8} value={confirm} onChange={(e) => setConfirm(e.target.value)} className={input} />
        </div>
        {error && <p className="text-xs text-red-600 bg-red-50 px-3 py-2 rounded-lg">{error}</p>}
        <button
          type="submit"
          disabled={loading}
          className="w-full h-8 px-4 bg-orange-600 border border-orange-700 text-white text-sm rounded hover:bg-orange-700 disabled:opacity-50 disabled:cursor-not-allowed transition-colors"
        >
          {loading ? (way === 'windows' ? 'Waiting for Windows…' : 'Resetting…') : way === 'windows' ? 'Confirm with Windows' : 'Reset password'}
        </button>
      </form>

      <p className="text-xs text-gray-500 mt-4">
        No recovery key? Another administrator can reset your password under Administration → Users.
        {!windowsOk && ' On the main PC, the TallyCheck desktop app can also reset it with your Windows sign-in.'}
      </p>
      <button type="button" onClick={() => navigate('/login')} className="mt-3 text-xs text-orange-700 hover:underline">
        ← Back to sign in
      </button>
    </AuthShell>
  );
}
