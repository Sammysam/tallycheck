import { useState, useEffect, useRef, type FormEvent } from 'react';
import { useNavigate } from 'react-router-dom';
import AuthShell from '../components/AuthShell';
import axios from 'axios';
import { useAuth } from '../contexts/AuthContext';
import { signInError } from '../lib/signInError';

export default function TwoFactorSetup() {
  const { tempToken, completeAuth } = useAuth();
  // During sign-in this is the temporary token; a signed-in user turning 2FA on uses their own.
  const [localToken] = useState(() => tempToken ?? localStorage.getItem('accessToken'));
  const optional = !tempToken;
  const [qrCode, setQrCode] = useState('');
  const [secret, setSecret] = useState('');
  const [code, setCode] = useState('');
  const [error, setError] = useState('');
  const [loading, setLoading] = useState(false);
  const [loadingQr, setLoadingQr] = useState(true);
  const navigate = useNavigate();
  const done = useRef(false);

  useEffect(() => {
    if (!localToken) { navigate('/login'); return; }
    axios.get('/api/auth/2fa/setup', {
      headers: { Authorization: `Bearer ${localToken}` },
    })
      .then((res) => {
        setQrCode(res.data.qrCode);
        setSecret(res.data.secret);
      })
      .catch(() => navigate('/login'))
      .finally(() => setLoadingQr(false));
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  const handleSubmit = async (e: FormEvent) => {
    e.preventDefault();
    setLoading(true);
    setError('');
    try {
      const { data } = await axios.post('/api/auth/2fa/enable',
        { code },
        { headers: { Authorization: `Bearer ${localToken}` } },
      );
      done.current = true;
      completeAuth(data.accessToken);
      navigate('/');
    } catch (err) {
      setError(signInError(err, 'code'));
    } finally {
      setLoading(false);
    }
  };

  return (
    <AuthShell>
          <div className="mb-6 text-center">
            <h1 className="text-xl font-bold text-gray-900 tracking-tight">Set up 2FA</h1>
            <p className="text-sm text-gray-400 mt-1">Scan the QR code with Google Authenticator</p>
            {optional && (
              <button type="button" onClick={() => navigate('/')} className="text-xs text-gray-500 hover:text-gray-700 mt-2">
                Not now — back to the app
              </button>
            )}
          </div>

          {loadingQr ? (
            <div className="flex justify-center py-8">
              <div className="w-6 h-6 border-2 border-orange-600 border-t-transparent rounded-full animate-spin" />
            </div>
          ) : (
            <>
              <div className="flex justify-center mb-4">
                <img src={qrCode} alt="QR Code" className="w-48 h-48 border border-gray-200 rounded-xl p-2" />
              </div>

              <div className="mb-5 p-3 bg-gray-50 rounded-lg">
                <p className="text-xs text-gray-500 mb-1">Or enter this key manually:</p>
                <p className="text-xs font-mono text-gray-700 break-all">{secret}</p>
              </div>

              <form onSubmit={handleSubmit} className="space-y-4">
                <div>
                  <label className="block text-xs font-medium text-gray-600 mb-1.5">
                    Enter the 6-digit code from your app
                  </label>
                  <input
                    type="text"
                    inputMode="numeric"
                    maxLength={6}
                    required
                    autoFocus
                    value={code}
                    onChange={(e) => setCode(e.target.value.replace(/\D/g, ''))}
                    className="w-full px-3 py-2 text-sm text-center tracking-widest border border-gray-300 rounded-lg focus:outline-none focus:ring-2 focus:ring-orange-500"
                    placeholder="000000"
                  />
                </div>

                {error && (
                  <p className="text-xs text-red-600 bg-red-50 px-3 py-2 rounded-lg">{error}</p>
                )}

                <button
                  type="submit"
                  disabled={loading || code.length !== 6}
                  className="w-full h-8 px-4 bg-orange-600 border border-orange-700 text-white text-sm rounded hover:bg-orange-700 disabled:opacity-50 disabled:cursor-not-allowed transition-colors flex items-center justify-center gap-2"
                >
                  {loading && (
                    <span className="w-3.5 h-3.5 border-2 border-white border-t-transparent rounded-full animate-spin" />
                  )}
                  {loading ? 'Verifying…' : 'Activate 2FA & Sign in'}
                </button>
              </form>
            </>
          )}
    </AuthShell>
  );
}
