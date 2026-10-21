/** A rough guide to how strong a new password is (length and variety). */
export function passwordStrength(p: string): { score: 0 | 1 | 2 | 3; label: string } {
  if (p.length < 8) return { score: 0, label: 'Too short — at least 8 characters' };
  const kinds = [/[a-z]/, /[A-Z]/, /\d/, /[^A-Za-z0-9]/].filter((r) => r.test(p)).length;
  if (p.length >= 12 && kinds >= 3) return { score: 3, label: 'Strong' };
  if (p.length >= 10 && kinds >= 2) return { score: 2, label: 'Good' };
  return { score: 1, label: 'Weak — add length, capitals, numbers or symbols' };
}
