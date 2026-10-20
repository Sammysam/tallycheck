import { useEffect, useState, type ChangeEvent, type FormEvent } from 'react';
import api, { errMsg } from '../api/client';
import { COUNTRIES } from '../lib/countries';
import { regionFromSettings, setRegion } from '../lib/region';

/**
 * D25: the customer's own company and region. Used by the first-run Company setup screen and
 * by Settings → Company. Choosing a country fills in its usual currency, number format,
 * currency words and financial year; every value can be changed.
 */

const FIELDS = [
  'company.name', 'company.address', 'company.phone', 'company.fax', 'company.email', 'company.website',
  'company.taxNumber', 'company.logo', 'company.paymentInstructions', 'company.footerNote',
  'region.country', 'region.currency', 'region.currencySymbol', 'region.currencyWord', 'region.currencyMinorWord',
  'region.locale', 'region.fiscalYearStart',
] as const;
type Key = (typeof FIELDS)[number];
type Form = Record<Key, string>;

const EMPTY: Form = Object.fromEntries(FIELDS.map((k) => [k, ''])) as Form;
const MONTHS = ['January', 'February', 'March', 'April', 'May', 'June', 'July', 'August', 'September', 'October', 'November', 'December'];
const MAX_LOGO_BYTES = 250_000;

function example(form: Form) {
  try {
    return new Intl.NumberFormat(form['region.locale'] || 'en-US', {
      style: 'currency', currency: form['region.currency'] || 'USD', minimumFractionDigits: 2, maximumFractionDigits: 2,
    }).format(1234567.5);
  } catch {
    return 'Check the currency code and number format';
  }
}

