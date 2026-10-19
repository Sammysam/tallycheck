import { useEffect, useState, type FormEvent } from 'react';
import { useNavigate } from 'react-router-dom';
import api, { errMsg } from '../api/client';
import { useAuth } from '../contexts/AuthContext';

type Status = { needsSetup: boolean; canSetUpHere: boolean };

/** First run (D17): create the first administrator. Only offered while the app has no users. */
export default function FirstRunSetup() {
  const [status, setStatus] = useState<Status | null>(null);
  const [username, setUsername] = useState('');
  const [password, setPassword] = useState('');
  const [confirm, setConfirm] = useState('');
  const [error, setError] = useState('');
  const [loading, setLoading] = useState(false);
  const { completeAuth } = useAuth();
  const navigate = useNavigate();

  useEffect(() => {
    api.get<Status>('/auth/first-run')
      .then(({ data }) => {
        if (!data.needsSetup) navigate('/login', { replace: true });
        else setStatus(data);
      })
      .catch(() => setError('Could not reach the app. Close it and open it again.'));
  }, [navigate]);

  const handleSubmit = async (e: FormEvent) => {
    e.preventDefault();
    setError('');
    if (password !== confirm) {
      setError('The two passwords do not match');
      return;
    }
    setLoading(true);
    try {
      const { data } = await api.post<{ accessToken: string }>('/auth/first-run', { username, password });
      completeAuth(data.accessToken);
      navigate('/', { replace: true });
    } catch (err) {
      setError(errMsg(err, 'Could not create the account'));
    } finally {
      setLoading(false);
    }
  };

  const input =
    'w-full px-3 py-2 text-sm border border-gray-300 rounded-lg focus:outline-none focus:ring-2 focus:ring-blue-500 focus:border-transparent';

  return (
    <div className="min-h-screen flex items-center justify-center bg-gray-50">
      <div className="w-full max-w-sm">
        <div className="bg-white rounded-2xl border border-gray-200 shadow-sm p-8">
          <div className="mb-6 text-center">
            <h1 className="text-xl font-bold text-gray-900 tracking-tight">Welcome</h1>
            <p className="text-sm text-gray-400 mt-1">Create the administrator account to get started</p>
          </div>

          {status && !status.canSetUpHere ? (
            <p className="text-sm text-gray-600 bg-gray-50 px-3 py-2 rounded-lg">
              This app has not been set up yet. Create the first administrator on the main PC where the app is installed.
            </p>
          ) : (
            <form onSubmit={handleSubmit} className="space-y-4">
              <div>
                <label className="block text-xs font-medium text-gray-600 mb-1.5">Username</label>
                <input type="text" required minLength={3} value={username} onChange={(e) => setUsername(e.target.value)} className={input} />
              </div>
              <div>
                <label className="block text-xs font-medium text-gray-600 mb-1.5">Password (at least 8 characters)</label>
                <input type="password" required minLength={8} value={password} onChange={(e) => setPassword(e.target.value)} className={input} />
              </div>
              <div>
                <label className="block text-xs font-medium text-gray-600 mb-1.5">Confirm password</label>
                <input type="password" required minLength={8} value={confirm} onChange={(e) => setConfirm(e.target.value)} className={input} />
              </div>
              <p className="text-xs text-gray-400">
                This account can do everything, including payroll and user management. Keep the password safe. You can turn on 2FA after signing in.
              </p>
              {error && <p className="text-xs text-red-600 bg-red-50 px-3 py-2 rounded-lg">{error}</p>}
              <button
                type="submit"
                disabled={loading || !status}
                className="w-full py-2.5 px-4 bg-blue-600 text-white text-sm font-medium rounded-lg hover:bg-blue-700 disabled:opacity-50 disabled:cursor-not-allowed transition-colors"
              >
                {loading ? 'Creating…' : 'Create administrator'}
              </button>
            </form>
          )}
        </div>
      </div>
    </div>
  );
}
