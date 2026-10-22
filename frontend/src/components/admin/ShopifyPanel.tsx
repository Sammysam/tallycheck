import { useEffect, useState } from 'react';
import { Link } from 'react-router-dom';
import { AlertTriangle, CheckCircle2, ExternalLink, RefreshCw, ShoppingBag } from 'lucide-react';
import api, { errMsg } from '../../api/client';
import PasswordInput from '../PasswordInput';
import ConfirmDialog from '../ConfirmDialog';
import { useAuth } from '../../contexts/AuthContext';
import { toast } from '../../lib/toast';
import { setUnsaved } from '../../lib/commands';
import { lineCodes, useTaxCodes } from '../../lib/taxes';

/**
 * D39: the Shopify connector. Connect with the store address and an Admin API access key, match
 * Shopify's money to the app's accounts and taxes, then Sync now: orders arrive as draft
 * invoices, Shopify Payments as lines to match on Bank Transactions.
 */

interface Account { id: string; code: string; name: string; type: string; currency?: string | null; isActive: boolean }
interface Report {
  at: string; since: string;
  orders: { created: number; skipped: number; errors: number };
  lines: { created: number; skipped: number };
  unmappedTaxes: string[]; notes: string[];
  rows: { kind: string; ref: string; status: 'NEW' | 'SKIPPED' | 'ERROR'; message: string }[];
}
interface Status {
  connected: boolean; shop?: string | null; shopName?: string | null; shopCurrency?: string | null; apiVersion: string;
  startDate?: string | null; clearingAccountId?: string | null; payoutAccountId?: string | null; feeAccountId?: string | null;
  incomeAccountId?: string | null; shippingAccountId?: string | null; taxMap: Record<string, string>; autoSync: boolean;
  lastSyncAt?: string | null; lastResult?: Report | null; lastError?: string | null;
}

/** "VAT|0.18+NBT|0.02" → "VAT 18% + NBT 2%". */
const taxLabel = (key: string) =>
  key.split('+').map((p) => {
    const [title, rate] = p.split('|');
    const pct = Number(rate) * 100;
    return Number.isFinite(pct) && rate !== '' ? `${title} ${Math.round(pct * 1000) / 1000}%` : title;
  }).join(' + ');

const STATUS_STYLE = { NEW: 'bg-green-50 text-green-700 border-green-200', SKIPPED: 'bg-gray-50 text-gray-600 border-gray-200', ERROR: 'bg-red-50 text-red-700 border-red-200' };
const STATUS_TEXT = { NEW: 'New', SKIPPED: 'Skipped', ERROR: 'Not brought in' };

