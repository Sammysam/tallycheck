import { useEffect, useRef, useState, type ChangeEvent, type FormEvent } from 'react';
import { useNavigate } from 'react-router-dom';
import api, { errMsg } from '../api/client';
import { COUNTRIES, offeredCountries } from '../lib/countries';
import { toast } from '../lib/toast';
import { syncPayrollCountry } from '../lib/usPayroll';
import { regionFromSettings, setRegion, SETUP_LATER } from '../lib/region';
import { NO_OPENING_BALANCES } from '../components/GettingStarted';

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
  // D73: keep the books safe, and bring in what an existing business already has.
  { id: 'backup', title: 'Keep your books safe', help: 'TallyCheck copies your books to a folder every day, protected by a password. The folder below works; a USB drive or a folder that syncs to the cloud is safer, because it survives this PC breaking. The first copy is made as soon as you press Next.', keys: [] },
  { id: 'opening', title: 'Do you already have a business running?', help: 'If you do, bring in what you have today — your bank balance, what customers owe you and what you owe — so your reports start from the right figures.', keys: [] },
  { id: 'review', title: 'All done — check your details', help: 'Click a row to change it. You can change everything later under Administration → Settings.', keys: [] },
];

const SHORT: Record<string, string> = {
  name: 'Company name', address: 'Address', email: 'Email', phone: 'Phone', website: 'Website',
  country: 'Country and currency', tax: 'Tax number', logo: 'Logo', payment: 'How customers pay', footer: 'Invoice note',
  backup: 'Automatic backups', opening: 'Opening balances',
};

