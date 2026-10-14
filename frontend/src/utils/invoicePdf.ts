// Invoice PDF generator — renders a styled invoice and downloads it as a .pdf file.
import jsPDF from 'jspdf';
import html2canvas from 'html2canvas';
import { amountInWords } from './amountInWords';

// ─────────────────────────────────────────────────────────────
// EDIT YOUR COMPANY DETAILS HERE
// ─────────────────────────────────────────────────────────────
export const COMPANY = {
  name: 'Cenango (Pvt) Ltd',
  logo: '/Logo_black.png',
  // Address shown under "Remit To"
  remitAddress: ['50 Galpotha Road,', 'Nawala', 'Sri Lanka'],
  // Centred contact line under the logo — update phone/fax/email as needed
  contactLine: '50 Galpotha Road, Nawala, Sri Lanka   Tel: +94-11-2505546   Fax: +94-11-2504551',
  emailWebLine: 'info@cenango.lk  |  www.cenango.lk',
  paymentOptions: [
    "01. By Cheque drawn in favour of 'Cenango (Pvt) Ltd'.",
    '02. Direct remittance to Account Number 1500013830 of Cenango (Pvt) Ltd at Commercial Bank of Ceylon PLC.',
  ],
  // Printed at the very bottom of every invoice.
  footerNote: 'This is a system-generated invoice and no signature is required',
  // Fallbacks only — the live values come from Settings → Company (GET /settings).
  signatoryName: 'Samir Husain',
  signatoryTitle: 'Director',
};
// ─────────────────────────────────────────────────────────────

/** Company settings that are editable in the app (Settings page). */
export interface InvoiceSettings {
  signatoryName?: string;
  signatoryTitle?: string;
}

interface PdfLine {
  description: string;
  quantity: number | string;
  unitPrice: number | string;
  amount: number | string;
}

interface PdfInvoice {
  invoiceNumber: string;
  poNumber?: string | null;
  date: string;
  dueDate?: string | null;
  notes?: string | null;
  totalAmount: number | string;
  customer?: { name?: string; address?: string | null } | null;
  lines?: PdfLine[];
  /** Per-invoice overrides captured when the invoice was raised. */
  billToName?: string | null;
  billToAddress?: string | null;
  /** Newline-separated, one option per line. */
  paymentOptions?: string | null;
  signatoryName?: string | null;
  signatoryTitle?: string | null;
}

/** Splits a multi-line field into printable, blank-free lines. */
const splitLines = (v?: string | null) =>
  String(v ?? '')
    .split(/\r?\n/)
    .map((l) => l.trim())
    .filter(Boolean);

const num = (v: number | string) =>
  new Intl.NumberFormat('en-LK', { minimumFractionDigits: 2, maximumFractionDigits: 2 }).format(Number(v) || 0);

const fmtDate = (d?: string | null) => {
  if (!d) return '';
  const date = new Date(d);
  if (isNaN(date.getTime())) return d;
  return date.toLocaleDateString('en-GB', { day: '2-digit', month: 'short', year: 'numeric' });
};

