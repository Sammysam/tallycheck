import { CURRENCIES } from '../lib/countries';
import { region } from '../lib/region';

/**
 * D30: the document's currency and exchange rate. The home currency needs no rate; a foreign
 * one asks how many home units one unit is worth, starting from the last rate used.
 */
export default function CurrencyPicker({ currency, rate, onChange, compact = false }: {
  /** '' for the home currency. */
  currency: string;
  rate: string;
  onChange: (currency: string, rate: string) => void;
  compact?: boolean;
}) {
  const home = region().currency;
  const choose = (code: string) => {
    const foreign = code && code !== home ? code : '';
    const last = foreign ? region().lastRates[foreign] : undefined;
    onChange(foreign, foreign ? (rate && currency === foreign ? rate : last ? String(last) : '') : '');
  };
  return (
    <div className={compact ? 'flex gap-2 items-end' : 'space-y-1'}>
      <label className="text-xs font-medium text-gray-600 block">
        Currency
        <select className="mt-1" value={currency || home} onChange={(e) => choose(e.target.value)} aria-label="Currency">
          <option value={home}>{home} (home)</option>
          {CURRENCIES.filter((c) => c !== home).map((c) => <option key={c} value={c}>{c}</option>)}
        </select>
      </label>
      {currency && (
        <label className="text-xs font-medium text-gray-600 block">
          1 {currency} = how many {home}?
          <input className="mt-1" type="number" min="0.00000001" step="any" required placeholder="Exchange rate" value={rate}
            onChange={(e) => onChange(currency, e.target.value)} aria-label="Exchange rate" />
        </label>
      )}
    </div>
  );
}