export default function ShopifyPanel() {
  const { hasPermission } = useAuth();
  const canSync = hasPermission('invoices') && hasPermission('banking');
  const [st, setSt] = useState<Status | null>(null);
  const [accounts, setAccounts] = useState<Account[]>([]);
  const taxCodes = lineCodes(useTaxCodes());
  const [shop, setShop] = useState('');
  const [token, setToken] = useState('');
  const [form, setForm] = useState<Partial<Status>>({});
  const [busy, setBusy] = useState<'' | 'connect' | 'save' | 'sync' | 'clearing' | 'off'>('');
  const [error, setError] = useState('');
  const [confirmOff, setConfirmOff] = useState(false);
  const [report, setReport] = useState<Report | null>(null);

  const apply = (s: Status) => {
    setSt(s);
    setForm({
      startDate: s.startDate ?? '', clearingAccountId: s.clearingAccountId ?? '', payoutAccountId: s.payoutAccountId ?? '',
      feeAccountId: s.feeAccountId ?? '', incomeAccountId: s.incomeAccountId ?? '', shippingAccountId: s.shippingAccountId ?? '',
      taxMap: { ...s.taxMap }, autoSync: s.autoSync,
    });
    setReport(s.lastResult ?? null);
  };
  const load = () => api.get('/integrations/shopify').then((r) => apply(r.data)).catch((e) => setError(errMsg(e, 'Could not load the integration')));
  useEffect(() => {
    load();
    api.get('/accounts').then((r) => setAccounts(r.data));
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  const set = (patch: Partial<Status>) => { setForm((f) => ({ ...f, ...patch })); setUnsaved(true); };
  const byType = (t: string) => accounts.filter((a) => a.isActive && a.type === t && !a.currency);

  const connect = async () => {
    setBusy('connect');
    setError('');
    try {
      const r = await api.post('/integrations/shopify/connect', { shop, token });
      setToken('');
      apply(r.data);
      toast(`Connected to ${r.data.shopName ?? r.data.shop}.`);
    } catch (e) {
      setError(errMsg(e, 'Could not connect'));
    } finally {
      setBusy('');
    }
  };

  const save = async () => {
    setBusy('save');
    setError('');
    const blank = (v?: string | null) => (v ? v : null);
    try {
      const r = await api.put('/integrations/shopify', {
        startDate: blank(form.startDate), clearingAccountId: blank(form.clearingAccountId), payoutAccountId: blank(form.payoutAccountId),
        feeAccountId: blank(form.feeAccountId), incomeAccountId: blank(form.incomeAccountId), shippingAccountId: blank(form.shippingAccountId),
        taxMap: form.taxMap ?? {}, autoSync: !!form.autoSync,
      });
      apply(r.data);
      setUnsaved(false);
      toast('Shopify settings saved.');
    } catch (e) {
      setError(errMsg(e, 'Could not save'));
    } finally {
      setBusy('');
    }
  };

  const makeClearing = async () => {
    setBusy('clearing');
    try {
      const r = await api.post('/integrations/shopify/clearing-account');
      const a = await api.get('/accounts');
      setAccounts(a.data);
      set({ clearingAccountId: r.data.clearingAccountId });
      toast('Account 1090 Shopify Clearing is ready.');
    } catch (e) {
      toast(errMsg(e, 'Could not add the account'), 'error');
    } finally {
      setBusy('');
    }
  };

  const sync = async () => {
    setBusy('sync');
    setError('');
    try {
      const r = await api.post('/integrations/shopify/sync');
      setReport(r.data);
      const s = await api.get('/integrations/shopify');
      setSt(s.data);
      const { orders, lines } = r.data as Report;
      toast(`Shopify: ${orders.created} new order${orders.created === 1 ? '' : 's'}, ${lines.created} payment line${lines.created === 1 ? '' : 's'}.`);
    } catch (e) {
      setError(errMsg(e, 'The sync did not finish'));
      load();
    } finally {
      setBusy('');
    }
  };

  const disconnect = async () => {
    setBusy('off');
    try {
      const r = await api.delete('/integrations/shopify');
      apply(r.data);
      toast('Shopify disconnected. The access key was removed from this PC.');
    } catch (e) {
      toast(errMsg(e, 'Could not disconnect'), 'error');
    } finally {
      setBusy('');
      setConfirmOff(false);
    }
  };

  if (!st) return <p className="text-sm text-gray-500">{error || 'Loading…'}</p>;

  const accountSelect = (label: string, key: keyof Status, list: Account[], hint?: string) => (
    <label className="text-xs font-medium text-gray-600">{label}
      <select className="mt-1 w-full" value={(form[key] as string) ?? ''} onChange={(e) => set({ [key]: e.target.value } as Partial<Status>)} aria-label={label}>
        <option value="">— not chosen —</option>
        {list.map((a) => <option key={a.id} value={a.id}>{a.code} – {a.name}</option>)}
      </select>
      {hint && <span className="text-[11px] text-gray-500 font-normal">{hint}</span>}
    </label>
  );
  const taxKeys = Array.from(new Set([...Object.keys(form.taxMap ?? {}), ...(report?.unmappedTaxes ?? [])])).sort();

  return (
    <div className="space-y-4">
      {error && <div className="px-3 py-2 rounded border border-red-200 bg-red-50 text-sm text-red-700" role="alert">{error}</div>}

      {!st.connected ? (
        <section className="card !mb-0">
          <h3 className="text-sm font-semibold text-gray-800 flex items-center gap-2 mb-1"><ShoppingBag size={16} className="text-green-700" /> Connect your Shopify store</h3>
          <p className="text-xs text-gray-600 mb-3">Orders come in as draft invoices and Shopify Payments as lines to match. Nothing is posted until someone checks it.</p>
          <ol className="text-xs text-gray-700 list-decimal pl-5 space-y-1 mb-3">
            <li>In Shopify admin, open <b>Settings → Apps and sales channels → Develop apps</b> and create an app, e.g. “Cenango Accounting”.</li>
            <li>Under <b>Admin API access scopes</b>, tick <b>read_orders</b> and <b>read_shopify_payments_payouts</b> (and <b>read_all_orders</b> to bring in orders older than 60 days); save and <b>Install app</b>.</li>
            <li>Copy the <b>Admin API access token</b> (it starts with shpat_) and paste it below.</li>
          </ol>
          <div className="grid gap-2" style={{ gridTemplateColumns: '1fr 1fr auto' }}>
            <label className="text-xs font-medium text-gray-600">Store address
              <input className="mt-1 w-full" value={shop} onChange={(e) => setShop(e.target.value)} placeholder="mystore.myshopify.com" aria-label="Store address" />
            </label>
            <label className="text-xs font-medium text-gray-600">Admin API access key
              <div className="mt-1"><PasswordInput value={token} onChange={setToken} placeholder="shpat_…" ariaLabel="Access key" /></div>
            </label>
            <div className="flex items-end"><button className="btn btn-primary" disabled={!shop.trim() || !token.trim() || busy !== ''} onClick={connect}>{busy === 'connect' ? 'Connecting…' : 'Connect'}</button></div>
          </div>
          <p className="text-[11px] text-gray-500 mt-3">The key is stored encrypted on this PC and is never shown again. The app goes online only while it syncs.</p>
        </section>
      ) : (
        <>
          <section className="card !mb-0 flex flex-wrap items-center gap-3">
            <CheckCircle2 size={18} className="text-green-600" />
            <div className="text-sm">
              <div>Connected to <b>{st.shopName ?? st.shop}</b> <span className="text-gray-500">({st.shop}{st.shopCurrency ? ` · ${st.shopCurrency}` : ''})</span></div>
              <div className="text-xs text-gray-500">
                {st.lastSyncAt ? `Last synced ${new Date(st.lastSyncAt).toLocaleString()}` : 'Not synced yet'}
                {st.autoSync && ' · syncs every hour while the app is open'}
              </div>
            </div>
            <div className="ml-auto flex gap-2">
              {canSync && <button className="btn btn-primary inline-flex items-center gap-1.5" disabled={busy !== ''} onClick={sync}><RefreshCw size={14} className={busy === 'sync' ? 'animate-spin' : ''} /> {busy === 'sync' ? 'Syncing…' : 'Sync now'}</button>}
              <button className="btn btn-secondary" disabled={busy !== ''} onClick={() => setConfirmOff(true)}>Disconnect</button>
            </div>
          </section>
          {st.lastError && <div className="px-3 py-2 rounded border border-amber-200 bg-amber-50 text-xs text-amber-800 flex gap-2"><AlertTriangle size={14} /> Last problem: {st.lastError}</div>}

          {report && (
            <section className="card !mb-0" data-testid="shopify-report">
              <h3 className="text-sm font-semibold text-gray-800 mb-2">Last sync</h3>
              <div className="flex flex-wrap gap-2 text-sm mb-2">
                <span className="px-2 py-1 rounded border bg-green-50 border-green-200 text-green-800">{report.orders.created} new order{report.orders.created === 1 ? '' : 's'}</span>
                <span className="px-2 py-1 rounded border bg-gray-50 border-gray-200 text-gray-700">{report.orders.skipped} already in / cancelled</span>
                {report.orders.errors > 0 && <span className="px-2 py-1 rounded border bg-red-50 border-red-200 text-red-700">{report.orders.errors} not brought in</span>}
                <span className="px-2 py-1 rounded border bg-blue-50 border-blue-200 text-blue-800">{report.lines.created} payment line{report.lines.created === 1 ? '' : 's'}</span>
              </div>
              {report.notes.map((n) => <p key={n} className="text-xs text-amber-800 mb-1">{n}</p>)}
              {report.rows.length > 0 && (
                <div className="max-h-[260px] overflow-auto border rounded mt-2">
                  <table className="min-w-full text-sm">
                    <tbody>
                      {report.rows.map((r, i) => (
                        <tr key={i} className="border-t border-[#f3f3f3] first:border-0">
                          <td className="px-2 py-1 whitespace-nowrap font-medium">{r.ref}</td>
                          <td className="px-2"><span className={`text-[11px] px-1.5 py-0.5 rounded border ${STATUS_STYLE[r.status]}`}>{STATUS_TEXT[r.status]}</span></td>
                          <td className={`px-2 text-xs ${r.status === 'ERROR' ? 'text-red-700' : 'text-gray-700'}`}>{r.message}</td>
                        </tr>
                      ))}
                    </tbody>
                  </table>
                </div>
              )}
              <div className="flex gap-3 mt-3 text-sm">
                <Link to="/invoices" className="btn-link">Check and post the draft invoices →</Link>
                {st.clearingAccountId && <Link to={`/bank-transactions?accountId=${st.clearingAccountId}`} className="btn-link">Match the payments →</Link>}
              </div>
            </section>
          )}

          <section className="card !mb-0">
            <h3 className="text-sm font-semibold text-gray-800 mb-1">Where Shopify's money goes</h3>
            <p className="text-xs text-gray-600 mb-3">Shopify Payments money passes through a clearing account until it is paid out to your bank.</p>
            <div className="grid grid-cols-2 gap-3">
              <label className="text-xs font-medium text-gray-600">Bring in orders from
                <input className="mt-1 w-full" type="date" value={form.startDate ?? ''} onChange={(e) => set({ startDate: e.target.value })} aria-label="Start date" />
              </label>
              {accountSelect('Sales account', 'incomeAccountId', byType('REVENUE'), 'For lines without a matching product (matched by SKU).')}
              {accountSelect('Shipping account', 'shippingAccountId', byType('REVENUE'), 'Blank = the sales account.')}
              <div>
                {accountSelect('Shopify clearing account', 'clearingAccountId', byType('ASSET'))}
                {!form.clearingAccountId && <button type="button" className="btn-link text-xs mt-1" disabled={busy !== ''} onClick={makeClearing}>+ Add “1090 Shopify Clearing”</button>}
              </div>
              {accountSelect('Payouts reach', 'payoutAccountId', byType('ASSET'), 'Your bank account.')}
              {accountSelect('Shopify fees', 'feeAccountId', byType('EXPENSE'))}
            </div>

            <h4 className="text-xs font-semibold text-gray-700 uppercase tracking-wide mt-4 mb-1">Shopify taxes</h4>
            {taxKeys.length === 0
              ? <p className="text-xs text-gray-500">Taxes found on orders appear here after a sync, to be matched to your tax codes.</p>
              : (
                <table className="text-sm">
                  <tbody>
                    {taxKeys.map((k) => (
                      <tr key={k}>
                        <td className="pr-3 py-1">{taxLabel(k)}{!(form.taxMap ?? {})[k] && <span className="ml-2 text-[11px] text-amber-700">not matched</span>}</td>
                        <td className="py-1">
                          <select value={(form.taxMap ?? {})[k] ?? ''} aria-label={`Tax code for ${taxLabel(k)}`}
                            onChange={(e) => set({ taxMap: { ...(form.taxMap ?? {}), [k]: e.target.value } })}>
                            <option value="">Choose a tax code…</option>
                            {taxCodes.map((c) => <option key={c.id} value={c.id}>{c.name}</option>)}
                          </select>
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              )}

            <label className="flex items-center gap-2 text-sm mt-4">
              <input type="checkbox" checked={!!form.autoSync} onChange={(e) => set({ autoSync: e.target.checked })} />
              Sync automatically every hour while the app is open
            </label>
            <div className="flex gap-2 mt-4">
              <button className="btn btn-primary" disabled={busy !== ''} onClick={save}>{busy === 'save' ? 'Saving…' : 'Save'}</button>
            </div>
          </section>
          <p className="text-[11px] text-gray-500 flex items-center gap-1"><ExternalLink size={11} /> Data is read from Shopify only; the app never changes anything in your store.</p>
        </>
      )}

      {confirmOff && (
        <ConfirmDialog title="Disconnect Shopify" confirmLabel="Disconnect" danger busy={busy === 'off'} onConfirm={disconnect} onCancel={() => setConfirmOff(false)}>
          <p>Remove the access key from this PC and stop syncing?</p>
          <p className="text-gray-600">Invoices and bank lines already brought in stay. The account matching is kept for when you connect again.</p>
        </ConfirmDialog>
      )}
    </div>
  );
}
