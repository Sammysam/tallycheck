# CI / CD

| What | Where | When | File |
|---|---|---|---|
| **CI**: Preserve pre-flight, security audit, formatting, frontend build, clippy, all tests | GitHub Actions (Windows and macOS, GitHub's machines) | every push and pull request on GitHub | `.github/workflows/ci.yml` |
| **CI**: the same checks (this is where the checks are watched) | Bitbucket Pipelines (Windows, **self-hosted runner**) | every push on Bitbucket; tag `v*` adds the release pre-flight; weekly audit (schedule below) | `bitbucket-pipelines.yml` |
| **CD**: installer (not code-signed) → update signature → draft GitHub release | GitHub Actions | run by hand only | `.github/workflows/release.yml` (D77) |
| Microsoft Store MSIX (no secrets: the Store signs it) | GitHub Actions (Windows) or this PC | by hand or tag `v<version>` | `.github/workflows/msix.yml`, `python tools/build_msix.py` (D47, `docs/microsoft-store.md`) |
| macOS app (.dmg), signed and notarized when the Apple secrets are set | GitHub Actions (Mac) or a Mac | by hand or tag `v<version>` | `.github/workflows/macos.yml`, `tools/build_macos.sh` (D53, `docs/macos.md`) |

Both CI systems run one script, `tools/ci.ps1`, so they always check the same things. Run it
yourself before pushing:

```
powershell -ExecutionPolicy Bypass -File tools\ci.ps1
```

`npm run lint` is not part of CI: the copied frontend has 57 problems inherited from the
reference (docs/design.md §9).

What the script checks, in this order:

1. Formatting.
2. **Preserve** (`docs/preserve.md`): migrations, data formats, app identity, versions.
3. **Security audit** (`tools/audit.ps1`): `cargo audit` fails on a known vulnerability in
   `Cargo.lock` (exceptions with reasons in `.cargo/audit.toml`); `npm audit` fails on high or
   critical advisories in the shipped packages (`frontend/` and the root) and only warns about
   build tools. Advisories are published at any time, so also schedule it (below).
4. Frontend build, clippy, all tests.

On Bitbucket, a green **Checks (Windows)** step means all of that passed; a red one says which step
and why. The Preserve report (`preflight-out/preflight.html`) is kept as a build artifact.

**Weekly audit on Bitbucket**: Repository -> Pipelines -> Schedules -> New schedule -> branch
`main`, pipeline `custom: audit`, every week. A failure there means a new advisory: update the
dependency, or, if it truly does not apply, add it to `.cargo/audit.toml` with the reason.

## GitHub Actions

Nothing to set up for CI: push to the `github` remote (`git push github main`) and look under
the repository's **Actions** tab. Windows minutes are free for a public repository.

## Bitbucket Pipelines: self-hosted Windows runner (one-time setup)

Bitbucket's own build machines are Linux, and TallyCheck builds only on Windows, so the
pipeline runs on a Windows PC of yours that stays switched on (a spare PC is better than a
developer's PC: builds use a lot of CPU).

1. **Turn Pipelines on**: repository → Repository settings → Pipelines → Settings → Enable.
2. **Prepare the PC**: Git for Windows, Python 3 (`python --version`), rustup, Node.js 24, Visual Studio Build Tools with
   "Desktop development with C++", the WebView2 runtime, and OpenJDK 11 or later (the runner is
   a Java program). Open a new PowerShell window and check `git`, `cargo`, `node` and `java`
   all answer `--version`.
3. **Add the runner**: Repository settings → Pipelines → Runners → **Add runner** → System
   *Windows*. Keep the labels `self.hosted` and `windows`. Bitbucket shows a PowerShell command
   with the runner's own secrets: copy it straight into PowerShell on the runner PC; never put
   it in the repository, a chat or a ticket.
4. Run the command. The runner shows **ONLINE** in Bitbucket. To start it with Windows, follow
   Bitbucket's "Windows runner as a service" instructions (Task Scheduler or NSSM).
5. Push a commit. Pipelines shows the step **Checks (Windows)** running on the runner.

The first build takes a while (it compiles every dependency). Later builds are quicker because
`CARGO_TARGET_DIR` is `%USERPROFILE%\tallycheck-ci-target`, outside the clone the runner deletes.
Delete that folder if it grows too big.

Build minutes on a self-hosted runner do not count against Bitbucket's plan.

## Releasing (CD)

1. Change the version in `src-tauri/tauri.conf.json` and `Cargo.toml` (`python tools/bump_version.py patch`) and commit. CI must be green (Preserve checks that the two agree and that the version only goes up).
2. Push the commit and a tag `v<version>` to GitHub: `git tag v1.0.1 && git push github main v1.0.1`.
   The tag builds the Microsoft Store package (`msix.yml`) and the Mac app (`macos.yml`).
3. For the Microsoft Store, download the .msix from the **Microsoft Store package** run (or build it on
   this PC) and upload it in Partner Center. The Mac app is submitted from its own run (`docs/macos.md`).
4. On GitHub, draft a release from the tag with that version's notes (no files) and publish it.
5. Only if a direct Windows download is wanted: run `release.yml` by hand (Actions → Release → Run
   workflow). It builds the installer, signs the update (needs the update-key secrets) and makes a
   **draft** release with the files. The installer is not code-signed, so SmartScreen warns.
6. After the release is published: `python tools/desktop_preflight.py --update-lock` and commit
   `preserve/migrations.lock.json` in a pull request of its own (it freezes the migrations that
   shipped; `docs/preserve.md`).
