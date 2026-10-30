# Installer, signing and updates

## Building the installer

```
cd frontend && npm ci && npm run build && cd ..
npm ci                 # pinned Tauri CLI only
npm run installer      # = build cenango-mcp, then tauri build with src-tauri/tauri.installer.json
```

The result is `target\release\bundle\nsis\TallyCheck_<version>_x64-setup.exe`:

* An NSIS installer for the current Windows user, so no administrator rights are needed. The React app ships as a resource; the API server is inside the executable.
* D41: `cenango-mcp.exe` (the Claude connector) is installed next to the app, as a resource added by `src-tauri/tauri.installer.json`; development builds do not need it.
* First start on a new PC: the app has no users, so it shows **Create administrator**. That screen works only on the PC itself and disappears once the first account exists (D17).
* The WebView2 bootstrapper is embedded. On Windows 10/11, WebView2 is normally present already. On a PC without it and without internet, use Microsoft's offline WebView2 installer once.
* The data folder `%LOCALAPPDATA%\CenangoAccounting` is **not** touched by install, upgrade or uninstall.

## Code signing

An unsigned installer works, but Windows SmartScreen warns about it. Signing needs the company's code-signing certificate, from a CA or an internal CA trusted by the office PCs, installed on the build PC. Then either:

* set `bundle.windows.certificateThumbprint` in `src-tauri/tauri.conf.json` to the certificate's thumbprint and build; or
* sign afterwards:

  ```
  signtool sign /fd sha256 /tr http://timestamp.digicert.com /td sha256 /sha1 <thumbprint> ^
    "target\release\bundle\nsis\TallyCheck_<version>_x64-setup.exe"
  ```

No certificate is available to this project, so the installers built here are **unsigned**.

## Updates

| Situation | How it works |
|---|---|
| Daily work | Fully offline after installation. Only the update check goes online (below). |
| In-app update (D45) | When an Administrator opens TallyCheck on the main PC, the app quietly checks the release page. A newer version shows **Update to x.y.z** in the status bar; **Help → Check for updates…** checks on request. **Update** downloads the new installer, verifies its signature, closes the database, installs and opens the new version. No internet: nothing is shown; the app works as before. |
| LAN clients | They load the app from the main PC, so they follow it; only the main PC updates. |
| Offline PCs | Run the newer installer from a USB stick. It replaces the program and keeps the data. |
| Every upgrade | At the next start the database is copied to `pre-upgrade\` before any migration runs. |

### How an update is trusted

Every update is signed with Cenango's **update key** (minisign, Tauri's updater format). The app
holds only the public key (`plugins.updater.pubkey` in `src-tauri/tauri.conf.json`) and refuses an
update whose signature does not verify: "The update could not be verified as coming from Cenango,
so it was not installed." This is separate from Windows code signing (see above).

* The private key and its password were created on the release PC in
  `%USERPROFILE%\.tallycheck-signing\` (`updater.key`, `updater.key.password.txt`). **Move both
  into the company password manager and keep a second copy offline.** They are never in the
  project. Without them no further update can be published: installed copies would have to be
  updated by hand with a new installer that carries a new public key.
* Anyone holding the key can publish an update every installation accepts. Keep it to the people
  who release TallyCheck.

### Publishing a release

1. Raise the version in `Cargo.toml` (`[workspace.package] version`) and `src-tauri/tauri.conf.json`.
2. `python tools/release.py --notes "What's new" --download-base https://<release page>/v{version}`
   — signs and builds the installer (`npm run installer`) and writes `latest.json` next to it.
3. Upload `TallyCheck_<version>_x64-setup.exe` to the download address and replace the
   `latest.json` that the app's feed points at.

The feed address is `UPDATE_FEED` in `src-tauri/src/updates.rs` (HTTPS only).
**It is not set yet**: until the release page exists, the app reports that it is not set up to
update itself. `ACCOUNTING_UPDATE_URL` overrides it for testing (a debug build also accepts http).

Before any update: make a backup (*File → Backup and restore…*) and keep it off the PC.

Going back to an older version after an upgrade is not supported, because the database may have been migrated. Restore the pre-upgrade copy or a backup with the older version instead.
