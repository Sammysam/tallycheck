import { useEffect, useState, type FormEvent } from 'react';
import { useNavigate, useSearchParams } from 'react-router-dom';
import { KeyRound, MonitorCheck, UserRound } from 'lucide-react';
import AuthShell, { PasswordInput } from '../components/AuthShell';
import RecoveryKeyCard from '../components/RecoveryKeyCard';
import api, { errMsg } from '../api/client';
import { isMac } from '../lib/platform';

type Way = 'key' | 'windows';
type What = 'password' | 'username';

/**
 * D49: reset a forgotten password with the recovery key, or — in the desktop app on the main
 * PC — by confirming with Windows (Windows Hello or the Windows password).
 * D56: find a forgotten username the same two ways. The recovery key shows only its own
 * username; Windows sign-in lists the active usernames to the Windows owner of the main PC.
 */
export default function ForgotPassword() {
  const [params] = useSearchParams();
  const [what, setWhat] = useState<What>(params.get('what') === 'username' ? 'username' : 'password');
  const [way, setWay] = useState<Way>('key');
  const [found, setFound] = useState<{ username: string; role: string | null }[] | null>(null);
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

  const findUsername = async (e: FormEvent) => {
    e.preventDefault();
    setError('');
    setLoading(true);
    try {
      if (way === 'key') {
        const { data } = await api.post<{ username: string }>('/auth/recovery/username/key', { recoveryKey });
        setFound([{ username: data.username, role: null }]);
      } else {
        const { data } = await api.post<{ users: { username: string; role: string | null }[] }>('/auth/recovery/username/windows');
        setFound(data.users);
      }
    } catch (err) {
      setError(errMsg(err, 'The username could not be found'));
    } finally {
      setLoading(false);
    }
  };

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

  const signInAs = (name: string) => navigate('/login', { replace: true, state: { username: name } });
  const choose = (w: What) => { setWhat(w); setError(''); setFound(null); };
  const whatTab = (w: What) =>
    `flex-1 h-8 inline-flex items-center justify-center gap-1.5 text-xs rounded ${
      what === w ? 'bg-orange-50 text-orange-700 font-medium border border-orange-200' : 'text-gray-500 hover:text-gray-700 border border-transparent'
    }`;

  return (
    <AuthShell>
      <div className="flex gap-1 mb-4" role="tablist" aria-label="What did you forget?">
        <button type="button" role="tab" aria-selected={what === 'password'} className={whatTab('password')} onClick={() => choose('password')}>
          <KeyRound size={13} /> Password
        </button>
        <button type="button" role="tab" aria-selected={what === 'username'} className={whatTab('username')} onClick={() => choose('username')} data-testid="forgot-username-tab">
          <UserRound size={13} /> Username
        </button>
      </div>
      <div className="mb-4">
        <h1 className="text-lg font-semibold text-gray-900">{what === 'username' ? 'Forgot your username?' : 'Forgot your password?'}</h1>
        <p className="text-sm text-gray-500 mt-1">
          {what === 'username' ? 'Find the username you sign in with.' : 'Set a new password for your TallyCheck user.'}
        </p>
      </div>

      {windowsOk && (
        <div className="flex gap-1 p-1 bg-gray-100 rounded-lg mb-4">
          <button type="button" className={tab('key')} onClick={() => { setWay('key'); setError(''); setFound(null); }}>
            <KeyRound size={13} /> Recovery key
          </button>
          <button type="button" className={tab('windows')} onClick={() => { setWay('windows'); setError(''); setFound(null); }}>
            <MonitorCheck size={13} /> Windows sign-in
          </button>
        </div>
      )}

      {what === 'username' && found && (
        <div className="space-y-3" data-testid="found-usernames">
          <p className="text-sm text-gray-700">
            {way === 'key' ? 'This recovery key belongs to:' : 'The TallyCheck users on this PC:'}
          </p>
          <ul className="space-y-1.5">
            {found.map((u) => (
              <li key={u.username} className="flex items-center justify-between gap-3 px-3 py-2 bg-gray-50 rounded-lg">
                <span className="text-sm font-medium text-gray-900 select-all">
                  {u.username}
                  {u.role && <span className="ml-2 text-xs font-normal text-gray-500">{u.role}</span>}
                </span>
                <button type="button" className="text-xs text-orange-700 hover:underline whitespace-nowrap" onClick={() => signInAs(u.username)}>
                  Sign in as this user
                </button>
              </li>
            ))}
          </ul>
          {way === 'key' && <p className="text-xs text-gray-500">Your recovery key still works. Keep it safe.</p>}
        </div>
      )}

      {what === 'username' && !found && (
        <form onSubmit={findUsername} className="space-y-3.5">
          {way === 'windows' ? (
            <p className="text-xs text-gray-600 bg-gray-50 px-3 py-2 rounded-lg">
              Windows will ask you to confirm it's you. Then the usernames of this PC's TallyCheck users are shown.
              This works only on the main PC, for the Windows user that runs TallyCheck.
            </p>
          ) : (
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
                autoFocus
              />
              <p className="text-xs text-gray-500 mt-1.5">Only the username this key belongs to is shown.</p>
            </div>
          )}
          {error && <p className="text-xs text-red-600 bg-red-50 px-3 py-2 rounded-lg">{error}</p>}
          <button
            type="submit"
            disabled={loading}
            className="w-full h-8 px-4 bg-orange-600 border border-orange-700 text-white text-sm rounded hover:bg-orange-700 disabled:opacity-50 disabled:cursor-not-allowed transition-colors"
          >
            {loading ? (way === 'windows' ? 'Waiting for Windows…' : 'Looking up…') : way === 'windows' ? 'Confirm with Windows' : 'Show my username'}
          </button>
        </form>
      )}

      {what === 'password' && (
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
      )}

      <p className="text-xs text-gray-500 mt-4">
        {what === 'username'
          ? 'No recovery key? An administrator can see every username under Administration → Users.'
          : 'No recovery key? Another administrator can reset your password under Administration → Users.'}
        {!windowsOk && !isMac && (what === 'username'
          ? ' On the main PC, the TallyCheck desktop app can also show it after your Windows sign-in.'
          : ' On the main PC, the TallyCheck desktop app can also reset it with your Windows sign-in.')}
      </p>
      <button type="button" onClick={() => navigate('/login')} className="mt-3 text-xs text-orange-700 hover:underline">
        ← Back to sign in
      </button>
    </AuthShell>
  );
}
