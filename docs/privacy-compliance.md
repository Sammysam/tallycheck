# Privacy compliance: GDPR and California (CCPA / CPRA)

Last reviewed: 14 November 2026. This is an engineering checklist, **not legal advice**: have a
privacy lawyer confirm it before relying on it, in particular items marked 👤.

Legend: ✅ in place · ⚠️ gap to fix · 👤 needs a decision or action by Cenango (not code)

## 1. Who is responsible for what

| Product | Personal data handled | Cenango's role |
|---|---|---|
| **TallyCheck Desktop** (this repository, Windows / macOS) | The customer's own books: their customers, vendors, employees, salaries, bank details. Stored only on their computer. | **None for the books.** The business using TallyCheck is the *controller*; Cenango never receives the data (no accounts, analytics or telemetry). |
| **tallycheck.io website** | Emails sent through the contact / sign-up links; hosting logs (IP address, pages). | **Controller.** |
| **TallyCheck Web** (hosted edition, separate code base) | The customer's books on Cenango's servers. | **Processor** for the customer's books (GDPR Art. 28 / CCPA "service provider"); controller for account and billing data. |

So most legal duties fall on the website and the hosted Web edition. For Desktop, the job is to
(a) describe the few internet connections honestly and (b) give customers the tools they need to
meet *their* duties towards the people in their books.

## 2. Does each law apply?

* **GDPR / UK GDPR:** applies to Cenango as soon as it offers TallyCheck (website, Web, Store
  listing) to people in the EU / UK and processes their personal data, even though Cenango is
  outside the EU. Customers in the EU are themselves controllers for their books.
* **CCPA / CPRA (California):** applies to a for-profit business that does business in California
  and meets **one** of: annual gross revenue above the current threshold (about US$26.6 million,
  adjusted each year), buys/sells/shares personal information of 100,000+ California consumers or
  households, or earns 50%+ of revenue from selling or sharing it. 👤 Cenango should confirm
  whether it meets a threshold; if not, CCPA does not apply yet, but following it costs little and
  US customers expect it. Since 2023 it also covers **employee** data, which matters to US
  employers using the payroll features.

## 3. Checklist

### A. TallyCheck Desktop (this repository)

| # | Item | GDPR | CCPA | Status |
|---|---|---|---|---|
| A1 | No telemetry, analytics, ads or accounts; Cenango never receives the books | Art. 5(1)(c), 25 | §1798.100 | ✅ |
| A2 | Every internet connection listed in [PRIVACY.md](../PRIVACY.md) (update check, connectors, Claude) and off until switched on | Art. 13 | Notice at collection | ✅ |
| A3 | Privacy policy names the correct data folders (installer, Microsoft Store, macOS) and a private contact | Art. 13 | §1798.130 | ✅ (updated with this review) |
| A4 | Roles and permissions; optional 2FA; activity log of changes | Art. 32 | §1798.150 | ✅ |
| A5 | Passwords hashed (Argon2id); employee documents, connector keys and backups encrypted | Art. 32 | §1798.150 | ✅ |
| A6 | No US Social Security numbers stored (W-4 data only) | Art. 5(1)(c) | Sensitive PI | ✅ |
| A6b | Employee national ID numbers (Sri Lankan NIC, for EPF / ETF forms) are stored in the plain database; a government ID is "sensitive personal information" under CPRA | Art. 32 | Sensitive PI | ⚠️ Optional field; covered by A8 (disk / database encryption); consider showing it masked |
| A7 | Passwords, tokens and document contents never written to logs | Art. 32 | — | ✅ |
| A8 | The database file itself is **not encrypted** (salaries, bank details in plain SQLite) | Art. 32 | §1798.150 | ⚠️ Recommend BitLocker / FileVault in the install guide now; consider database encryption (SQLCipher) later |
| A9 | Office LAN mode uses **plain HTTP** between PCs | Art. 32 | §1798.150 | ⚠️ Documented limitation; add HTTPS for LAN mode, or advise use only on trusted networks |
| A10 | **Access / portability:** export everything about one customer, vendor or employee | Art. 15, 20 | Right to know | ⚠️ CSV export exists per screen and customer statements exist; add "Export this person's data" (one file per customer / vendor / employee) |
| A11 | **Erasure:** remove a person while keeping legally required accounting records | Art. 17(3)(b) | Right to delete (exception for legal obligations) | ⚠️ Unused records can be deleted; people with transactions can only be switched off. Add "Anonymise" (replace name, email, phone, address, tax id) that keeps the amounts |
| A12 | **Rectification:** correct a person's details | Art. 16 | Right to correct | ✅ Edit customer / vendor / employee |
| A13 | **Retention guidance:** how long to keep books and payroll records, and how to delete old data | Art. 5(1)(e) | §1798.100(a)(3) | ⚠️ Add a section to the user guide (accounting laws usually require 6–10 years) |
| A14 | Claude connector sends questions and data to Anthropic: customers must know they need their own agreement with Anthropic | Art. 28, 44 | Service provider contract | ⚠️ Add a clear warning in Settings → Integrations → Claude and in the docs |
| A15 | Third-party connectors (Shopify, WooCommerce, PayPal, Stripe) are the customer's own processors | Art. 28 | — | ✅ Stated in PRIVACY.md |
| A16 | Privacy policy reachable from inside the app (Help → About / Privacy) | Art. 13 | Notice at collection | ⚠️ Add a link |

