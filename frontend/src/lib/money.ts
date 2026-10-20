/** Rupee amounts as the reports show them: "LKR 2,500.63". */
export const money = (v: number) => new Intl.NumberFormat('en-LK', { style: 'currency', currency: 'LKR' }).format(v ?? 0);
