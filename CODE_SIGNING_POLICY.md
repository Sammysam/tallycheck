# Code signing policy

Free code signing provided by [SignPath.io](https://about.signpath.io/), certificate by
[SignPath Foundation](https://signpath.org/).

## What is signed

Only the TallyCheck Windows installer (`TallyCheck_<version>_x64-setup.exe`), built by GitHub
Actions (`.github/workflows/release.yml`) from the source code in this repository. Nothing else
is signed, and nothing built outside that workflow is signed.

## Team roles

| Role | Members |
|---|---|
| Committers and reviewers | [Sammysam](https://github.com/Sammysam) |
| Approvers (approve every signing request) | [Sammysam](https://github.com/Sammysam) |

Every member uses multi-factor authentication on GitHub and SignPath. Every release is approved
by hand in SignPath before it is signed.

## Privacy policy

This program will not transfer any information to other networked systems unless specifically
requested by the user or the person installing or operating it. In detail:

* The accounting data stays on the PC it is installed on (`%LOCALAPPDATA%\TallyCheckData`).
* When an Administrator opens the app, it checks
  `https://github.com/Sammysam/tallycheck/releases` for a newer version; no accounting data is
  sent. Updates are installed only when the Administrator clicks **Update**.
* Online services (Shopify, WooCommerce, PayPal, Stripe, the Claude connection) are contacted
  only after the user connects them in Settings → Integrations, and only for what that
  integration does.
* LAN mode, which lets other PCs in the office use the app, is off until an administrator turns
  it on.

## Installing and removing

The installer installs for the current Windows user and adds an uninstaller (Settings → Apps).
Uninstalling removes the program; the accounting data folder is kept on purpose, so that a
company's books are never lost by reinstalling.
