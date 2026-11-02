# Installing TallyCheck

**Download:** https://github.com/Sammysam/tallycheck/releases/latest/download/TallyCheck-setup.exe

## 1. Run the installer

Open `TallyCheck-setup.exe`. It installs for your Windows user only — no administrator rights are
needed — and adds TallyCheck to the Start menu.

## 2. If Windows shows "Windows protected your PC"

TallyCheck is new, and Windows shows this blue window for programs it has not seen often yet. It
does not mean anything is wrong with the file.

1. Click **More info** (under the text).
2. Check that the app is **TallyCheck-setup.exe** (or `TallyCheck_<version>_x64-setup.exe`).
3. Click **Run anyway**.

Each install like this helps Windows learn that the file is safe, and the window goes away by
itself as more people install TallyCheck.

**Another way:** right-click the downloaded file → **Properties** → tick **Unblock** at the bottom
→ **OK**, then open it. A copy from a USB stick or a shared office folder usually opens without
the window.

## 3. Check the file (optional)

Every release lists the installer's SHA-256 on the release page. To compare, open PowerShell in
the folder with the file and run:

```
Get-FileHash .\TallyCheck-setup.exe -Algorithm SHA256
```

The value must match the one on the release page.

## 4. First start

TallyCheck asks you to create the first administrator, then to enter your company details. Your
books are kept on this PC in `%LOCALAPPDATA%\TallyCheckData`.

## Updates and uninstalling

* When a new version is published, TallyCheck shows **Update to x.y.z** at the bottom of the
  window (for administrators). Click it; TallyCheck updates itself and opens again.
* Uninstall from **Settings → Apps → Installed apps → TallyCheck**. Your accounting data is kept on
  purpose; make a backup first (**File → Backup and restore**) if you plan to move to another PC.
