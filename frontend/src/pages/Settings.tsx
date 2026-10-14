import { useEffect, useState } from 'react';
import api from '../api/client';
import PageHeader from '../components/PageHeader';

const emptyForm = { signatoryName: '', signatoryTitle: '' };

export default function Settings() {
  const [form, setForm] = useState(emptyForm);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState('');
  const [saved, setSaved] = useState(false);

  useEffect(() => {
    api.get('/settings')
      .then((r) => setForm({
        signatoryName: r.data.signatoryName ?? '',
        signatoryTitle: r.data.signatoryTitle ?? '',
      }))
      .catch((err) => setError(err?.response?.data?.message ?? 'Failed to load settings'))
      .finally(() => setLoading(false));
  }, []);

  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    setError('');
    setSaved(false);
    setSaving(true);
    try {
      await api.patch('/settings', form);
      setSaved(true);
    } catch (err: any) {
      setError(err?.response?.data?.message ?? 'Failed to save settings');
    } finally {
      setSaving(false);
    }
  };

  return (
    <div>
      <PageHeader title="Settings" />

      {loading ? (
        <div className="card text-sm text-gray-500">Loading…</div>
      ) : (
        <div className="card" style={{ maxWidth: 520 }}>
          <h3 className="text-sm font-semibold text-gray-700 mb-1">Invoice signatory</h3>
          <p className="text-xs text-gray-500 mb-4">
            Printed under “Thanking You” at the bottom of every invoice PDF.
          </p>

          <form onSubmit={submit}>
            <div className="form-grid mb-3" style={{ gridTemplateColumns: '1fr' }}>
              <label className="text-xs font-medium text-gray-600">
                Signatory name
                <input
                  required
                  className="mt-1"
                  placeholder="e.g. Samir Husain"
                  value={form.signatoryName}
                  onChange={(e) => { setSaved(false); setForm({ ...form, signatoryName: e.target.value }); }}
                />
              </label>
              <label className="text-xs font-medium text-gray-600">
                Signatory title
                <input
                  required
                  className="mt-1"
                  placeholder="e.g. Director"
                  value={form.signatoryTitle}
                  onChange={(e) => { setSaved(false); setForm({ ...form, signatoryTitle: e.target.value }); }}
                />
              </label>
            </div>

            {error && <p className="text-sm text-red-600 mb-2">{error}</p>}
            {saved && <p className="text-sm text-green-600 mb-2">Settings saved.</p>}

            <button type="submit" className="btn btn-primary" disabled={saving}>
              {saving ? 'Saving…' : 'Save'}
            </button>
          </form>
        </div>
      )}
    </div>
  );
}
