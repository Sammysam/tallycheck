import { useEffect, useState } from 'react';
import { Link, useSearchParams } from 'react-router-dom';
import { AlertTriangle, Bot, CheckCircle2, ChevronLeft, CreditCard, ExternalLink, Landmark, RefreshCw, ShoppingBag, ShoppingCart } from 'lucide-react';
import ClaudePanel from './ClaudePanel';
import api, { errMsg } from '../../api/client';
import PasswordInput from '../PasswordInput';
import ConfirmDialog from '../ConfirmDialog';
import { useAuth } from '../../contexts/AuthContext';
import { toast } from '../../lib/toast';
import { setUnsaved } from '../../lib/commands';
import { lineCodes, useTaxCodes } from '../../lib/taxes';

/**
 * D39/D40: Settings → Integrations. Shops (Shopify, WooCommerce) bring orders in as draft invoices;
 * payment services (Shopify Payments, PayPal, Stripe) bring payments, fees, refunds and payouts in
 * as lines to match on Bank Transactions. Nothing is posted until someone checks it.
 */

type Id = 'shopify' | 'woocommerce' | 'paypal' | 'stripe';
interface Account { id: string; code: string; name: string; type: string; currency?: string | null; isActive: boolean }
interface Report {
  at: string; since: string;
  orders: { created: number; skipped: number; errors: number };
  lines: { created: number; skipped: number };
  unmappedTaxes: string[]; notes: string[];
  rows: { kind: string; ref: string; status: 'NEW' | 'SKIPPED' | 'ERROR'; message: string }[];
}
interface Status {
  id: Id; label: string; orders: boolean; money: boolean; connected: boolean;
  remoteId?: string | null; remoteName?: string | null; remoteCurrency?: string | null; options: Record<string, string>;
  startDate?: string | null; clearingAccountId?: string | null; payoutAccountId?: string | null; feeAccountId?: string | null;
  incomeAccountId?: string | null; shippingAccountId?: string | null; taxMap: Record<string, string>; autoSync: boolean;
  lastSyncAt?: string | null; lastResult?: Report | null; lastError?: string | null;
}
interface Field { key: string; label: string; placeholder?: string; secret?: boolean; choices?: [string, string][] }

const INFO: Record<Id, { icon: typeof ShoppingBag; color: string; blurb: string; steps: React.ReactNode[]; fields: Field[]; money?: string }> = {
  shopify: {
    icon: ShoppingBag, color: 'text-green-700',
    blurb: 'Orders as draft invoices; Shopify Payments as lines to match.',
    steps: [
      <>In Shopify admin, open <b>Settings → Apps and sales channels → Develop apps</b> and create an app, e.g. “TallyCheck”.</>,
      <>Under <b>Admin API access scopes</b>, tick <b>read_orders</b> and <b>read_shopify_payments_payouts</b> (and <b>read_all_orders</b> for orders older than 60 days); save and <b>Install app</b>.</>,
      <>Copy the <b>Admin API access token</b> (it starts with shpat_) and paste it below.</>,
    ],
    fields: [{ key: 'shop', label: 'Store address', placeholder: 'mystore.myshopify.com' }, { key: 'token', label: 'Admin API access key', placeholder: 'shpat_…', secret: true }],
    money: 'Shopify Payments',
  },
  woocommerce: {
    icon: ShoppingCart, color: 'text-purple-700',
    blurb: 'Paid orders as draft invoices. Payments come through Stripe or PayPal.',
    steps: [
      <>In WordPress, open <b>WooCommerce → Settings → Advanced → REST API</b> and click <b>Add key</b>.</>,
      <>Give it a name (“TallyCheck”), choose your user, set <b>Permissions: Read</b> and click <b>Generate API key</b>.</>,
      <>Copy the <b>consumer key</b> (ck_…) and <b>consumer secret</b> (cs_…) — WooCommerce shows the secret only once.</>,
    ],
    fields: [
      { key: 'site', label: 'Shop address', placeholder: 'https://shop.example.lk' },
      { key: 'consumerKey', label: 'Consumer key', placeholder: 'ck_…', secret: true },
      { key: 'consumerSecret', label: 'Consumer secret', placeholder: 'cs_…', secret: true },
    ],
  },
  paypal: {
    icon: CreditCard, color: 'text-blue-700',
    blurb: 'Payments received, fees, refunds and withdrawals to the bank.',
    steps: [
      <>Sign in at <b>developer.paypal.com</b> with your business account and open <b>Apps & Credentials</b> (Live).</>,
      <>Create an app, then under its features turn on <b>Transaction search</b>.</>,
      <>Copy the app's <b>Client ID</b> and <b>Secret</b>. PayPal shows transactions about three hours after they happen.</>,
    ],
    fields: [
      { key: 'environment', label: 'Account', choices: [['live', 'Live (real money)'], ['sandbox', 'Sandbox (testing)']] },
      { key: 'clientId', label: 'Client ID', placeholder: 'A…', secret: true },
      { key: 'clientSecret', label: 'Secret', placeholder: 'E…', secret: true },
    ],
    money: 'PayPal',
  },
  stripe: {
    icon: Landmark, color: 'text-indigo-700',
    blurb: 'Card payments, Stripe fees, refunds and payouts to the bank.',
    steps: [
      <>In the Stripe Dashboard, open <b>Developers → API keys</b>.</>,
      <>Click <b>Create restricted key</b> with <b>Read</b> access to <b>Balance</b>, <b>Charges</b> and <b>Payouts</b> (or use the secret key).</>,
      <>Copy the key (rk_live_… or sk_live_…) and paste it below.</>,
    ],
    fields: [{ key: 'secretKey', label: 'Restricted or secret key', placeholder: 'rk_live_…', secret: true }],
    money: 'Stripe',
  },
};

