import { useEffect, useRef, useState, type ChangeEvent, type FormEvent } from 'react';
import { useNavigate } from 'react-router-dom';
import api, { errMsg } from '../api/client';
import { COUNTRIES } from '../lib/countries';
import { regionFromSettings, setRegion, SETUP_LATER } from '../lib/region';

/**
 * D25/D31: shown to the administrator after the first sign-in, until the company is entered.
 * The app ships with no company's details. A wizard asks one thing per page; each page can be
 * skipped, and the whole setup can be left for later (it asks again at the next sign-in).
 * Every value can be changed afterwards under Administration → Settings.
 */

type Values = Record<string, string>;

interface Step {
  id: string;
  title: string;
  help: string;
  /** The settings this page saves. */
  keys: string[];
}

const STEPS: Step[] = [
  { id: 'name', title: 'What is your company called?', help: 'Your company’s legal name, as it should print on invoices.', keys: ['company.name'] },
  { id: 'address', title: 'What is the company’s address?', help: 'Street, city, postcode and country — one item per line.', keys: ['company.address'] },
  { id: 'email', title: 'What email address should customers use?', help: 'Printed on invoices and statements.', keys: ['company.email'] },
  { id: 'phone', title: 'What is the company’s phone number?', help: 'Printed on invoices and statements.', keys: ['company.phone'] },
  { id: 'website', title: 'Does the company have a website?', help: 'Printed on invoices. Skip this if you have none.', keys: ['company.website'] },
  {
    id: 'country',
    title: 'Which country is the company in?',
    help: 'This sets your currency, how amounts are written and when your financial year starts.',
    keys: ['region.country', 'region.currency', 'region.currencySymbol', 'region.currencyWord', 'region.currencyMinorWord', 'region.locale', 'region.fiscalYearStart'],
  },
  { id: 'tax', title: 'Is the company registered for tax?', help: 'Enter your VAT / GST / tax number to print it on invoices. Skip this if you are not registered.', keys: ['company.taxNumber'] },
  { id: 'logo', title: 'Add your logo', help: 'Shown on invoices and statements. PNG, JPEG or WebP, under 250 KB.', keys: ['company.logo'] },
  { id: 'payment', title: 'How do customers pay you?', help: 'Printed on invoices and statements — for example your bank and account number.', keys: ['company.paymentInstructions'] },
  { id: 'footer', title: 'A note at the bottom of your invoices', help: 'For example “Thank you for your business”.', keys: ['company.footerNote'] },
  { id: 'review', title: 'All done — check your details', help: 'Click a row to change it. You can change everything later under Administration → Settings.', keys: [] },
];

const SHORT: Record<string, string> = {
  name: 'Company name', address: 'Address', email: 'Email', phone: 'Phone', website: 'Website',
  country: 'Country and currency', tax: 'Tax number', logo: 'Logo', payment: 'How customers pay', footer: 'Invoice note',
};
const MONTHS = ['January', 'February', 'March', 'April', 'May', 'June', 'July', 'August', 'September', 'October', 'November', 'December'];
const MAX_LOGO_BYTES = 250_000;
const EMAIL = /^[^\s@]+@[^\s@]+\.[^\s@]+$/;

function example(v: Values) {
  try {
    return new Intl.NumberFormat(v['region.locale'] || 'en-US', {
      style: 'currency', currency: v['region.currency'] || 'USD', minimumFractionDigits: 2, maximumFractionDigits: 2,
    }).format(1234567.5);
  } catch {
    return 'Check the currency code';
  }
}

