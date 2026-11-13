# Microsoft Store (MSIX) — D47

TallyCheck is published in the Microsoft Store as an **MSIX package**. The Store signs it with a
Microsoft certificate after certification, so no code-signing certificate has to be bought, and
the Store delivers updates. The direct-download installer (`npm run installer`, NSIS) is a
separate build and is unchanged.

Checked against Microsoft's documentation on 2026-10-30:

* Developer accounts are free for individuals and companies: the company fee was removed on
  2026-05-07. Open the account at **storedeveloper.microsoft.com** — other entry points show the
  older sign-up, where a fee may still apply. An Individual account cannot be turned into a
  Company account later.
* MSIX packages need no CA certificate: "The Microsoft Store will automatically re-sign your
  MSIX/AppX packages with a Microsoft certificate". EXE/MSI submissions are *not* re-signed and
  must be signed by us, which is why the Store build is MSIX.
* Store version numbers: four parts, the first not 0, the fourth reserved (0).

## What is different in the Store build

| | Direct installer | Microsoft Store (MSIX) |
|---|---|---|
| Build | `npm run installer` | `python tools/build_msix.py …` |
| Signed by | nobody yet (SmartScreen warns) | Microsoft, after certification |
| Updates | in-app updater (D45, GitHub releases) | the Store; the in-app updater is not compiled in (`--features store`), and Help → Check for updates says so |
| Data folder | `%LOCALAPPDATA%\TallyCheckData` (or an existing `CenangoAccounting`, D46) | `%USERPROFILE%\TallyCheckData` (below, D55) |
| Executable | installed by NSIS | `TallyCheck.exe` inside the package |

### Keeping the books on update and uninstall

An MSIX app's writes to AppData go to a private copy that Windows **deletes when the app is
uninstalled**. Excluding the data folder from that needs the restricted capability
`unvirtualizedResources`, which the Store **refused** for TallyCheck (2026-11-13). So the Store
build keeps the books in the user's own folder, **`%USERPROFILE%\TallyCheckData`** (for example
`C:\Users\Anne\TallyCheckData`), outside AppData: MSIX does not redirect it, so the books survive
updates and uninstalling. `tools/build_msix.py` compiles this in with `ACCOUNTING_STORE_EDITION=1`
(D55). That folder is not one OneDrive backs up (Desktop, Documents, Pictures), which matters for
the live database.

The package asks only for **`runFullTrust`**, which every desktop app needs. The direct installer
keeps `%LOCALAPPDATA%\TallyCheckData`; someone moving between the two moves the books with
**Settings → Backup & restore**.

## 1. Partner Center (manual — needs a person from Cenango)

1. Go to **storedeveloper.microsoft.com** and choose **Company**. Sign in with a work account
   (Microsoft Entra ID) if Cenango has one. Business verification: a D-U-N-S number makes it
   faster; without one, upload the company registration documents (manual review, up to three
   appeals).
2. **Apps and games → New product → MSIX or PWA app.** Reserve the name **TallyCheck**. (Check
   the trademark question in the risk list first: "Tally" is Tally Solutions' trademark.)
3. Open **Product management → Product identity** and copy, exactly:
   * `Package/Identity/Name`
   * `Package/Identity/Publisher` (starts with `CN=`)
   * `Package/Properties/PublisherDisplayName`
4. Prepare a **privacy policy** page (an https address). TallyCheck keeps employees' salaries
   and bank details on the customer's PC, sends nothing to Cenango, goes online only for the
   integrations the customer switches on, and sends data to Anthropic only if the customer turns
   on the Claude connection. The page must say this.

## 2. Build the package

```
cd frontend && npm ci && npm run build && cd ..
python tools/build_msix.py --identity-name "<Name>" --publisher "CN=<…>" --publisher-display-name "<PublisherDisplayName>"
```

The name reserved in Partner Center is **TallyCheck.io**, so add `--display-name "TallyCheck.io"`
(Package/Properties/DisplayName must be a reserved name, or the upload is refused). Cenango's
values: `--identity-name "Cenango.TallyCheck.io" --publisher "CN=10446E21-680A-45CC-AC6E-7844866B02E5"
--publisher-display-name "Cenango"`.

Result: `target\msix\TallyCheck_<version>_x64.msix` (unsigned — the Store signs it). Raise the
version (Cargo.toml `[workspace.package]` and `src-tauri/tauri.conf.json`) before each new
submission; the Store takes the highest version.

Optional before uploading: run the **Windows App Certification Kit** on the package.

## 3. Submission

1. **Pricing and availability:** markets, price (free or paid), release date.
2. **Properties:** category *Business* (subcategory *Accounting and finance*), privacy policy
   address, support contact.
3. **Age ratings:** the questionnaire (no user-generated content shared between users, no
   purchases inside the app).
4. **Packages:** upload `TallyCheck_<version>_x64.msix`.
5. **Restricted capabilities — justification** (Submission options):
   > *runFullTrust*: TallyCheck is a desktop accounting application (Win32, Tauri/WebView2) that
   > runs its own local database and a local web server for its window.
6. **Store listing:** description, at least one screenshot (1366×768 or larger), the app icon
   (`src-tauri/icons/icon-512.png` works as the 300×300+ logo after resizing), search terms.
7. Submit. Certification usually takes a few business days; Microsoft may ask questions about the
   restricted capabilities.

## 4. Testing on a PC before submitting

The test package uses a made-up identity and its own data folder (`TallyCheckStoreTest`), so it
never opens a real company's books, even on a PC that has some.

```
python tools/build_msix.py --test                       # target\msix\layout-test + …_test.msix
Add-AppxPackage -Register target\msix\layout-test\AppxManifest.xml   # Developer Mode on
```

It appears in Start as **TallyCheck (test)**. Remove it with
`Get-AppxPackage TallyCheck.StoreTest | Remove-AppxPackage`; its data folder stays, and can be
deleted by hand.

**Not verifiable before certification:** that the Store-installed copy shows no SmartScreen
warning and carries Microsoft's signature. Check that on a clean Windows PC after the app is
live, by installing it from the Store.
