import { useEffect, useState, type FormEvent } from 'react';
import { Link, useSearchParams } from 'react-router-dom';
import { Building2, CalendarCheck, FileText, Globe2, HardDrive, Percent, Plug, Wand2 } from 'lucide-react';
import api, { errMsg } from '../api/client';
import PageHeader from '../components/PageHeader';
import BooksLock from '../components/BooksLock';
import CompanyForm from '../components/CompanyForm';
import BackupPanel from '../components/admin/BackupPanel';
import ShopifyPanel from '../components/admin/ShopifyPanel';
import ConfirmDialog from '../components/ConfirmDialog';
import { isUnsaved, setUnsaved } from '../lib/commands';
import { toast } from '../lib/toast';

/** D36: Settings as tabs down the side, each saving on its own. */

const TABS = [
  { id: 'company', label: 'Company', icon: Building2, help: 'Name, address, contact details, tax number and logo.' },
  { id: 'region', label: 'Region & currency', icon: Globe2, help: 'Country, currency, how amounts are written and when the financial year starts.' },
  { id: 'documents', label: 'Invoices & documents', icon: FileText, help: 'What prints on invoices and statements: payment details, footer and signatory.' },
  { id: 'closing', label: 'Closing', icon: CalendarCheck, help: 'Lock a closed period so nothing more is posted to it.' },
  { id: 'backup', label: 'Backup & restore', icon: HardDrive, help: 'Encrypted backups, the daily backup, and restoring from a backup.' },
  { id: 'integrations', label: 'Integrations', icon: Plug, help: 'Connect Shopify: orders arrive as draft invoices, Shopify Payments as lines to match.' },
] as const;
type TabId = (typeof TABS)[number]['id'];

function Signatory() {
  const [form, setForm] = useState({ signatoryName: '', signatoryTitle: '' });
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState('');
  const [saved, setSaved] = useState(false);

  useEffect(() => {
    api.get('/settings')
      .then((r) => setForm({ signatoryName: r.data.signatoryName ?? '', signatoryTitle: r.data.signatoryTitle ?? '' }))
      .catch((err) => setError(errMsg(err, 'Failed to load settings')))
      .finally(() => setLoading(false));
  }, []);

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    setError('');
    setSaved(false);
    setSaving(true);
    try {
      await api.patch('/settings', form);
      setSaved(true);
      setUnsaved(false);
      toast('Signatory saved.');
    } catch (err) {
      setError(errMsg(err, 'Failed to save settings'));
    } finally {
      setSaving(false);
    }
  };
  const change = (k: 'signatoryName' | 'signatoryTitle', v: string) => { setSaved(false); setUnsaved(true); setForm({ ...form, [k]: v }); };

  if (loading) return <p className="text-sm text-gray-500">Loading…</p>;
  return (
    <form onSubmit={submit}>
      <div className="grid gap-3 mb-3" style={{ gridTemplateColumns: '1fr 1fr' }}>
        <label className="text-xs font-medium text-gray-600">Signatory name
          <input required className="mt-1" placeholder="e.g. Jane Smith" value={form.signatoryName} onChange={(e) => change('signatoryName', e.target.value)} />
        </label>
        <label className="text-xs font-medium text-gray-600">Signatory title
          <input required className="mt-1" placeholder="e.g. Director" value={form.signatoryTitle} onChange={(e) => change('signatoryTitle', e.target.value)} />
        </label>
      </div>
      {error && <p className="text-sm text-red-600 mb-2">{error}</p>}
      {saved && <p className="text-sm text-green-600 mb-2">Settings saved.</p>}
      <button type="submit" className="btn btn-primary" disabled={saving}>{saving ? 'Saving…' : 'Save'}</button>
    </form>
  );
}

function Section({ title, help, children }: { title: string; help?: string; children: React.ReactNode }) {
  return (
    <section className="card">
      <h3 className="text-sm font-semibold text-gray-800">{title}</h3>
      {help && <p className="text-xs text-gray-500 mt-0.5 mb-4">{help}</p>}
      {children}
    </section>
  );
}

export default function Settings() {
  const [params, setParams] = useSearchParams();
  const tab = (TABS.find((t) => t.id === params.get('tab'))?.id ?? 'company') as TabId;
  const [pending, setPending] = useState<TabId | null>(null);
  const current = TABS.find((t) => t.id === tab)!;

  const go = (id: TabId) => {
    if (id === tab) return;
    if (isUnsaved()) { setPending(id); return; }
    setParams({ tab: id }, { replace: true });
  };
  // Leaving Settings with nothing typed: clear any stale flag from this page.
  useEffect(() => () => setUnsaved(false), []);

  return (
    <div>
      <PageHeader title="Settings" subtitle="Your company, region, documents, closing and backups." />
      <div className="flex gap-5 items-start">
        <nav className="w-56 shrink-0 card !p-1.5" aria-label="Settings sections">
          {TABS.map((t) => {
            const Icon = t.icon;
            const on = t.id === tab;
            return (
              <button key={t.id} type="button" onClick={() => go(t.id)} role="tab" aria-selected={on}
                className={`w-full flex items-center gap-2.5 px-3 py-2 rounded text-sm text-left ${on ? 'bg-orange-50 text-orange-700 font-medium' : 'text-gray-700 hover:bg-gray-50'}`}>
                <Icon size={16} /> {t.label}
              </button>
            );
          })}
          <div className="border-t border-[#eee] mt-1.5 pt-1.5">
            <Link to="/taxes" className="w-full flex items-center gap-2.5 px-3 py-2 rounded text-sm text-gray-700 hover:bg-gray-50"><Percent size={16} /> Taxes</Link>
            <Link to="/company-setup" className="w-full flex items-center gap-2.5 px-3 py-2 rounded text-sm text-gray-700 hover:bg-gray-50"><Wand2 size={16} /> Setup wizard</Link>
          </div>
        </nav>

        <div className="flex-1 min-w-0" style={{ maxWidth: 900 }}>
          <div className="mb-3">
            <h2 className="text-base font-semibold text-gray-900">{current.label}</h2>
            <p className="text-xs text-gray-500">{current.help}</p>
          </div>
          {tab === 'company' && <Section title="Company details"><CompanyForm part="company" /></Section>}
          {tab === 'region' && <Section title="Country and currency"><CompanyForm part="region" /></Section>}
          {tab === 'documents' && (
            <div className="space-y-4">
              <Section title="Printed on invoices and statements"><CompanyForm part="documents" /></Section>
              <Section title="Invoice signatory" help="Printed under “Thanking You” at the bottom of every invoice PDF."><Signatory /></Section>
            </div>
          )}
          {tab === 'closing' && <BooksLock />}
          {tab === 'backup' && <BackupPanel />}
          {tab === 'integrations' && <ShopifyPanel />}
        </div>
      </div>

      {pending && (
        <ConfirmDialog title="Unsaved changes" confirmLabel="Leave without saving" danger
          onConfirm={() => { setUnsaved(false); setParams({ tab: pending }, { replace: true }); setPending(null); }}
          onCancel={() => setPending(null)}>
          <p>You have changes on this tab that are not saved. Leave them?</p>
        </ConfirmDialog>
      )}
    </div>
  );
}
