import axios from 'axios';

/**
 * What to tell someone whose sign-in step failed. The server's own reason is shown when it is
 * safe to: too many attempts (429) or a disabled account (403, only sent after the right
 * password). A wrong username or password stays one message, so it never reveals which
 * usernames exist.
 */
export function signInError(err: unknown, step: 'password' | 'code'): string {
  if (!axios.isAxiosError(err) || !err.response) {
    return 'Cannot reach the accounting server. Check that Cenango Accounting is open on the main PC, then try again.';
  }
  const { status, data } = err.response;
  const message = typeof data?.message === 'string' ? data.message : '';
  if (status === 429) return message || 'Too many failed attempts. Wait a few minutes and try again.';
  if (status === 403 && /inactive/i.test(message)) {
    return 'This account has been turned off. Ask an administrator to turn it back on.';
  }
  if (status >= 500) return 'Something went wrong on the server. Try again; if it keeps happening, tell your administrator.';
  if (step === 'code') {
    if (/token/i.test(message)) return 'Your sign-in has timed out. Go back and sign in again.';
    return 'That code is not right. Check the 6-digit number in your authenticator app — it changes every 30 seconds.';
  }
  return 'Invalid username or password';
}
