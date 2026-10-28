---
name: prepare-vat-return
description: Prepare the figures for a VAT / GST / sales tax return from TallyCheck for a period, with checks. Use when the user asks for the tax return, VAT payable, or tax for a quarter or month.
---

# Prepare the VAT return

1. Confirm the period (start and end dates). Sri Lankan VAT is usually monthly or quarterly; ask if
   the user has not said.
2. Call `run_report` with `tax_return` for the period. Present, per tax code: taxable sales, tax on
   sales, taxable purchases, tax on purchases, and the **tax to pay** (or to reclaim). Include
   withholding tax deducted by customers if the report shows it.
3. Checks before the user files:
   - `list_invoices` with status `DRAFT` and dates in the period — drafts are not in the return;
   - posted invoices and bills in the period without a tax code, if the user expects tax on them
     (look at a few with `get_invoice`);
   - `bank_lines_to_review` for lines in the period that may be unrecorded sales or purchases.
4. Say clearly that you are not a tax adviser, that the figures come from the books as posted,
   and that the user fills in the official form (e.g. the IRD return) themselves.

Never round or adjust the report's figures; show them as the tool returns them.
