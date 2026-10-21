import { useEffect, useState, type FormEvent } from 'react';
import { useNavigate } from 'react-router-dom';
import api, { errMsg } from '../api/client';
import PageHeader from '../components/PageHeader';
import { useCommand } from '../lib/commands';
import Table from '../components/Table';
import Dialog from '../components/desktop/Dialog';
import { CURRENCIES } from '../lib/countries';
import { region, todayIso } from '../lib/region';

interface Account { id: string; code: string; name: string; type: string; subtype: string | null; currency?: string | null }

const TYPES = ['ASSET', 'LIABILITY', 'EQUITY', 'REVENUE', 'EXPENSE'];
const SUBTYPES: Record<string, string[]> = {
  ASSET: ['current_asset', 'fixed_asset', 'non_current_asset', 'bank'],
  LIABILITY: ['current_liability', 'non_current_liability', 'long_term_loan'],
  EQUITY: ['equity'],
  REVENUE: ['operating_revenue', 'other_income'],
  EXPENSE: ['cogs', 'operating_expense', 'other_expense'],
};
const EMPTY = { code: '', name: '', type: 'ASSET', subtype: '', currency: '' };

export default function Accounts() {
  const navigate = useNavigate();
  const [accounts, setAccounts] = useState<Account[]>([]);
  const [showForm, setShowForm] = useState(false);
  /** D33: the account being edited, or null for a new one. */
  const [editing, setEditing] = useState<Account | null>(null);
  const [form, setForm] = useState(EMPTY);
  const [error, setError] = useState('');
  const [revaluing, setRevaluing] = useState<Account | null>(null);
  const [reval, setReval] = useState({ date: todayIso(), rate: '' });
  const [revalMsg, setRevalMsg] = useState('');
  const [busy, setBusy] = useState(false);

  const load = () => api.get('/accounts').then((r) => setAccounts(r.data));
  useEffect(() => { load(); }, []);

  const openNew = () => { setEditing(null); setForm(EMPTY); setError(''); setShowForm(true); };
  const openEdit = (a: Account) => {
    setEditing(a);
    setForm({ code: a.code, name: a.name, type: a.type, subtype: a.subtype ?? '', currency: a.currency ?? '' });
    setError('');
    setShowForm(true);
    window.scrollTo({ top: 0 });
  };
  const close = () => { setShowForm(false); setEditing(null); setError(''); };
  // Ctrl+N / Esc and the toolbar, via the desktop shell.
  useCommand('new', openNew);
  useCommand('close', close);

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    setError('');
    const body = { ...form, subtype: form.subtype || null, currency: form.currency || null };
    try {
      if (editing) await api.patch(`/accounts/${editing.id}`, body);
      else await api.post('/accounts', body);
      close();
      setForm(EMPTY);
      load();
    } catch (err) {
      setError(errMsg(err, 'Could not save the account'));
    }
  };

  const remove = async (row: Account) => {
    if (!window.confirm(`Delete account "${row.code} – ${row.name}"? This cannot be undone.`)) return;
    try {
      await api.delete(`/accounts/${row.id}`);
      load();
    } catch (err) {
      alert(errMsg(err, 'Failed to delete account'));
    }
  };

  const revalue = async () => {
    if (!revaluing) return;
    setBusy(true);
    setRevalMsg('');
    try {
      const r = await api.post(`/accounts/${revaluing.id}/revalue`, { date: reval.date, exchangeRate: Number(reval.rate) });
      setRevalMsg(r.status === 204 ? 'Already at that rate — nothing to post.' : `Posted ${r.data.entryNumber}.`);
    } catch (err) {
      setRevalMsg(errMsg(err, 'Could not revalue the account'));
    } finally {
      setBusy(false);
    }
  };

  const home = region().currency;
  const canHaveCurrency = form.type === 'ASSET' || form.type === 'LIABILITY';

  return (
    <div>
      <PageHeader
        title="Chart of Accounts"
        action={<button onClick={() => (showForm ? close() : openNew())} className="btn btn-primary">+ New Account</button>}
      />

      {showForm && (
        <div className="card mb-5">
          <h3 className="text-sm font-semibold text-gray-700 mb-3">{editing ? `Edit ${editing.code} – ${editing.name}` : 'New Account'}</h3>
          <form onSubmit={submit} className="form-grid" style={{ gridTemplateColumns: '1fr 2fr' }} aria-label="Account">
            <input required placeholder="Code (e.g. 1000)" value={form.code} onChange={(e) => setForm({ ...form, code: e.target.value })} />
            <input required placeholder="Account Name" value={form.name} onChange={(e) => setForm({ ...form, name: e.target.value })} />
            <select value={form.type} onChange={(e) => setForm({ ...form, type: e.target.value, subtype: '', currency: '' })} aria-label="Account type">
              {TYPES.map((t) => <option key={t}>{t}</option>)}
            </select>
            <input list="account-subtypes" placeholder="Subtype (optional, e.g. current_asset)" value={form.subtype}
              onChange={(e) => setForm({ ...form, subtype: e.target.value })} aria-label="Subtype" />
            <datalist id="account-subtypes">
              {(SUBTYPES[form.type] ?? []).map((s) => <option key={s} value={s} />)}
            </datalist>
            {canHaveCurrency && (
              <label className="text-xs font-medium text-gray-600" style={{ gridColumn: '1 / -1' }}>
                Currency — for a bank account kept in a foreign currency, such as a USD account
                <select className="mt-1" value={form.currency || home} onChange={(e) => setForm({ ...form, currency: e.target.value === home ? '' : e.target.value })} aria-label="Account currency">
                  <option value={home}>{home} (home)</option>
                  {CURRENCIES.filter((c) => c !== home).map((c) => <option key={c} value={c}>{c}</option>)}
                </select>
              </label>
            )}
            {editing && <p className="text-xs text-gray-500" style={{ gridColumn: '1 / -1' }}>An account that already has entries keeps its type and currency.</p>}
            {error && <p className="text-sm text-red-600" role="alert" style={{ gridColumn: '1 / -1' }}>{error}</p>}
            <div className="flex gap-2" style={{ gridColumn: '1 / -1' }}>
              <button type="submit" className="btn btn-primary">{editing ? 'Save Changes' : 'Save Account'}</button>
              <button type="button" onClick={close} className="btn btn-secondary">Cancel</button>
            </div>
          </form>
        </div>
      )}

      <Table
        data={accounts}
        columns={[
          { header: 'Code',     accessor: 'code' },
          { header: 'Name',     accessor: (r) => <button className="btn-link" onClick={() => navigate(`/ledger/account/${r.id}`)}>{r.name}</button> },
          { header: 'Type',     accessor: 'type' },
          { header: 'Subtype',  accessor: (r) => r.subtype ?? '—' },
          { header: 'Currency', accessor: (r) => r.currency ?? home },
          { header: '', accessor: (r) => (
            <span className="flex gap-3">
              <button onClick={() => openEdit(r)} className="btn-link text-xs">Edit</button>
              {r.currency && (
                <button onClick={() => { setRevaluing(r); setRevalMsg(''); setReval({ date: todayIso(), rate: '' }); }} className="btn-link text-xs">Revalue</button>
              )}
              <button onClick={() => remove(r)} className="btn-link text-xs" style={{ color: '#dc2626' }}>Delete</button>
            </span>
          )},
        ]}
      />

      {revaluing && (
        <Dialog
          title={`Revalue ${revaluing.code} ${revaluing.name}`}
          onClose={() => !busy && setRevaluing(null)}
          buttons={
            <>
              <button type="button" className="btn btn-primary" disabled={busy || !(Number(reval.rate) > 0)} onClick={revalue}>{busy ? 'Posting…' : 'Revalue'}</button>
              <button type="button" className="btn btn-secondary" disabled={busy} onClick={() => setRevaluing(null)}>Close</button>
            </>
          }
        >
          <p className="text-sm text-gray-700 mb-3">
            Brings the {home} value of this {revaluing.currency} account in line with its {revaluing.currency} balance at the
            month-end rate. The difference posts to Foreign Exchange Gain/Loss.
          </p>
          <div className="form-grid" style={{ gridTemplateColumns: '1fr 1fr' }}>
            <label className="text-xs font-medium text-gray-600">As of
              <input className="mt-1" type="date" value={reval.date} onChange={(e) => setReval({ ...reval, date: e.target.value })} aria-label="Revaluation date" />
            </label>
            <label className="text-xs font-medium text-gray-600">1 {revaluing.currency} = how many {home}?
              <input className="mt-1" type="number" min="0.00000001" step="any" value={reval.rate} onChange={(e) => setReval({ ...reval, rate: e.target.value })} aria-label="Revaluation rate" />
            </label>
          </div>
          {revalMsg && <p className="text-sm mt-3" role="status">{revalMsg}</p>}
        </Dialog>
      )}
    </div>
  );
}
