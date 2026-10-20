/**
 * Starting values for the company setup screen (D25). Picking a country fills in its usual
 * currency, number format, currency words and financial year start; every value can still be
 * changed. Only currencies with two decimal places are listed, because the books keep amounts
 * in hundredths.
 */
export interface CountryPreset {
  code: string;
  name: string;
  currency: string;
  symbol: string;
  word: string;
  minorWord: string;
  locale: string;
  fiscalYearStart: number;
}

export const COUNTRIES: CountryPreset[] = [
  { code: 'AU', name: 'Australia', currency: 'AUD', symbol: '$', word: 'Dollars', minorWord: 'Cents', locale: 'en-AU', fiscalYearStart: 7 },
  { code: 'BD', name: 'Bangladesh', currency: 'BDT', symbol: '৳', word: 'Taka', minorWord: 'Poisha', locale: 'en-BD', fiscalYearStart: 7 },
  { code: 'CA', name: 'Canada', currency: 'CAD', symbol: '$', word: 'Dollars', minorWord: 'Cents', locale: 'en-CA', fiscalYearStart: 1 },
  { code: 'FR', name: 'France', currency: 'EUR', symbol: '€', word: 'Euro', minorWord: 'Cent', locale: 'fr-FR', fiscalYearStart: 1 },
  { code: 'DE', name: 'Germany', currency: 'EUR', symbol: '€', word: 'Euro', minorWord: 'Cent', locale: 'de-DE', fiscalYearStart: 1 },
  { code: 'IN', name: 'India', currency: 'INR', symbol: '₹', word: 'Rupees', minorWord: 'Paise', locale: 'en-IN', fiscalYearStart: 4 },
  { code: 'IE', name: 'Ireland', currency: 'EUR', symbol: '€', word: 'Euro', minorWord: 'Cent', locale: 'en-IE', fiscalYearStart: 1 },
  { code: 'IT', name: 'Italy', currency: 'EUR', symbol: '€', word: 'Euro', minorWord: 'Cent', locale: 'it-IT', fiscalYearStart: 1 },
  { code: 'KE', name: 'Kenya', currency: 'KES', symbol: 'KSh', word: 'Shillings', minorWord: 'Cents', locale: 'en-KE', fiscalYearStart: 7 },
  { code: 'MY', name: 'Malaysia', currency: 'MYR', symbol: 'RM', word: 'Ringgit', minorWord: 'Sen', locale: 'en-MY', fiscalYearStart: 1 },
  { code: 'MV', name: 'Maldives', currency: 'MVR', symbol: 'Rf', word: 'Rufiyaa', minorWord: 'Laari', locale: 'en-MV', fiscalYearStart: 1 },
  { code: 'NP', name: 'Nepal', currency: 'NPR', symbol: 'Rs.', word: 'Rupees', minorWord: 'Paisa', locale: 'en-NP', fiscalYearStart: 7 },
  { code: 'NL', name: 'Netherlands', currency: 'EUR', symbol: '€', word: 'Euro', minorWord: 'Cent', locale: 'nl-NL', fiscalYearStart: 1 },
  { code: 'NZ', name: 'New Zealand', currency: 'NZD', symbol: '$', word: 'Dollars', minorWord: 'Cents', locale: 'en-NZ', fiscalYearStart: 4 },
  { code: 'NG', name: 'Nigeria', currency: 'NGN', symbol: '₦', word: 'Naira', minorWord: 'Kobo', locale: 'en-NG', fiscalYearStart: 1 },
  { code: 'PK', name: 'Pakistan', currency: 'PKR', symbol: 'Rs', word: 'Rupees', minorWord: 'Paisa', locale: 'en-PK', fiscalYearStart: 7 },
  { code: 'PH', name: 'Philippines', currency: 'PHP', symbol: '₱', word: 'Pesos', minorWord: 'Centavos', locale: 'en-PH', fiscalYearStart: 1 },
  { code: 'QA', name: 'Qatar', currency: 'QAR', symbol: 'QR', word: 'Riyals', minorWord: 'Dirhams', locale: 'en-QA', fiscalYearStart: 1 },
  { code: 'SA', name: 'Saudi Arabia', currency: 'SAR', symbol: 'SR', word: 'Riyals', minorWord: 'Halalas', locale: 'en-SA', fiscalYearStart: 1 },
  { code: 'SG', name: 'Singapore', currency: 'SGD', symbol: '$', word: 'Dollars', minorWord: 'Cents', locale: 'en-SG', fiscalYearStart: 1 },
  { code: 'ZA', name: 'South Africa', currency: 'ZAR', symbol: 'R', word: 'Rand', minorWord: 'Cents', locale: 'en-ZA', fiscalYearStart: 3 },
  { code: 'ES', name: 'Spain', currency: 'EUR', symbol: '€', word: 'Euro', minorWord: 'Cent', locale: 'es-ES', fiscalYearStart: 1 },
  { code: 'LK', name: 'Sri Lanka', currency: 'LKR', symbol: 'Rs.', word: 'Rupees', minorWord: 'Cents', locale: 'en-LK', fiscalYearStart: 4 },
  { code: 'AE', name: 'United Arab Emirates', currency: 'AED', symbol: 'AED', word: 'Dirhams', minorWord: 'Fils', locale: 'en-AE', fiscalYearStart: 1 },
  { code: 'GB', name: 'United Kingdom', currency: 'GBP', symbol: '£', word: 'Pounds', minorWord: 'Pence', locale: 'en-GB', fiscalYearStart: 4 },
  { code: 'US', name: 'United States', currency: 'USD', symbol: '$', word: 'Dollars', minorWord: 'Cents', locale: 'en-US', fiscalYearStart: 1 },
];
