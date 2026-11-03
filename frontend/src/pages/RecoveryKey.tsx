import { useEffect, useState, type FormEvent } from 'react';
import { useNavigate } from 'react-router-dom';
import { KeyRound } from 'lucide-react';
import { PasswordInput } from '../components/AuthShell';
import RecoveryKeyCard from '../components/RecoveryKeyCard';
import api, { errMsg } from '../api/client';
import { useAuth } from '../contexts/AuthContext';
import { fmtDate } from '../lib/region';

/** D49: the signed-in user's own password recovery key — see whether one exists, make a new one. */
export default function RecoveryKey() {
  const { user } = useAuth();
  const navigate = useNavigate();
  const [status, setStatus] = useState<{ hasKey: boolean; createdAt: string | null } | null>(null);
  const [password, setPassword] = useState('');
  const [error, setError] = useState('');
  const [loading, setLoading] = useState(false);
  const [newKey, setNewKey] = useState('');

  const load = () =>
    api.get<{ hasKey: boolean; createdAt: string | null }>('/auth/recovery-key').then(({ data }) => setStatus(data)).catch(() => {});
  useEffect(() => { load(); }, []);

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    setError('');
    setLoading(true);
    try {
      const { data } = await api.post<{ recoveryKey: string }>('/auth/recovery-key', { password });
      setPassword('');
      setNewKey(data.recoveryKey);
    } catch (err) {
      setError(errMsg(err, 'The recovery key could not be made'));
    } finally {
      setLoading(false);
    }
  };

  return (
    <div className="max-w-lg">
      <h1 className="text-xl font-semibold text-gray-900 flex items-center gap-2"><KeyRound size={20} className="text-orange-600" /> Recovery key</h1>
      <p className="text-sm text-gray-500 mt-1">
        A recovery key lets you set a new password from the sign-in screen ("Forgot password?") if you ever forget yours.
      </p>

      <div className="mt-5 bg-white border border-gray-200 rounded-lg p-5">
        {newKey ? (
          <RecoveryKeyCard
            recoveryKey={newKey}
            username={user?.username ?? ''}
            doneLabel="Done"
            onDone={() => { setNewKey(''); load(); }}
          />
        ) : (
          <>
            <p className="text-sm text-gray-700">
              {status === null ? 'Checking…'
                : status.hasKey ? <>You have a recovery key{status.createdAt ? <>, made on <b>{fmtDate(status.createdAt.slice(0, 10))}</b></> : null}. Making a new one replaces it.</>
                : <><b>You have no recovery key yet.</b> Make one now, so a forgotten password never locks you out.</>}
            </p>
            <form onSubmit={submit} className="mt-4 space-y-3">
              <div>
                <label className="block text-xs font-medium text-gray-600 mb-1.5">Your current password</label>
                <PasswordInput
                  required
                  value={password}
                  onChange={(e) => setPassword(e.target.value)}
                  className="w-full px-3 py-2 text-sm border border-gray-300 rounded-lg focus:outline-none focus:ring-2 focus:ring-orange-500 focus:border-transparent"
                />
              </div>
              {error && <p className="text-xs text-red-600 bg-red-50 px-3 py-2 rounded-lg">{error}</p>}
              <div className="flex gap-2">
                <button type="submit" disabled={loading} className="btn btn-primary">
                  {loading ? 'Making…' : status?.hasKey ? 'Make a new recovery key' : 'Make my recovery key'}
                </button>
                <button type="button" className="btn btn-secondary" onClick={() => navigate(-1)}>Close</button>
              </div>
            </form>
          </>
        )}
      </div>
    </div>
  );
}