/** D73: what the backup page shows and saves. */
interface BackupState { available: boolean; passwordSet: boolean; enabled: boolean; folder: string; canChooseFolder: boolean }
const read = (k: string) => { try { return localStorage.getItem(k); } catch { return null; } };
const write = (k: string, v: string | null) => { try { if (v === null) localStorage.removeItem(k); else localStorage.setItem(k, v); } catch { /* private window */ } };
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
  // The country saved before (an existing company keeps it in the list, D57).
  const [savedCountry, setSavedCountry] = useState<string | undefined>(undefined);
  const step = STEPS[index];
  const last = STEPS.length - 1;
  // D73: the backup page.
  const [backup, setBackup] = useState<BackupState>({ available: false, passwordSet: false, enabled: false, folder: '', canChooseFolder: false });
  const [backupPassword, setBackupPassword] = useState('');
  const [backupConfirm, setBackupConfirm] = useState('');
  const [daily, setDaily] = useState(true);
  // D73: 'new' business, 'existing' (go on to Quick start), or not answered.
  const [opening, setOpening] = useState<'' | 'new' | 'existing'>(read(NO_OPENING_BALANCES) === '1' ? 'new' : '');

  useEffect(() => {
    api.get('/settings')
      .then((r) => {
        setSavedCountry((r.data as Values)['region.country'] || undefined);
        setValues({ 'region.fiscalYearStart': '1', ...(r.data as Values) });
      })
      .catch((e) => setError(errMsg(e, 'Could not load the company details')))
      .finally(() => setLoaded(true));
    api.get('/system/backup')
      .then((r) => setBackup({
        available: true,
        passwordSet: !!r.data.passwordSet,
        enabled: !!r.data.settings?.enabled,
        folder: r.data.settings?.folder || r.data.defaultFolder || '',
        canChooseFolder: !!r.data.canChooseFolder,
      }))
      .catch(() => setBackup((b) => ({ ...b, available: false })));
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
    // D73: an existing business goes straight on to enter its opening balances.
    navigate(opening === 'existing' ? '/move-books' : '/', { replace: true });
  };

  /** D73: sets the backup password (first time) and turns daily backups on. */
  const saveBackup = async (): Promise<boolean> => {
    if (!backup.available) return true;
    if (!backup.folder.trim()) { setError('Choose a folder for the backups, or press Skip.'); return false; }
    if (!backup.passwordSet) {
      if (backupPassword.length < 8) { setError('The backup password needs at least 8 characters.'); return false; }
      if (backupPassword !== backupConfirm) { setError('The two backup passwords are not the same.'); return false; }
      await api.post('/system/backup/password', { password: backupPassword });
    }
    const r = await api.put('/system/backup/settings', { enabled: daily, folder: backup.folder.trim(), everyDays: 1 });
    setBackup((b) => ({ ...b, passwordSet: true, enabled: daily }));
    // D75: turning backups on makes the first copy at once; say where it went, or why it failed.
    if (r.data?.firstBackupError) {
      setError(`Backups are on, but the first copy could not be made: ${r.data.firstBackupError}`);
      return false;
    }
    if (r.data?.firstBackup?.file) toast(`Your first backup is saved: ${r.data.firstBackup.file}`);
    setBackupPassword('');
    setBackupConfirm('');
    return true;
  };

  const chooseBackupFolder = async () => {
    try {
      const r = await api.post('/system/backup/choose-folder', { start: backup.folder || undefined });
      if (r.data?.folder) setBackup((b) => ({ ...b, folder: r.data.folder }));
    } catch (err) {
      setError(errMsg(err, 'Could not open the folder window'));
    }
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
    if (step.id === 'backup') {
      setSaving(true);
      setError('');
      try {
        if (await saveBackup()) go(index + 1);
      } catch (err) {
        setError(errMsg(err, 'Could not turn backups on'));
      } finally {
        setSaving(false);
      }
      return;
    }
    if (step.id === 'opening') {
      if (!opening) { setError('Choose one of the two, or press Skip.'); return; }
      write(NO_OPENING_BALANCES, opening === 'new' ? '1' : null);
      go(index + 1);
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
      if (step.id === 'country') {
        const problem = await syncPayrollCountry(body['region.country']);
        if (problem) toast(problem);
      }
      go(index + 1);
    } catch (err) {
      setError(errMsg(err, 'Could not save this'));
    } finally {
      setSaving(false);
    }
  };

  const fieldFor = (s: Step) => {
    switch (s.id) {
      case 'backup':
        if (!backup.available) {
          return <p className="text-sm text-gray-600">Backups are set up on the main PC by an administrator. You can do it later under Settings → Backup &amp; restore.</p>;
        }
        return (
          <div className="space-y-3">
            <label className="block text-xs font-medium text-gray-600">Backup folder
              <div className="flex gap-2 mt-1">
                <input value={backup.folder} onChange={(e) => setBackup((b) => ({ ...b, folder: e.target.value }))} aria-label="Backup folder" />
                {backup.canChooseFolder && <button type="button" className="btn btn-secondary whitespace-nowrap" onClick={chooseBackupFolder}>Choose folder…</button>}
              </div>
            </label>
            {backup.passwordSet ? (
              <p className="text-xs text-gray-500">A backup password is already set; backups use it.</p>
            ) : (
              <div className="grid grid-cols-2 gap-3">
                <label className="block text-xs font-medium text-gray-600">Backup password (at least 8 characters)
                  <input className="mt-1" type="password" autoComplete="new-password" value={backupPassword} onChange={(e) => setBackupPassword(e.target.value)} aria-label="Backup password" /></label>
                <label className="block text-xs font-medium text-gray-600">Type it again
                  <input className="mt-1" type="password" autoComplete="new-password" value={backupConfirm} onChange={(e) => setBackupConfirm(e.target.value)} aria-label="Confirm backup password" /></label>
              </div>
            )}
            <label className="flex items-center gap-2 text-sm"><input type="checkbox" checked={daily} onChange={(e) => setDaily(e.target.checked)} />Back up automatically every day (the last 14 copies are kept)</label>
            {!backup.passwordSet && <p className="text-xs text-amber-700">Write the backup password down and keep it somewhere safe — without it a backup cannot be opened.</p>}
          </div>
        );
      case 'opening':
        return (
          <div className="space-y-2" role="radiogroup" aria-label="Opening balances">
            <label className={`flex items-start gap-3 p-3 border rounded cursor-pointer ${opening === 'new' ? 'border-orange-500 bg-orange-50' : 'border-[#ddd]'}`}>
              <input type="radio" name="opening" checked={opening === 'new'} onChange={() => setOpening('new')} className="mt-1" />
              <span><span className="block text-sm font-medium text-gray-900">It’s a new business</span><span className="block text-xs text-gray-600">Nothing to bring in — start recording from today.</span></span>
            </label>
            <label className={`flex items-start gap-3 p-3 border rounded cursor-pointer ${opening === 'existing' ? 'border-orange-500 bg-orange-50' : 'border-[#ddd]'}`}>
              <input type="radio" name="opening" checked={opening === 'existing'} onChange={() => setOpening('existing')} className="mt-1" />
              <span><span className="block text-sm font-medium text-gray-900">It’s already running</span><span className="block text-xs text-gray-600">After this setup you enter your bank and cash balances, what each customer owes you and what you owe each supplier — it takes a few minutes. You can also bring in files from QuickBooks, Xero or a spreadsheet.</span></span>
            </label>
          </div>
        );
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
        return <input value={val('company.taxNumber')} onChange={set('company.taxNumber')} maxLength={60} placeholder={val('region.country') === 'US' ? 'EIN, e.g. 12-3456789' : 'Tax / VAT / GST number'} aria-label="Tax number" />;
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
              {offeredCountries(savedCountry).map((c) => <option key={c.code} value={c.code}>{c.name}</option>)}
            </select>
            <p className="text-xs text-gray-500">TallyCheck is available for Sri Lanka and the United States for now; more countries will follow.</p>
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
    if (s.id === 'backup') return !backup.available ? '' : backup.enabled ? `On, every day, to ${backup.folder}` : 'Off';
    if (s.id === 'opening') return opening === 'new' ? 'New business — nothing to bring in' : opening === 'existing' ? 'Existing business — enter them next' : '';
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
            <p className="text-sm text-gray-600 mt-1 mb-4">
              {step.id === 'tax' && val('region.country') === 'US'
                ? 'Enter your EIN (Employer Identification Number) or state sales tax number to print it on invoices. Skip this if you have none yet.'
                : step.help}
            </p>

            <div ref={box}>
              {step.id === 'review' ? (
                <table className="w-full text-sm table-fixed">
                  <colgroup><col style={{ width: '9.5rem' }} /><col /><col style={{ width: '4.5rem' }} /></colgroup>
                  <tbody>
                    {STEPS.slice(0, last).map((s, i) => (
                      <tr key={s.id} className="border-b border-[#eee] cursor-pointer hover:bg-[#f7f7f7]" onClick={() => go(i)} title="Change">
                        <td className="py-2 pr-3 text-gray-600 whitespace-nowrap align-top">{SHORT[s.id]}</td>
                        <td className="py-2 text-gray-900 break-all" style={{ overflowWrap: 'anywhere' }}>{summary(s) || <span className="text-gray-400">Not entered</span>}</td>
                        <td className="py-2 pl-2 text-xs text-orange-600 align-top whitespace-nowrap"><button type="button" className="underline" onClick={(e) => { e.stopPropagation(); go(i); }}>Change</button></td>
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
                {saving ? 'Saving…' : step.id === 'review' ? (opening === 'existing' ? 'Finish and enter opening balances' : 'Finish') : 'Next'}
              </button>
            </div>
          </form>
        )}
      </div>
    </div>
  );
}