/** "VAT|0.18+NBT|0.02" → "VAT 18% + NBT 2%". */
const taxLabel = (key: string) =>
  key.split('+').map((p) => {
    const [title, rate] = p.split('|');
    const pct = Number(rate) * 100;
    return rate && Number.isFinite(pct) ? `${title} ${Math.round(pct * 1000) / 1000}%` : title;
  }).join(' + ');

const STATUS_STYLE = { NEW: 'bg-green-50 text-green-700 border-green-200', SKIPPED: 'bg-gray-50 text-gray-600 border-gray-200', ERROR: 'bg-red-50 text-red-700 border-red-200' };
const STATUS_TEXT = { NEW: 'New', SKIPPED: 'Skipped', ERROR: 'Not brought in' };
const plural = (n: number, word: string) => `${n} ${word}${n === 1 ? '' : 's'}`;

export default function IntegrationsPanel() {
  const [params, setParams] = useSearchParams();
  const open = params.get('connector') as Id | 'claude' | null;
  const [list, setList] = useState<Status[] | null>(null);
  const [error, setError] = useState('');
  const load = () => api.get('/integrations').then((r) => setList(r.data)).catch((e) => setError(errMsg(e, 'Could not load the integrations')));
  useEffect(() => { load(); }, []);

  const choose = (id: Id | 'claude' | null) => {
    const next = new URLSearchParams(params);
    if (id) next.set('connector', id); else next.delete('connector');
    setParams(next, { replace: true });
    if (!id) load();
  };

  if (open === 'claude') return <ClaudePanel onBack={() => choose(null)} />;
  if (open && INFO[open]) return <Connector id={open} onBack={() => choose(null)} />;
  if (!list) return <p className="text-sm text-gray-500">{error || 'Loading…'}</p>;
  return (
    <div className="space-y-3">
      <p className="text-xs text-gray-600">Connect the services you sell and get paid through. The app goes online only while it syncs and only reads — it never changes anything in those services.</p>
      <div className="grid gap-3" style={{ gridTemplateColumns: 'repeat(auto-fill, minmax(260px, 1fr))' }}>
        {list.map((s) => {
          const info = INFO[s.id];
          const Icon = info.icon;
          return (
            <button key={s.id} type="button" onClick={() => choose(s.id)} data-testid={`connector-${s.id}`}
              className="card !mb-0 text-left hover:border-orange-300 transition-colors">
              <div className="flex items-center gap-2 font-semibold text-gray-900"><Icon size={18} className={info.color} /> {s.label}</div>
              <p className="text-xs text-gray-600 mt-1">{info.blurb}</p>
              <div className="text-xs mt-2">
                {s.connected
                  ? <span className="text-green-700 inline-flex items-center gap-1"><CheckCircle2 size={12} /> Connected{s.remoteName ? ` — ${s.remoteName}` : ''}</span>
                  : <span className="text-gray-500">Not connected</span>}
                {s.lastError && <span className="block text-amber-700 mt-0.5">Last sync had a problem</span>}
              </div>
            </button>
          );
        })}
        <button type="button" onClick={() => choose('claude')} data-testid="connector-claude" className="card !mb-0 text-left hover:border-orange-300 transition-colors">
          <div className="flex items-center gap-2 font-semibold text-gray-900"><Bot size={18} className="text-orange-600" /> Claude (AI assistant)</div>
          <p className="text-xs text-gray-600 mt-1">Ask questions about your books; with permission, Claude drafts invoices and bills for you to post.</p>
          <div className="text-xs mt-2 text-gray-500">Keys and setup</div>
        </button>
      </div>
      <p className="text-[11px] text-gray-500">Other banks, shops and systems: use <Link to="/import" className="btn-link">Import from File</Link> with their CSV or Excel export.</p>
    </div>
  );
}