export default function CompanySetup() {
  const navigate = useNavigate();
  const [values, setValues] = useState<Values>({});
  const [loaded, setLoaded] = useState(false);
  const [index, setIndex] = useState(0);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState('');
  const box = useRef<HTMLDivElement>(null);
  const step = STEPS[index];
  const last = STEPS.length - 1;

  useEffect(() => {
    api.get('/settings')
      .then((r) => setValues({ 'region.fiscalYearStart': '1', ...(r.data as Values) }))
      .catch((e) => setError(errMsg(e, 'Could not load the company details')))
      .finally(() => setLoaded(true));
  }, []);

  // Each page starts with its first field ready to type in.
  useEffect(() => {
    box.current?.querySelector<HTMLElement>('input:not([type=file]), textarea, select')?.focus();
  }, [index, loaded]);

  const val = (k: string) => values[k] ?? '';
  const set = (k: string) => (e: ChangeEvent<HTMLInputElement | HTMLTextAreaElement | HTMLSelectElement>) =>
    setValues((v) => ({ ...v, [k]: e.target.value }));

  const go = (i: number) => {
    setError('');
    setIndex(Math.max(0, Math.min(last, i)));
  };

  const chooseCountry = (code: string) => {
    const c = COUNTRIES.find((x) => x.code === code);
    setValues((v) => (c ? {
      ...v,
      'region.country': c.code,
      'region.currency': c.currency,
      'region.currencySymbol': c.symbol,
      'region.currencyWord': c.word,
      'region.currencyMinorWord': c.minorWord,
      'region.locale': c.locale,
      'region.fiscalYearStart': String(c.fiscalYearStart),
    } : { ...v, 'region.country': '' }));
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
      setValues((v) => ({ ...v, 'company.logo': String(reader.result) }));
    };
    reader.readAsDataURL(file);
  };

  const finish = () => {
    if (!val('company.name').trim()) {
      setError('The company name is still missing. Add it, or choose “Set up later”.');
      return;
    }
    sessionStorage.removeItem(SETUP_LATER);
    navigate('/', { replace: true });
  };

  const later = () => {
    sessionStorage.setItem(SETUP_LATER, '1');
    navigate('/', { replace: true });
  };

  /** Saves this page's answers, then moves on. */
  const next = async (e?: FormEvent) => {
    e?.preventDefault();
    if (step.id === 'review') {
      finish();
      return;
    }
    const body: Values = {};
    for (const k of step.keys) {
      const v = val(k).trim();
      // Country settings are only sent once chosen; an empty one would be refused.
      if (v || !k.startsWith('region.')) body[k] = v;
    }
    if (step.id === 'name' && !body['company.name']) {
      setError('Enter the company name, or press Skip to leave it for now.');
      return;
    }
    if (step.id === 'email' && body['company.email'] && !EMAIL.test(body['company.email'])) {
      setError('That does not look like an email address.');
      return;
    }
    if (step.id === 'country' && body['region.currency'] && !/^[A-Z]{3}$/.test(body['region.currency'])) {
      setError('The currency code is three capital letters, for example USD.');
      return;
    }
    setSaving(true);
    setError('');
    try {
      const r = await api.patch('/settings', body);
      setValues((v) => ({ ...v, ...(r.data as Values) }));
      setRegion(regionFromSettings(r.data));
      go(index + 1);
    } catch (err) {
      setError(errMsg(err, 'Could not save this'));
    } finally {
      setSaving(false);
    }
  };

  const fieldFor = (s: Step) => {
    switch (s.id) {
      case 'name':
        return <input value={val('company.name')} onChange={set('company.name')} maxLength={120} placeholder="Your company’s legal name" aria-label="Company name" />;
      case 'address':
        return <textarea rows={4} value={val('company.address')} onChange={set('company.address')} maxLength={500} placeholder={'12 Main Street\nCity, postcode\nCountry'} aria-label="Address" />;
      case 'email':
        return <input inputMode="email" value={val('company.email')} onChange={set('company.email')} maxLength={120} placeholder="accounts@yourcompany.com" aria-label="Email" />;
      case 'phone':
        return <input type="tel" value={val('company.phone')} onChange={set('company.phone')} maxLength={40} placeholder="+1 555 123 4567" aria-label="Phone" />;
      case 'website':
        return <input value={val('company.website')} onChange={set('company.website')} maxLength={120} placeholder="www.yourcompany.com" aria-label="Website" />;
      case 'tax':
        return <input value={val('company.taxNumber')} onChange={set('company.taxNumber')} maxLength={60} placeholder="Tax / VAT / GST number" aria-label="Tax number" />;
      case 'payment':
        return <textarea rows={4} value={val('company.paymentInstructions')} onChange={set('company.paymentInstructions')} maxLength={2000} placeholder="Bank transfer to … account number …" aria-label="How customers pay you" />;
      case 'footer':
        return <input value={val('company.footerNote')} onChange={set('company.footerNote')} maxLength={300} placeholder="Thank you for your business" aria-label="Footer note" />;
      case 'logo':
        return (
          <div className="flex items-center gap-3">
            <div className="w-48 h-20 border border-dashed border-[#c8c8c8] rounded flex items-center justify-center bg-[#fafafa]">
              {val('company.logo') ? <img src={val('company.logo')} alt="Company logo" className="max-h-16 max-w-44 object-contain" />
                : <span className="text-xs text-gray-400">No logo</span>}
            </div>
            <label className="btn btn-secondary">
              Choose logo…
              <input type="file" accept="image/png,image/jpeg,image/webp" className="hidden" onChange={chooseLogo} />
            </label>
            {val('company.logo') && (
              <button type="button" className="btn btn-secondary" onClick={() => setValues((v) => ({ ...v, 'company.logo': '' }))}>Remove</button>
            )}
          </div>
        );
      case 'country':
        return (
          <div className="space-y-3">
            <select value={COUNTRIES.some((c) => c.code === val('region.country')) ? val('region.country') : ''} onChange={(e) => chooseCountry(e.target.value)} aria-label="Country">
              <option value="">Choose a country…</option>
              {COUNTRIES.map((c) => <option key={c.code} value={c.code}>{c.name}</option>)}
            </select>
            <div className="grid grid-cols-2 gap-3">
              <label className="text-xs font-medium text-gray-600">
                Currency code
                <input className="mt-1" value={val('region.currency')} onChange={(e) => setValues((v) => ({ ...v, 'region.currency': e.target.value.toUpperCase() }))} maxLength={3} placeholder="e.g. USD" aria-label="Currency code" />
              </label>
              <label className="text-xs font-medium text-gray-600">
                Financial year starts in
                <select className="mt-1" value={val('region.fiscalYearStart') || '1'} onChange={set('region.fiscalYearStart')} aria-label="Financial year starts in">
                  {MONTHS.map((m, i) => <option key={m} value={String(i + 1)}>{m}</option>)}
                </select>
              </label>
            </div>
            <p className="text-xs text-gray-600">Amounts will look like: <strong data-testid="money-example">{example(values)}</strong></p>
          </div>
        );
      default:
        return null;
    }
  };

  const summary = (s: Step): string => {
    if (s.id === 'country') {
      const c = COUNTRIES.find((x) => x.code === val('region.country'));
      return c ? `${c.name} · ${val('region.currency')} · year starts in ${MONTHS[Number(val('region.fiscalYearStart') || 1) - 1]}` : '';
    }
    if (s.id === 'logo') return val('company.logo') ? 'Added' : '';
    return val(s.keys[0]).replace(/\s*\n\s*/g, ', ');
  };

  return (
    <div className="h-screen overflow-auto bg-[#e9ecef] py-10 px-4">
      <div className="max-w-xl mx-auto bg-white border border-[#cfcfcf] rounded-lg shadow-[0_16px_40px_rgba(0,0,0,0.12)] overflow-hidden">
        <div className="auth-panel px-6 py-4 flex items-start justify-between gap-4">
          <div>
            <p className="text-[11px] font-medium uppercase tracking-[0.18em] text-orange-300">Set up your company</p>
            <p className="text-xs text-slate-300 mt-1">Step {index + 1} of {STEPS.length}</p>
          </div>
          <button type="button" className="text-xs text-slate-200 underline hover:text-white" onClick={later}>Set up later</button>
        </div>
        <div className="h-1 bg-[#e5e7eb]">
          <div className="h-1 bg-orange-500 transition-all" style={{ width: `${((index + 1) / STEPS.length) * 100}%` }} />
        </div>

        {!loaded ? <p className="px-6 py-8 text-sm text-gray-500">Loading…</p> : (
          <form onSubmit={next} aria-label="Company setup" className="px-6 py-6">
            <h1 className="text-lg font-semibold text-gray-900">{step.title}</h1>
            <p className="text-sm text-gray-600 mt-1 mb-4">{step.help}</p>

            <div ref={box}>
              {step.id === 'review' ? (
                <table className="w-full text-sm">
                  <tbody>
                    {STEPS.slice(0, last).map((s, i) => (
                      <tr key={s.id} className="border-b border-[#eee] cursor-pointer hover:bg-[#f7f7f7]" onClick={() => go(i)} title="Change">
                        <td className="py-2 pr-3 text-gray-600 whitespace-nowrap align-top">{SHORT[s.id]}</td>
                        <td className="py-2 text-gray-900">{summary(s) || <span className="text-gray-400">Not entered</span>}</td>
                        <td className="py-2 pl-2 text-xs text-orange-600 align-top">Change</td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              ) : fieldFor(step)}
            </div>

            {error && <p className="text-sm text-red-600 mt-3" role="alert">{error}</p>}

            <div className="flex items-center gap-2 mt-6">
              <button type="button" className="btn btn-secondary" disabled={index === 0 || saving} onClick={() => go(index - 1)}>Back</button>
              <span className="flex-1" />
              {step.id !== 'review' && (
                <button type="button" className="btn btn-secondary" disabled={saving} onClick={() => go(index + 1)}>Skip</button>
              )}
              <button type="submit" className="btn btn-primary" disabled={saving}>
                {saving ? 'Saving…' : step.id === 'review' ? 'Finish' : 'Next'}
              </button>
            </div>
          </form>
        )}
      </div>
    </div>
  );
}
