# Installer, signing and updates

## Building the installer

```
cd frontend && npm ci && npm run build && cd ..
npm ci                 # pinned Tauri CLI only
npm run installer      # = build cenango-mcp, then tauri build with src-tauri/tauri.installer.json
```

The result is `target\release\bundle\nsis\Cenango Accounting_<version>_x64-setup.exe`:

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
    "target\release\bundle\nsis\Cenango Accounting_<version>_x64-setup.exe"
  ```

No certificate is available to this project, so the installers built here are **unsigned**.

## Updates

| Situation | How it works |
|---|---|
| Daily work | Fully offline after installation. Nothing contacts the internet. |
| Installing | Offline, from the installer file (USB stick or a share). |
| Updating | Run the newer installer. It replaces the program and keeps the data. At the next start, the database is copied to `pre-upgrade\` before any migration runs. |
| Automatic updates | **Not enabled.** Tauri's updater (`tauri-plugin-updater`) only works with an internet connection and a signed update feed. It cannot update a PC without internet, so it is not promised. If the office later wants it, it needs an update signing key pair, an HTTPS location for the update manifest, and internet on the main PC. |
| Offline update package | The signed installer *is* the offline update package. Distribute it with its SHA-256 (`certutil -hashfile <file> SHA256`) so the office can check it before running it. |

Before any update: make a backup (*File → Backup and restore…*) and keep it off the PC.

Going back to an older version after an upgrade is not supported, because the database may have been migrated. Restore the pre-upgrade copy or a backup with the older version instead.