const esc = (s?: string | null) =>
  String(s ?? '').replace(/[&<>"]/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' }[c] as string));

function buildMarkup(invoice: PdfInvoice, origin: string, settings?: InvoiceSettings): string {
  const signatoryName =
    invoice.signatoryName?.trim() || settings?.signatoryName?.trim() || COMPANY.signatoryName;
  const signatoryTitle =
    invoice.signatoryTitle?.trim() || settings?.signatoryTitle?.trim() || COMPANY.signatoryTitle;
  const billToName = invoice.billToName?.trim() || invoice.customer?.name || '';
  const billToAddress = invoice.billToAddress?.trim()
    ? splitLines(invoice.billToAddress)
    : splitLines(invoice.customer?.address);
  const paymentOptions = invoice.paymentOptions?.trim()
    ? splitLines(invoice.paymentOptions)
    : COMPANY.paymentOptions;
  const lines = invoice.lines ?? [];
  const rowsHtml = lines
    .map(
      (l, i) => `
        <tr>
          <td class="c-date">${i === 0 ? esc(fmtDate(invoice.date)) : ''}</td>
          <td class="c-desc">${esc(l.description)}</td>
          <td class="c-num">${Number(l.quantity) ? num(l.quantity) : ''}</td>
          <td class="c-num">${Number(l.unitPrice) ? num(l.unitPrice) : ''}</td>
          <td class="c-num">${Number(l.amount) ? num(l.amount) : ''}</td>
        </tr>`,
    )
    .join('');

  return `
    <style>
      #inv-pdf-root { font-family: Arial, Helvetica, sans-serif; color: #111; font-size: 13px; background:#fff; padding: 40px 44px; }
      #inv-pdf-root * { box-sizing: border-box; }
      #inv-pdf-root .logo-row { display: flex; justify-content: flex-end; }
      #inv-pdf-root .logo-row img { height: 40px; }
      #inv-pdf-root .rule { border-top: 1px solid #111; margin: 7px 0; }
      #inv-pdf-root .contact { text-align: center; font-size: 11px; color: #333; line-height: 1.6; }
      #inv-pdf-root h1 { font-size: 26px; margin: 22px 0 18px; }
      #inv-pdf-root .meta { display: flex; justify-content: space-between; margin-bottom: 16px; }
      #inv-pdf-root .label { font-weight: bold; margin-bottom: 5px; }
      #inv-pdf-root .block { margin-bottom: 16px; }
      #inv-pdf-root table { width: 100%; border-collapse: collapse; margin-top: 8px; }
      #inv-pdf-root th, #inv-pdf-root td { border: 1px solid #444; padding: 6px 8px; vertical-align: top; }
      #inv-pdf-root thead th { background: #63d2e8; font-weight: bold; text-align: center; }
      #inv-pdf-root .c-desc { text-align: left; }
      #inv-pdf-root .c-num { text-align: right; white-space: nowrap; }
      #inv-pdf-root .c-date { white-space: nowrap; }
      #inv-pdf-root tfoot td { border: none; padding: 4px 8px; }
      #inv-pdf-root tfoot .total-label { text-align: right; font-weight: bold; }
      #inv-pdf-root tfoot .total-val { text-align: right; font-weight: bold; border-top: 1px solid #444; white-space: nowrap; }
      #inv-pdf-root .words { margin-top: 10px; font-size: 12px; }
      #inv-pdf-root .pay { margin-top: 30px; }
      #inv-pdf-root .pay p { margin: 3px 0; font-size: 12px; }
      #inv-pdf-root .sign { margin-top: 46px; }
      #inv-pdf-root .sign .thanks { font-weight: bold; margin-bottom: 46px; }
      #inv-pdf-root .sign .name { font-weight: bold; }
      #inv-pdf-root .sysnote { margin-top: 34px; padding-top: 8px; border-top: 1px solid #ccc; text-align: center; font-size: 11px; font-style: italic; color: #444; }
    </style>
    <div class="logo-row"><img src="${origin}${COMPANY.logo}" alt="logo" crossorigin="anonymous" /></div>
    <div class="rule"></div>
    <div class="contact">${esc(COMPANY.contactLine)}<br/>${esc(COMPANY.emailWebLine)}</div>
    <div class="rule"></div>

    <h1>Invoice</h1>

    <div class="meta">
      <div>
        <div class="label">Remit To:</div>
        <div>${esc(COMPANY.name)}</div>
        ${COMPANY.remitAddress.map((l) => `<div>${esc(l)}</div>`).join('')}
      </div>
      <div>
        <div>${esc(fmtDate(invoice.date))}</div>
        <div style="margin-top:10px;"><strong>Invoice No:</strong> ${esc(invoice.invoiceNumber)}</div>
        ${invoice.poNumber ? `<div><strong>PO No:</strong> ${esc(invoice.poNumber)}</div>` : ''}
        ${invoice.dueDate ? `<div><strong>Due Date:</strong> ${esc(fmtDate(invoice.dueDate))}</div>` : ''}
      </div>
    </div>

    <div class="block">
      <div class="label">Bill To:</div>
      <div>${esc(billToName) || '—'}</div>
      ${billToAddress.map((l) => `<div>${esc(l)}</div>`).join('')}
    </div>

    <table>
      <thead>
        <tr>
          <th style="width:80px;">Date</th>
          <th class="c-desc">Description</th>
          <th style="width:50px;">Qty</th>
          <th style="width:95px;">Unit Price (Rs.)</th>
          <th style="width:95px;">Price (Rs.)</th>
        </tr>
      </thead>
      <tbody>
        ${rowsHtml || '<tr><td colspan="5" style="text-align:center;color:#888;">No line items</td></tr>'}
      </tbody>
      <tfoot>
        <tr>
          <td colspan="3" style="border:none;"></td>
          <td class="total-label">Total</td>
          <td class="total-val">${num(invoice.totalAmount)}</td>
        </tr>
      </tfoot>
    </table>

    <div class="words"><strong>Amount in words:</strong> ${esc(amountInWords(Number(invoice.totalAmount) || 0))}</div>

    <div class="pay">
      <div class="label">Payment options</div>
      ${paymentOptions.map((p) => `<p>${esc(p)}</p>`).join('')}
    </div>

    <div class="sign">
      <div class="thanks">Thanking You</div>
      <div class="name">${esc(signatoryName)}</div>
      <div>${esc(signatoryTitle)}</div>
    </div>

    <div class="sysnote">${esc(COMPANY.footerNote)}</div>`;
}

/**
 * Opens the invoice in a new tab. The screen styling (grey backdrop, drop
 * shadow, toolbar) is stripped by the print stylesheet, so what reaches the
 * printer is the sheet alone on a clean A4 page.
 *
 * @param autoPrint fire the browser print dialog once the logo has loaded.
 */
function openInvoiceWindow(
  invoice: PdfInvoice,
  settings: InvoiceSettings | undefined,
  autoPrint: boolean,
  target?: Window | null,
) {
  const origin = window.location.origin;
  const markup = buildMarkup(invoice, origin, settings);

  const win = target ?? window.open('', '_blank');
  if (!win) {
    alert('Please allow pop-ups for this site to open the invoice.');
    return;
  }

  win.document.open();
  win.document.write(
    `<!doctype html><html><head><meta charset="utf-8" />` +
      `<title>Invoice ${esc(invoice.invoiceNumber)}</title>` +
      `<style>` +
      `body{margin:0;background:#e5e7eb;padding:24px 0;font-family:Arial,Helvetica,sans-serif;}` +
      `#sheet{width:794px;margin:0 auto;background:#fff;box-shadow:0 2px 14px rgba(0,0,0,.18);}` +
      `#bar{width:794px;margin:0 auto 12px;display:flex;gap:8px;justify-content:flex-end;}` +
      `#bar button{font:500 14px/1 Arial,Helvetica,sans-serif;padding:9px 14px;border-radius:6px;` +
      `border:1px solid #2563eb;background:#2563eb;color:#fff;cursor:pointer;}` +
      `#bar button.secondary{background:#fff;color:#374151;border-color:#d1d5db;}` +
      `@media print{body{background:#fff;padding:0;}#sheet{width:auto;box-shadow:none;}#bar{display:none;}}` +
      `@page{size:A4;margin:0;}` +
      `</style></head><body>` +
      `<div id="bar"><button class="secondary" onclick="window.close()">Close</button>` +
      `<button onclick="window.print()">Print</button></div>` +
      `<div id="sheet"><div id="inv-pdf-root">${markup}</div></div>` +
      `</body></html>`,
  );
  win.document.close();

  if (!autoPrint) return;

  // Printing before the logo decodes would leave a gap where it should be.
  const go = () => { win.focus(); win.print(); };
  const img = win.document.querySelector('img');
  if (img && !img.complete) {
    img.onload = go;
    img.onerror = go;
  } else {
    go();
  }
}

/**
 * Opens a blank tab immediately, for callers that must fetch the invoice first.
 *
 * `window.open` only escapes the pop-up blocker while the click is still being
 * handled. Awaiting an API call first loses that, and the tab is silently
 * blocked — which is why it used to take two clicks. Open the tab here, then
 * hand it to previewInvoice / printInvoice once the data lands.
 */
export function openInvoiceTab(): Window | null {
  const win = window.open('', '_blank');
  if (win) {
    win.document.write(
      '<!doctype html><meta charset="utf-8"><title>Invoice</title>' +
        '<p style="font:14px Arial,sans-serif;color:#6b7280;padding:24px">Loading invoice…</p>',
    );
  }
  return win;
}

/** Opens the invoice in a new tab for review, with Print and Close buttons. */
export function previewInvoice(invoice: PdfInvoice, settings?: InvoiceSettings, target?: Window | null) {
  openInvoiceWindow(invoice, settings, false, target);
}

/** Opens the invoice and goes straight to the browser's print dialog. */
export function printInvoice(invoice: PdfInvoice, settings?: InvoiceSettings, target?: Window | null) {
  openInvoiceWindow(invoice, settings, true, target);
}

export async function openInvoicePdf(invoice: PdfInvoice, settings?: InvoiceSettings) {
  const origin = window.location.origin;

  const root = document.createElement('div');
  root.id = 'inv-pdf-root';
  root.style.position = 'fixed';
  root.style.left = '-99999px';
  root.style.top = '0';
  root.style.width = '794px'; // ≈ A4 width at 96dpi
  root.innerHTML = buildMarkup(invoice, origin, settings);
  document.body.appendChild(root);

  // Wait for the logo image to finish loading before capturing.
  const img = root.querySelector('img');
  if (img && !img.complete) {
    await new Promise<void>((resolve) => {
      img.onload = () => resolve();
      img.onerror = () => resolve();
    });
  }

  try {
    const canvas = await html2canvas(root, { scale: 2, useCORS: true, backgroundColor: '#ffffff' });
    const imgData = canvas.toDataURL('image/png');

    const pdf = new jsPDF({ unit: 'mm', format: 'a4', orientation: 'portrait' });
    const pageW = 210;
    const pageH = 297;
    const imgW = pageW;
    const imgH = (canvas.height * imgW) / canvas.width;

    let heightLeft = imgH;
    let position = 0;
    pdf.addImage(imgData, 'PNG', 0, position, imgW, imgH);
    heightLeft -= pageH;
    while (heightLeft > 0) {
      position -= pageH;
      pdf.addPage();
      pdf.addImage(imgData, 'PNG', 0, position, imgW, imgH);
      heightLeft -= pageH;
    }

    const safeName = `Invoice-${invoice.invoiceNumber || 'invoice'}`.replace(/[\\/:*?"<>|]+/g, '-');
    pdf.save(`${safeName}.pdf`);
  } finally {
    document.body.removeChild(root);
  }
}