### B. tallycheck.io website (Cenango is the controller) 👤

| # | Item | Status |
|---|---|---|
| B1 | Name the legal entity and postal address (Cenango (Pvt) Ltd, Sri Lanka) as controller in the website privacy policy (Art. 13(1)(a)) | ⚠️ The policy says only "TallyCheck" and hello@tallycheck.io |
| B2 | State the lawful basis for each use (contract, legitimate interests, consent for marketing) (Art. 13(1)(c)) | ⚠️ Missing |
| B3 | List all GDPR rights, including objection, portability, withdrawing consent and complaining to a data protection authority (Art. 13(2)) | ⚠️ Partly there |
| B4 | International transfers (to Sri Lanka, US hosting) and the safeguard used, e.g. Standard Contractual Clauses (Art. 44–49) | ⚠️ Missing |
| B5 | EU / UK representative (Art. 27) if EU / UK residents' data is processed regularly | 👤 Decide (often needed once TallyCheck Web has EU customers) |
| B6 | California section: categories collected in the last 12 months, sources, purposes, "we do not sell or share personal information", sensitive PI use, how to submit requests, verification, authorised agents, no discrimination, retention per category | ⚠️ Missing (one sentence today) |
| B7 | Honour Global Privacy Control signals if any tracking or ads are ever added | ✅ No tracking today; keep it that way or add GPC support |
| B8 | Cookie banner | ✅ Not needed while only essential cookies are used; needed before adding analytics |
| B9 | Records of processing (Art. 30) and a breach-response procedure (Art. 33–34: notify within 72 hours) | 👤 Internal documents to write |
| B10 | Contracts (DPAs) with website and email providers | 👤 Check they are signed |

### C. TallyCheck Web (hosted; separate code base) 👤

| # | Item | Status |
|---|---|---|
| C1 | Data Processing Agreement offered to customers (GDPR Art. 28) and CCPA service-provider terms | ⚠️ To prepare |
| C2 | Public list of sub-processors (hosting, email, backups) and notice of changes | ⚠️ To prepare |
| C3 | Export of all of a customer's data and deletion of the account and its books on request (and of backups within a stated time) | 👤 Check in the Web code base |
| C4 | Encryption in transit (HTTPS) and at rest, access logging, staff access only on request ("we only look at your books if you ask") | 👤 Check in the Web code base |
| C5 | Hosting region stated (EU customers often ask for EU hosting) | 👤 Decide |

## 4. Suggested order

1. **Now (text only):** PRIVACY.md updated (done); fix B1–B4 and B6 on the website privacy page;
   add BitLocker / FileVault advice and a retention section to the guides (A8, A13).
2. **Next app release:** in-app privacy link (A16), Claude warning (A14), "Export this person's
   data" (A10), "Anonymise" (A11).
3. **Before EU customers on TallyCheck Web:** DPA and sub-processor list (C1, C2), EU
   representative decision (B5), internal records and breach procedure (B9).
4. **Later:** HTTPS for LAN mode (A9) and an encrypted database (A8).