export default function CompanyForm({ onSaved, submitLabel = 'Save' }: { onSaved?: () => void; submitLabel?: string }) {
  const [form, setForm] = useState<Form>(EMPTY);
  const [loaded, setLoaded] = useState(false);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState('');
  const [saved, setSaved] = useState(false);

  useEffect(() => {
    api.get('/settings')
      .then((r) => {
        const s = r.data as Record<string, string>;
        const f = { ...EMPTY };
        FIELDS.forEach((k) => { f[k] = s[k] ?? ''; });
        if (!f['region.fiscalYearStart']) f['region.fiscalYearStart'] = '1';
        setForm(f);
      })
      .catch((e) => setError(errMsg(e, 'Could not load the company details')))
      .finally(() => setLoaded(true));
  }, []);

  const set = (k: Key) => (e: ChangeEvent<HTMLInputElement | HTMLTextAreaElement | HTMLSelectElement>) => {
    setSaved(false);
    setForm((f) => ({ ...f, [k]: e.target.value }));
  };

  const chooseCountry = (code: string) => {
    setSaved(false);
    const c = COUNTRIES.find((x) => x.code === code);
    setForm((f) => (c ? {
      ...f,
      'region.country': c.code,
      'region.currency': c.currency,
      'region.currencySymbol': c.symbol,
      'region.currencyWord': c.word,
      'region.currencyMinorWord': c.minorWord,
      'region.locale': c.locale,
      'region.fiscalYearStart': String(c.fiscalYearStart),
    } : { ...f, 'region.country': code }));
  };

  const chooseLogo = (e: ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0];
    e.target.value = '';
    if (!file) return;
    if (!['image/png', 'image/jpeg', 'image/webp'].includes(file.type)) {
      setError('The logo must be a PNG, JPEG or WebP picture.');
      return;
    }
    if (file.size > MAX_LOGO_BYTES) {
      setError(`The logo is ${Math.round(file.size / 1024)} KB; please use a picture under ${MAX_LOGO_BYTES / 1000} KB.`);
      return;
    }
    const reader = new FileReader();
    reader.onload = () => {
      setError('');
      setSaved(false);
      setForm((f) => ({ ...f, 'company.logo': String(reader.result) }));
    };
    reader.readAsDataURL(file);
  };

  const submit = async (e: FormEvent) => {
    e.preventDefault();
    setError('');
    setSaving(true);
    try {
      const body = Object.fromEntries(FIELDS.map((k) => [k, form[k].trim()]));
      const r = await api.patch('/settings', body);
      setRegion(regionFromSettings(r.data));
      setSaved(true);
      onSaved?.();
    } catch (err) {
      setError(errMsg(err, 'Could not save the company details'));
    } finally {
      setSaving(false);
    }
  };

  if (!loaded) return <p className="text-sm text-gray-500">Loading…</p>;

  const field = (k: Key, label: string, props: Record<string, unknown> = {}) => (
    <label className="text-xs font-medium text-gray-600">
      {label}
      <input className="mt-1" value={form[k]} onChange={set(k)} {...props} />
    </label>
  );

  return (
    <form onSubmit={submit} className="space-y-4" aria-label="Company details">
      <fieldset className="grid grid-cols-1 md:grid-cols-2 gap-3">
        <legend className="text-sm font-semibold text-gray-800 mb-2">Company</legend>
        {field('company.name', 'Company name *', { required: true, maxLength: 120, placeholder: 'Your company’s legal name' })}
        {field('company.taxNumber', 'Tax / VAT / GST number', { maxLength: 60, placeholder: 'Printed on invoices' })}
        <label className="text-xs font-medium text-gray-600 md:col-span-2">
          Address
          <textarea className="mt-1" rows={3} maxLength={500} value={form['company.address']} onChange={set('company.address')} placeholder="Street, city, country — one item per line" />
        </label>
        {field('company.phone', 'Phone', { maxLength: 40 })}
        {field('company.fax', 'Fax', { maxLength: 40 })}
        {field('company.email', 'Email', { type: 'email', maxLength: 120 })}
        {field('company.website', 'Website', { maxLength: 120 })}
        <div className="md:col-span-2 flex items-center gap-3">
          <div className="w-40 h-14 border border-dashed border-[#c8c8c8] rounded flex items-center justify-center bg-[#fafafa]">
            {form['company.logo'] ? <img src={form['company.logo']} alt="Company logo" className="max-h-12 max-w-36 object-contain" />
              : <span className="text-xs text-gray-400">No logo</span>}
          </div>
          <label className="btn btn-secondary">
            Choose logo…
            <input type="file" accept="image/png,image/jpeg,image/webp" className="hidden" onChange={chooseLogo} />
          </label>
          {form['company.logo'] && (
            <button type="button" className="btn btn-secondary" onClick={() => setForm((f) => ({ ...f, 'company.logo': '' }))}>Remove</button>
          )}
          <span className="text-xs text-gray-500">PNG, JPEG or WebP, under 250 KB. Shown on invoices and statements.</span>
        </div>
        <label className="text-xs font-medium text-gray-600 md:col-span-2">
          How customers pay you (printed on invoices and statements)
          <textarea className="mt-1" rows={3} maxLength={2000} value={form['company.paymentInstructions']} onChange={set('company.paymentInstructions')} placeholder="e.g. Bank transfer to … account number …" />
        </label>
        <label className="text-xs font-medium text-gray-600 md:col-span-2">
          Footer note on invoices
          <input className="mt-1" maxLength={300} value={form['company.footerNote']} onChange={set('company.footerNote')} placeholder="e.g. Thank you for your business" />
        </label>
      </fieldset>

      <fieldset className="grid grid-cols-1 md:grid-cols-3 gap-3">
        <legend className="text-sm font-semibold text-gray-800 mb-2">Country and currency</legend>
        <label className="text-xs font-medium text-gray-600">
          Country
          <select className="mt-1" value={COUNTRIES.some((c) => c.code === form['region.country']) ? form['region.country'] : ''} onChange={(e) => chooseCountry(e.target.value)} aria-label="Country">
            <option value="">Choose a country…</option>
            {COUNTRIES.map((c) => <option key={c.code} value={c.code}>{c.name}</option>)}
          </select>
        </label>
        {field('region.currency', 'Currency code *', { required: true, pattern: '[A-Z]{3}', maxLength: 3, placeholder: 'e.g. USD' })}
        {field('region.currencySymbol', 'Currency symbol', { maxLength: 8, placeholder: 'e.g. $' })}
        {field('region.currencyWord', 'Currency in words', { maxLength: 30, placeholder: 'e.g. Dollars' })}
        {field('region.currencyMinorWord', 'Cents in words', { maxLength: 30, placeholder: 'e.g. Cents' })}
        {field('region.locale', 'Number format', { required: true, maxLength: 20, placeholder: 'e.g. en-GB' })}
        <label className="text-xs font-medium text-gray-600">
          Financial year starts in
          <select className="mt-1" value={form['region.fiscalYearStart']} onChange={set('region.fiscalYearStart')} aria-label="Financial year starts in">
            {MONTHS.map((m, i) => <option key={m} value={String(i + 1)}>{m}</option>)}
          </select>
        </label>
        <p className="text-xs text-gray-600 md:col-span-2 self-end pb-2">
          Amounts will look like: <strong data-testid="money-example">{example(form)}</strong>
        </p>
      </fieldset>

      {error && <p className="text-sm text-red-600">{error}</p>}
      {saved && <p className="text-sm text-green-700">Company details saved.</p>}
      <button type="submit" className="btn btn-primary" disabled={saving}>{saving ? 'Saving…' : submitLabel}</button>
    </form>
  );
}
