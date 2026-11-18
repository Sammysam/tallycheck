# TallyCheck in the Mac App Store (D58, in progress)

**Status:** not released. This branch prepares a sandboxed Mac App Store build next to the
website download. Nothing here changes the website `.dmg`.

| | Website download | Mac App Store |
|---|---|---|
| File | `TallyCheck_<version>_universal.dmg` | `TallyCheck_<version>.pkg` (uploaded with Transporter) |
| Built by | `tools/build_macos.sh` (`tauri.macos.json`) | `tools/build_macos_appstore.sh` (`tauri.macos-appstore.json`) |
| Signed with | Developer ID Application + notarization | Apple Distribution + Mac Installer Distribution |
| App Sandbox | no | **yes** (`src-tauri/macos/appstore.entitlements`) |
| Updates | download the new .dmg | the App Store (`store` feature: no in-app updater) |
| Claude connector | bundled (`tallycheck-mcp`) | **not bundled yet** (open item 2) |

## Entitlements

`src-tauri/macos/appstore.entitlements`: the App Sandbox; network server (the built-in server on
127.0.0.1:47800); network client (Shopify / WooCommerce / PayPal / Stripe, the AI model
download); read-write access to files the person picks (backups, restore, imports, exports,
documents, the logo); app-scoped security bookmarks (to remember the backup folder). The build
script adds `com.apple.application-identifier` and `com.apple.developer.team-identifier` from
`APPLE_TEAM_ID`.

## Open items (must be done and tested on a Mac before submitting)

1. **Backup folder.** A sandboxed app cannot write to `~/Documents/Cenango Backups` by itself.
   The backup settings must let the person pick the folder (open panel), keep a security-scoped
   bookmark of it, and open the bookmark before each backup, including scheduled ones. Restore
   and imports already go through a picked file.
2. **Claude connector.** Claude Desktop starts `tallycheck-mcp`, not TallyCheck. An embedded
   helper signed with `com.apple.security.inherit` only works when the sandboxed app starts it.
   Test whether a separately signed sandboxed helper works when Claude starts it; otherwise offer
   the connector as a separate download (it only needs network access to 127.0.0.1:47800) and
   change Settings → Integrations → Claude in this build to point there.
3. **Data folder and Keychain.** In the sandbox, `~/Library/Application Support/TallyCheckData`
   is inside the app's container, and the Keychain item is the App Store app's own. Books from
   the website version are not visible there: people move them with **Backup & restore** (keep the
   file-encryption key's recovery copy), as between the Windows installer and the Microsoft Store
   (D55). The first-run screen of the App Store version should say so.
4. **LAN mode.** Off by default. Either leave it out of this build or be ready to explain to App
   Review that it lets other PCs in the office use the same books.
5. **Test with the sandbox on:** first run, sign-in, recovery key, invoices, payments, reports,
   PDFs, backup to a picked folder (also scheduled) and restore, imports (CSV / Excel), export,
   documents, connectors, the AI model download (AI test edition only), quit and reopen.

## Apple account (Account Holder or Admin)

1. **Identifiers → +**: App ID with the bundle ID `lk.cenango.accounting` (as in
   `src-tauri/tauri.conf.json`).
2. **Certificates → +**: **Apple Distribution** and **Mac Installer Distribution** (made like the
   Developer ID one, with a request from Keychain Access).
3. **Profiles → +**: a **Mac App Store** provisioning profile for that App ID and the Apple
   Distribution certificate; download it.
4. **App Store Connect → Apps → +**: a new macOS app (name, bundle ID, SKU). Fill in the
   description, screenshots, privacy policy URL (`PRIVACY.md`), the privacy label (TallyCheck
   collects no data: everything stays on the Mac), age rating and price.

Keep the certificates (.p12) and the profile in the company password manager; never in this
repository.

## Build and upload (on a Mac)

```
export APPLE_SIGNING_IDENTITY="Apple Distribution: <Company> (<TEAM ID>)"
export APPLE_INSTALLER_IDENTITY="3rd Party Mac Developer Installer: <Company> (<TEAM ID>)"
export APPLE_TEAM_ID="<TEAM ID>"
export APPLE_PROVISIONING_PROFILE="$HOME/Downloads/TallyCheck_Mac_App_Store.provisionprofile"
bash tools/build_macos_appstore.sh
```

Then upload `target/macos-appstore/TallyCheck_<version>.pkg` with **Transporter** (free in the
Mac App Store), select the build in App Store Connect and **Submit for Review** (usually 1–3
days). The version must be higher than any build uploaded before.
