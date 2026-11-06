# TallyCheck on macOS (D53)

One universal app runs on Apple silicon (M1 and later) and Intel Macs with macOS 11 or later. It
is the same program as on Windows: the same screens, the same server and the same database.

| | Windows | macOS |
|---|---|---|
| Download | `TallyCheck-setup.exe`, Microsoft Store | `TallyCheck_<version>_universal.dmg` |
| Data folder | `%LOCALAPPDATA%\TallyCheckData` | `~/Library/Application Support/TallyCheckData` |
| Secret keys (sign-in key, file-encryption key, connector keys) | DPAPI | AES-256-GCM under a key in the user's **login Keychain** ("TallyCheck data protection") |
| Forgot password | recovery key, or Windows sign-in | recovery key only |
| Backups by default | `Documents\Cenango Backups` | `~/Documents/Cenango Backups` |
| Claude connector | `tallycheck-mcp.exe` next to the app | `TallyCheck.app/Contents/MacOS/tallycheck-mcp` |
| In-app updates | yes | **not yet**: download the new .dmg (the update feed has no Mac entry) |

The data folder must be on the Mac's own disk: a folder on a network volume is refused, as on
Windows. As with DPAPI, the secret files cannot be opened on another Mac or by another user: move
books to a new computer with **Settings → Backup & restore** and keep the file-encryption key's
offline recovery copy (`docs/backup-and-recovery.md`).

## Building

Mac apps can only be built on macOS.

* **On a Mac:** install Xcode Command Line Tools (`xcode-select --install`), rustup, Node.js 24
  and Python 3, then run `bash tools/build_macos.sh`. The disk image is in
  `target/universal-apple-darwin/release/bundle/dmg/`.
* **On GitHub:** Actions → **macOS app** → Run workflow (or push a tag `v<version>`). The .dmg is
  attached to the run. CI also runs every check on a Mac (`checks-macos` in `ci.yml`).

Without the signing variables the app is **unsigned**: fine for testing, but macOS refuses to open
it ("cannot be opened because Apple cannot check it") until the user right-clicks it → Open, or
allows it under System Settings → Privacy & Security.

## Signing and notarization (company Apple Developer account)

Done once by the account's **Account Holder or an Admin**:

1. **Developer ID Application certificate.** On a Mac: Keychain Access → Certificate Assistant →
   Request a Certificate From a Certificate Authority (saved to disk). developer.apple.com →
   Certificates, Identifiers & Profiles → Certificates → **+** → *Developer ID Application* →
   upload the request → download and double-click the certificate. In Keychain Access, export
   it together with its private key as a **.p12** with a strong password.
2. **Team ID**: developer.apple.com → Account → Membership details.
3. **App Store Connect API key** (for notarization): App Store Connect → Users and Access →
   Integrations → App Store Connect API → **+**, access *Developer*. Note the **Issuer ID** and
   **Key ID** and download the **.p8** (it can be downloaded only once).

Store the .p12, its password and the .p8 in the company password manager. **Never** put them in
the repository, a chat, an e-mail or a ticket.

**GitHub** (Settings → Secrets and variables → Actions → New repository secret):

| Secret | Value |
|---|---|
| `APPLE_CERTIFICATE` | the .p12, base64: `base64 -i certificate.p12 \| pbcopy` |
| `APPLE_CERTIFICATE_PASSWORD` | the .p12 password |
| `APPLE_SIGNING_IDENTITY` | `Developer ID Application: <Company name> (<TEAM ID>)` |
| `APPLE_API_ISSUER` | the Issuer ID |
| `APPLE_API_KEY` | the Key ID |
| `APPLE_API_KEY_P8` | the whole text of the .p8 file |

**On a Mac by hand:** the certificate is already in the login Keychain after step 1, so set
`APPLE_SIGNING_IDENTITY`, `APPLE_API_ISSUER`, `APPLE_API_KEY` and `APPLE_API_KEY_PATH` (the .p8's
path) in the terminal before `bash tools/build_macos.sh`.

Tauri then signs the app and the Claude connector with the hardened runtime, sends the app to
Apple's notary service, and staples the ticket to it. A signed and notarized .dmg opens without
any warning.

## Checking a build on a Mac

```
spctl -a -vvv -t install /Applications/TallyCheck.app   # "accepted, source=Notarized Developer ID"
codesign --verify --deep --strict -vvv /Applications/TallyCheck.app
```

Then: first run → create the administrator and save the recovery key → company setup → an
invoice, a payment, a report → Settings → Backup & restore (back up and verify) → quit and reopen
(the Keychain key must still open the books) → Settings → Integrations → Claude shows the
connector path.

## Not covered yet

* In-app updates on macOS (`tools/release.py` and `latest.json` are Windows-only).
* The Mac App Store (it needs the App Sandbox; the direct .dmg does not).
* Password recovery with Touch ID (the Windows sign-in option has no Mac version yet).