function Connector({ id, onBack }: { id: Id; onBack: () => void }) {
  const info = INFO[id];
  const { hasPermission } = useAuth();
  const canSync = hasPermission('invoices') && hasPermission('banking');
  const [st, setSt] = useState<Status | null>(null);
  const [accounts, setAccounts] = useState<Account[]>([]);
  const taxCodes = lineCodes(useTaxCodes());
  const [creds, setCreds] = useState<Record<string, string>>({ environment: 'live' });
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
  useEffect(() => {
    api.get(`/integrations/${id}`).then((r) => apply(r.data)).catch((e) => setError(errMsg(e, 'Could not load the integration')));
    api.get('/accounts').then((r) => setAccounts(r.data));
  }, [id]);

  const set = (patch: Partial<Status>) => { setForm((f) => ({ ...f, ...patch })); setUnsaved(true); };
  const byType = (t: string, foreignOk = false) => accounts.filter((a) => a.isActive && a.type === t && (foreignOk || !a.currency));

  const connect = async () => {
    setBusy('connect');
    setError('');
    try {
      const r = await api.post(`/integrations/${id}/connect`, creds);
      setCreds({ environment: 'live' });
      apply(r.data);
      toast(`Connected to ${r.data.remoteName ?? r.data.remoteId}.`);
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
      const r = await api.put(`/integrations/${id}`, {
        startDate: blank(form.startDate), clearingAccountId: blank(form.clearingAccountId), payoutAccountId: blank(form.payoutAccountId),
        feeAccountId: blank(form.feeAccountId), incomeAccountId: blank(form.incomeAccountId), shippingAccountId: blank(form.shippingAccountId),
        taxMap: form.taxMap ?? {}, autoSync: !!form.autoSync,
      });
      apply(r.data);
      setUnsaved(false);
      toast(`${st?.label} settings saved.`);
    } catch (e) {
      setError(errMsg(e, 'Could not save'));
    } finally {
      setBusy('');
    }
  };

  const makeClearing = async () => {
    setBusy('clearing');
    try {
      const r = await api.post(`/integrations/${id}/clearing-account`);
      setAccounts((await api.get('/accounts')).data);
      set({ clearingAccountId: r.data.clearingAccountId });
      toast('The money account is ready.');
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
      const r = await api.post(`/integrations/${id}/sync`);
      setReport(r.data);
      setSt((await api.get(`/integrations/${id}`)).data);
      const rep = r.data as Report;
      toast(`${st?.label}: ${[st?.orders ? plural(rep.orders.created, 'new order') : '', st?.money ? plural(rep.lines.created, 'money line') : ''].filter(Boolean).join(', ')}.`);
    } catch (e) {
      setError(errMsg(e, 'The sync did not finish'));
      api.get(`/integrations/${id}`).then((r) => setSt(r.data));
    } finally {
      setBusy('');
    }
  };

  const disconnect = async () => {
    setBusy('off');
    try {
      apply((await api.delete(`/integrations/${id}`)).data);
      toast(`${st?.label} disconnected. Its key was removed from this PC.`);
    } catch (e) {
      toast(errMsg(e, 'Could not disconnect'), 'error');
    } finally {
      setBusy('');
      setConfirmOff(false);
    }
  };

  const back = <button type="button" className="btn-link text-sm inline-flex items-center gap-1 mb-2" onClick={onBack}><ChevronLeft size={14} /> All integrations</button>;
  if (!st) return <div>{back}<p className="text-sm text-gray-500">{error || 'Loading…'}</p></div>;
  const Icon = info.icon;

  const accountSelect = (label: string, key: keyof Status, list: Account[], hint?: string) => (
    <label className="text-xs font-medium text-gray-600">{label}
      <select className="mt-1 w-full" value={(form[key] as string) ?? ''} onChange={(e) => set({ [key]: e.target.value } as Partial<Status>)} aria-label={label}>
        <option value="">— not chosen —</option>
        {list.map((a) => <option key={a.id} value={a.id}>{a.code} – {a.name}{a.currency ? ` (${a.currency})` : ''}</option>)}
      </select>
      {hint && <span className="text-[11px] text-gray-500 font-normal">{hint}</span>}
    </label>
  );
  const taxKeys = Array.from(new Set([...Object.keys(form.taxMap ?? {}), ...(report?.unmappedTaxes ?? [])])).sort();
  const filled = info.fields.every((f) => (creds[f.key] ?? '').trim());

  return (
    <div className="space-y-4">
      {back}
      {error && <div className="px-3 py-2 rounded border border-red-200 bg-red-50 text-sm text-red-700" role="alert">{error}</div>}

      {!st.connected ? (
        <section className="card !mb-0">
          <h3 className="text-sm font-semibold text-gray-800 flex items-center gap-2 mb-1"><Icon size={16} className={info.color} /> Connect {st.label}</h3>
          <p className="text-xs text-gray-600 mb-3">{info.blurb} Nothing is posted until someone checks it.</p>
          <ol className="text-xs text-gray-700 list-decimal pl-5 space-y-1 mb-3">{info.steps.map((s, i) => <li key={i}>{s}</li>)}</ol>
          <div className="grid gap-2" style={{ gridTemplateColumns: `repeat(${info.fields.length}, 1fr) auto` }}>
            {info.fields.map((f) => (
              <label key={f.key} className="text-xs font-medium text-gray-600">{f.label}
                {f.choices ? (
                  <select className="mt-1 w-full" value={creds[f.key] ?? ''} onChange={(e) => setCreds({ ...creds, [f.key]: e.target.value })} aria-label={f.label}>
                    {f.choices.map(([v, l]) => <option key={v} value={v}>{l}</option>)}
                  </select>
                ) : f.secret ? (
                  <div className="mt-1"><PasswordInput value={creds[f.key] ?? ''} onChange={(v) => setCreds({ ...creds, [f.key]: v })} placeholder={f.placeholder} ariaLabel={f.label} /></div>
                ) : (
                  <input className="mt-1 w-full" value={creds[f.key] ?? ''} onChange={(e) => setCreds({ ...creds, [f.key]: e.target.value })} placeholder={f.placeholder} aria-label={f.label} />
                )}
              </label>
            ))}
            <div className="flex items-end"><button className="btn btn-primary" disabled={!filled || busy !== ''} onClick={connect}>{busy === 'connect' ? 'Connecting…' : 'Connect'}</button></div>
          </div>
          <p className="text-[11px] text-gray-500 mt-3">Keys are stored encrypted on this PC and never shown again. The app goes online only while it syncs.</p>
        </section>
      ) : (
        <>
          <section className="card !mb-0 flex flex-wrap items-center gap-3">
            <CheckCircle2 size={18} className="text-green-600" />
            <div className="text-sm">
              <div>Connected to <b>{st.remoteName ?? st.remoteId}</b> <span className="text-gray-500">({st.remoteId}{st.remoteCurrency ? ` · ${st.remoteCurrency}` : ''})</span></div>
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
            <section className="card !mb-0" data-testid="sync-report">
              <h3 className="text-sm font-semibold text-gray-800 mb-2">Last sync</h3>
              <div className="flex flex-wrap gap-2 text-sm mb-2">
                {st.orders && <span className="px-2 py-1 rounded border bg-green-50 border-green-200 text-green-800">{plural(report.orders.created, 'new order')}</span>}
                {st.orders && <span className="px-2 py-1 rounded border bg-gray-50 border-gray-200 text-gray-700">{report.orders.skipped} already in / skipped</span>}
                {report.orders.errors > 0 && <span className="px-2 py-1 rounded border bg-red-50 border-red-200 text-red-700">{report.orders.errors} not brought in</span>}
                {st.money && <span className="px-2 py-1 rounded border bg-blue-50 border-blue-200 text-blue-800">{plural(report.lines.created, 'money line')}</span>}
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
                {st.orders && <Link to="/invoices" className="btn-link">Check and post the draft invoices →</Link>}
                {st.money && st.clearingAccountId && <Link to={`/bank-transactions?accountId=${st.clearingAccountId}`} className="btn-link">Match the money lines →</Link>}
              </div>
            </section>
          )}

          <section className="card !mb-0">
            <h3 className="text-sm font-semibold text-gray-800 mb-1">Where {st.label}'s figures go</h3>
            {st.money && <p className="text-xs text-gray-600 mb-3">{info.money} money sits in its own account until it is paid out to your bank.</p>}
            <div className="grid grid-cols-2 gap-3">
              <label className="text-xs font-medium text-gray-600">Bring in from
                <input className="mt-1 w-full" type="date" value={form.startDate ?? ''} onChange={(e) => set({ startDate: e.target.value })} aria-label="Start date" />
              </label>
              {st.orders && accountSelect('Sales account', 'incomeAccountId', byType('REVENUE'), 'For lines without a matching product (matched by SKU).')}
              {st.orders && accountSelect('Shipping account', 'shippingAccountId', byType('REVENUE'), 'Blank = the sales account.')}
              {st.money && (
                <div>
                  {accountSelect(`${info.money} money account`, 'clearingAccountId', byType('ASSET', true), st.remoteCurrency ? `In ${st.remoteCurrency}, like the ${info.money} balance.` : undefined)}
                  {!form.clearingAccountId && <button type="button" className="btn-link text-xs mt-1" disabled={busy !== ''} onClick={makeClearing}>+ Add a “{info.money === 'Shopify Payments' ? 'Shopify Clearing' : info.money}” account</button>}
                </div>
              )}
              {st.money && accountSelect('Payouts reach', 'payoutAccountId', byType('ASSET'), 'Your bank account.')}
              {st.money && accountSelect(`${st.label} fees`, 'feeAccountId', byType('EXPENSE'))}
            </div>

            {st.orders && (
              <>
                <h4 className="text-xs font-semibold text-gray-700 uppercase tracking-wide mt-4 mb-1">{st.label} taxes</h4>
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
              </>
            )}

            <label className="flex items-center gap-2 text-sm mt-4">
              <input type="checkbox" checked={!!form.autoSync} onChange={(e) => set({ autoSync: e.target.checked })} />
              Sync automatically every hour while the app is open
            </label>
            <div className="flex gap-2 mt-4">
              <button className="btn btn-primary" disabled={busy !== ''} onClick={save}>{busy === 'save' ? 'Saving…' : 'Save'}</button>
            </div>
          </section>
          <p className="text-[11px] text-gray-500 flex items-center gap-1"><ExternalLink size={11} /> The app only reads from {st.label}; it never changes anything there.</p>
        </>
      )}

      {confirmOff && (
        <ConfirmDialog title={`Disconnect ${st.label}`} confirmLabel="Disconnect" danger busy={busy === 'off'} onConfirm={disconnect} onCancel={() => setConfirmOff(false)}>
          <p>Remove the {st.label} key from this PC and stop syncing?</p>
          <p className="text-gray-600">What was already brought in stays. The account matching is kept for when you connect again.</p>
        </ConfirmDialog>
      )}
    </div>
  );
}
