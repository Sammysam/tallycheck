// Spells a money amount for the "Amount in words" line on printed invoices.

const ONES = [
  '', 'One', 'Two', 'Three', 'Four', 'Five', 'Six', 'Seven', 'Eight', 'Nine',
  'Ten', 'Eleven', 'Twelve', 'Thirteen', 'Fourteen', 'Fifteen', 'Sixteen',
  'Seventeen', 'Eighteen', 'Nineteen',
];
const TENS = ['', '', 'Twenty', 'Thirty', 'Forty', 'Fifty', 'Sixty', 'Seventy', 'Eighty', 'Ninety'];
const SCALES: [number, string][] = [
  [1_000_000_000, 'Billion'],
  [1_000_000, 'Million'],
  [1_000, 'Thousand'],
];

/** 0–999 in words. */
function underThousand(n: number): string {
  if (n < 20) return ONES[n];
  if (n < 100) {
    const rest = n % 10;
    return TENS[Math.floor(n / 10)] + (rest ? ` ${ONES[rest]}` : '');
  }
  const rest = n % 100;
  return `${ONES[Math.floor(n / 100)]} Hundred` + (rest ? ` ${underThousand(rest)}` : '');
}

/** Whole number in words. Returns 'Zero' for 0. */
export function numberToWords(value: number): string {
  let n = Math.floor(Math.abs(value));
  if (n === 0) return 'Zero';

  const parts: string[] = [];
  for (const [size, name] of SCALES) {
    if (n >= size) {
      parts.push(`${underThousand(Math.floor(n / size))} ${name}`);
      n %= size;
    }
  }
  if (n > 0) parts.push(underThousand(n));
  return parts.join(' ');
}

/**
 * "Rupees One Hundred Thousand and Cents Fifty Only" — the wording Sri Lankan
 * invoices carry under the total.
 */
export function amountInWords(value: number, currency = 'Rupees', fraction = 'Cents'): string {
  const amount = Math.abs(Number(value) || 0);
  const whole = Math.floor(amount);
  const cents = Math.round((amount - whole) * 100);

  const head = `${currency} ${numberToWords(whole)}`;
  return cents > 0 ? `${head} and ${fraction} ${numberToWords(cents)} Only` : `${head} Only`;
}
