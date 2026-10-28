---
name: explain-a-report
description: Explain a TallyCheck report (profit and loss, balance sheet, cash flow, trial balance, ageing) in plain words and dig into the accounts behind a figure. Use when the user asks why a number is what it is, what changed, or what a report means.
---

# Explain a report

1. Run the report the user means with `run_report` (ask for the period if needed). For "what
   changed", run the same report for the comparison period too (previous month, or the same
   period last year).
2. Explain it for someone who is not an accountant: what came in, what went out, what is left,
   in three to five sentences. Use the report's own figures.
3. For the biggest changes or anything the user asks about, call `account_activity` with the
   account code and the period, and name the largest entries behind the figure (date, description,
   amount, document number).
4. If a figure looks wrong (for example an expense in a revenue account, or a large round entry),
   say so as a question for the user to check — do not claim it is an error.
5. You cannot change entries. Corrections are made in the app (a reversing journal entry, or
   voiding the document).

Never estimate: if a figure is not in a tool's answer, say you do not have it.
