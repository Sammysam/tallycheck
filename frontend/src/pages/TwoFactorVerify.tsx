import { useState, useRef, type FormEvent } from 'react';
import { useNavigate } from 'react-router-dom';
import AuthShell from '../components/AuthShell';
import axios from 'axios';
import { useAuth } from '../contexts/AuthContext';

export default function TwoFactorVerify() {
  const { tempToken, completeAuth } = useAuth();
  const [localToken] = useState(() => tempToken);
  const [code, setCode] = useState('');
  const [error, setError] = useState('');
  const [loading, setLoading] = useState(false);
  const navigate = useNavigate();
  const done = useRef(false);

  if (!localToken && !done.current) {
    navigate('/login');
    return null;
  }

  const handleSubmit = async (e: FormEvent) => {
    e.preventDefault();
    setLoading(true);
    setError('');
    try {
      const { data } = await axios.post('/api/auth/2fa/verify',
        { code },
        { headers: { Authorization: `Bearer ${localToken}` } },
      );
      done.current = true;
      completeAuth(data.accessToken);
      navigate('/');
    } catch {
      setError('Invalid code — try again');
    } finally {
      setLoading(false);
    }
  };

  return (
    <AuthShell>
          <div className="mb-8 text-center">
            <h1 className="text-xl font-bold text-gray-900 tracking-tight">Two-Factor Authentication</h1>
            <p className="text-sm text-gray-400 mt-1">Enter the code from your authenticator app</p>
          </div>

          <form onSubmit={handleSubmit} className="space-y-4">
            <div>
              <input
                type="text"
                inputMode="numeric"
                maxLength={6}
                required
                autoFocus
                value={code}
                onChange={(e) => setCode(e.target.value.replace(/\D/g, ''))}
                className="w-full px-3 py-3 text-lg text-center tracking-widest border border-gray-300 rounded-lg focus:outline-none focus:ring-2 focus:ring-orange-500"
                placeholder="000000"
              />
            </div>

            {error && (
              <p className="text-xs text-red-600 bg-red-50 px-3 py-2 rounded-lg">{error}</p>
            )}

            <button
              type="submit"
              disabled={loading || code.length !== 6}
              className="w-full py-3 px-4 bg-orange-600 shadow-sm shadow-orange-600/30 text-white text-sm font-medium rounded-lg hover:bg-orange-700 disabled:opacity-50 disabled:cursor-not-allowed transition-colors flex items-center justify-center gap-2"
            >
              {loading && (
                <span className="w-3.5 h-3.5 border-2 border-white border-t-transparent rounded-full animate-spin" />
              )}
              {loading ? 'Verifying…' : 'Verify & Sign in'}
            </button>

            <button
              type="button"
              onClick={() => navigate('/login')}
              className="w-full text-xs text-gray-400 hover:text-gray-600 transition-colors"
            >
              Back to login
            </button>
          </form>
    </AuthShell>
  );
}
