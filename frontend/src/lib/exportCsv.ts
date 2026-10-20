import { region } from './region';

/**
 * D23: "Export to Excel". Every table on the screen goes into one CSV file (UTF-8 with a BOM,
 * so Excel reads it straight away), one block per table with the table's heading above it.
 *
 * Amounts shown in the company's currency and number format ("LKR 2,500.63", "£2,500.63",
 * "2.500,63 €") are written as plain numbers (2500.63) so Excel can add them up. Action columns
 * (no heading, or "Actions") and checkbox columns are left out, and so are "nothing here"
 * placeholder rows.
 */

/** Thin and no-break spaces (used as group separators by some locales) as plain spaces. */
const plainSpaces = (s: string) => s.replace(/[\u00a0\u202f]/g, ' ');

/** The amount in a cell as a plain number string, or null when the cell is not an amount. */
function plainAmount(text: string): string | null {
  const r = region();
  let parts: Intl.NumberFormatPart[] = [];
  try {
    parts = new Intl.NumberFormat(r.locale).formatToParts(12345.6);
  } catch {
    /* fall back to "," and "." */
  }
  const group = plainSpaces(parts.find((p) => p.type === 'group')?.value ?? ',');
  const decimal = parts.find((p) => p.type === 'decimal')?.value ?? '.';
  let t = plainSpaces(text).trim();
  let marked = false;
  for (const m of [r.currency, r.currencySymbol].filter(Boolean).sort((a, b) => b.length - a.length)) {
    if (t.includes(m)) {
      t = t.replace(m, '').trim();
      marked = true;
      break;
    }
  }
  // A bare "1000" (an account code) or "2026" stays text; amounts carry a separator or a currency.
  if (!marked && !t.includes(decimal) && !t.includes(group)) return null;
  const normal = t.split(group).join('').split(decimal).join('.');
  const m = /^(-)?\s*(\d+)(?:\.(\d+))?\s*(-)?$/.exec(normal);
  if (!m) return null;
  return `${m[1] || m[4] ? '-' : ''}${m[2]}${m[3] ? `.${m[3]}` : ''}`;
}

function cellValue(cell: HTMLTableCellElement): string {
  const text = (cell.innerText ?? cell.textContent ?? '').replace(/\s+/g, ' ').trim();
  return plainAmount(text) ?? text;
}

function csvField(raw: string): string {
  // Text that Excel would run as a formula (=, +, @, or - not followed by a number) is
  // prefixed with an apostrophe so it stays text.
  const v = /^[=+@\t\r]/.test(raw) || (/^-/.test(raw) && !/^-\d/.test(raw)) ? `'${raw}` : raw;
  return /[",\r\n]/.test(v) ? `"${v.replace(/"/g, '""')}"` : v;
}

/** The heading a table sits under: its own first header row is not enough on report pages. */
function titleOf(table: HTMLTableElement): string {
  const section = table.closest('section');
  const heading = section?.querySelector('h2, h3');
  return heading?.textContent?.trim() ?? '';
}

export function tablesToCsv(root: ParentNode): string | null {
  const tables = [...root.querySelectorAll('table')].filter((t) => t.offsetParent !== null || t.getClientRects().length > 0);
  if (tables.length === 0) return null;
  const blocks: string[] = [];
  for (const table of tables) {
    const headRow = table.tHead?.rows[table.tHead.rows.length - 1];
    const headers = headRow ? [...headRow.cells].map((c) => c.innerText.trim()) : [];
    const skip = new Set<number>();
    headers.forEach((h, i) => {
      const cell = headRow?.cells[i];
      if (h === '' || /^actions?$/i.test(h) || cell?.querySelector('input[type=checkbox]')) skip.add(i);
    });
    const lines: string[] = [];
    const title = titleOf(table);
    if (title && tables.length > 1) lines.push(csvField(title));
    if (headers.length) lines.push(headers.filter((_, i) => !skip.has(i)).map(csvField).join(','));
    for (const body of [...table.tBodies, ...(table.tFoot ? [table.tFoot] : [])]) {
      for (const row of [...body.rows]) {
        const cells = [...row.cells];
        // "No records" style placeholders span the whole table.
        if (cells.length === 1 && cells[0].colSpan > 1 && headers.length > 1) continue;
        const out: string[] = [];
        let col = 0;
        for (const cell of cells) {
          const value = cellValue(cell);
          for (let span = 0; span < cell.colSpan; span++, col++) {
            if (skip.has(col)) continue;
            out.push(span === 0 ? csvField(value) : '');
          }
        }
        lines.push(out.join(','));
      }
    }
    blocks.push(lines.join('\r\n'));
  }
  return blocks.join('\r\n\r\n');
}

/** Saves the screen's tables as "<title> <date>.csv". Returns false when there was nothing to export. */
export function exportScreen(root: ParentNode, title: string): boolean {
  const csv = tablesToCsv(root);
  if (!csv) return false;
  const blob = new Blob(['﻿', csv], { type: 'text/csv;charset=utf-8' });
  const url = URL.createObjectURL(blob);
  const a = document.createElement('a');
  const day = new Date();
  const stamp = `${day.getFullYear()}-${String(day.getMonth() + 1).padStart(2, '0')}-${String(day.getDate()).padStart(2, '0')}`;
  a.href = url;
  a.download = `${(title || 'Export').replace(/[\\/:*?"<>|]+/g, ' ').trim()} ${stamp}.csv`;
  document.body.appendChild(a);
  a.click();
  a.remove();
  setTimeout(() => URL.revokeObjectURL(url), 10000);
  return true;
}
