# Free code signing with SignPath Foundation (D48)

SignPath Foundation signs installers of open source projects for free, which removes the
"Unknown publisher" in Windows' SmartScreen window for the direct-download installer. (The
Microsoft Store version is signed by Microsoft instead: `docs/microsoft-store.md`.)

Conditions checked on signpath.org/terms on 2026-10-30, and where TallyCheck stands:

| Condition | TallyCheck |
|---|---|
| OSI-approved license for all components, **no commercial dual-licensing** | MIT OR Apache-2.0 since 2026-10-30; the commercial license was removed. All dependencies are open source (THIRD-PARTY-NOTICES.txt). |
| No proprietary components | None |
| Binaries built from the public source in a verifiable way (CI configuration counts as source) | `.github/workflows/release.yml` on GitHub-hosted runners |
| Already released in the form to be signed | The NSIS installer (`TallyCheck_<version>_x64-setup.exe`) |
| The signing team also develops and maintains the code | Cenango's team, via github.com/Sammysam/tallycheck |
| Multi-factor authentication for every team member | **To do** — turn on 2FA on GitHub and SignPath |
| Roles: authors, reviewers, approvers; manual approval of every release | `CODE_SIGNING_POLICY.md`; approval in SignPath |
| "Code signing policy" on the project's home page | `CODE_SIGNING_POLICY.md`, linked from the README |
| Privacy: disclose data transfers, provide uninstall | In `CODE_SIGNING_POLICY.md` |
| Product name and version attributes set consistently | TallyCheck, version from `tauri.conf.json` |

The certificate is issued to **SignPath Foundation**, so Windows shows that name as publisher.
SignPath may decline a project, and may pause or revoke signing if the rules are broken.

## Steps (manual)

1. **Turn on two-factor authentication** for every GitHub account with write access to the repo.
2. **Apply** at signpath.org (the "Apply" form). Suggested text:
   > TallyCheck is a free, open source (MIT OR Apache-2.0) offline accounting, payroll and
   > invoicing application for Windows, written in Rust and React (Tauri 2), published at
   > https://github.com/Sammysam/tallycheck. Small businesses install it from our GitHub
   > releases; the unsigned installer triggers SmartScreen warnings. The installer is built by
   > GitHub Actions from the public repository (.github/workflows/release.yml), and our code
   > signing policy is CODE_SIGNING_POLICY.md. We would like to sign the NSIS installer
   > (TallyCheck_<version>_x64-setup.exe) for each release, with manual approval.
3. When accepted, in SignPath:
   * create the project with slug **`tallycheck`**, linked to the GitHub repository (trusted build
     system: GitHub.com);
   * add an artifact configuration with slug **`installer`** — paste
     `.signpath/artifact-configuration.xml`;
   * use the signing policy slug **`release-signing`** (or change it in the workflow), with
     manual approval by the approvers in `CODE_SIGNING_POLICY.md`;
   * create a CI user and an API token for it.
4. In GitHub → the repo → **Settings → Secrets and variables → Actions**, add:
   * secret `SIGNPATH_API_TOKEN` — the CI user's token;
   * variable `SIGNPATH_ORGANIZATION_ID`;
   * secret `TAURI_SIGNING_PRIVATE_KEY` — the contents of `updater.key` (the update key, D45);
   * secret `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` — its password.

   The update key then also lives in GitHub's encrypted secrets. Anyone who can change the
   workflow on `main` could use it, so keep write access to the repository small. The workflow
   gives the key only to the step that runs `tauri signer sign` on the SignPath-signed installer:
   `npm ci` (run with `--ignore-scripts`) and the cargo / Tauri build never have it. Third-party
   actions are pinned to full commit SHAs (the tag is in a comment); to update one, look up the
   new tag's commit (`git ls-remote https://github.com/<owner>/<action> refs/tags/<tag>`) and
   replace both.

## Making a release

1. Raise the version (`Cargo.toml` `[workspace.package]`, `src-tauri/tauri.conf.json`), commit,
   push.
2. Either push a tag `v<version>` (`git tag v1.0.0 && git push github v1.0.0`), or run the
   **Release** workflow by hand (Actions → Release → Run workflow) and type the release notes.
3. The workflow builds the installer and waits for the signing request. **Approve it in
   SignPath.**
4. It then makes the update signature from the signed file, writes `latest.json`, and creates a
   **draft** GitHub release with both files.
5. Check the draft (download the installer, run it on a test PC), write the notes, and click
   **Publish release**. Installed copies offer the update from then on.

Until SignPath accepts the project, the signing step fails and no release is drafted; build and
publish by hand with `tools/release.py` (`docs/installer-and-updates.md`).

**Not verifiable before signing works:** that SmartScreen stops warning. A newly signed
certificate may still need some downloads to build reputation; check on a clean PC with a
downloaded copy.
