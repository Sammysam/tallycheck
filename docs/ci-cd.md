# CI / CD

| What | Where | When | File |
|---|---|---|---|
| **CI**: formatting, frontend build, npm advisories, clippy, all tests | GitHub Actions (Windows, GitHub's machines) | every push and pull request on GitHub | `.github/workflows/ci.yml` |
| **CI**: the same checks | Bitbucket Pipelines (Windows, **self-hosted runner**) | every push on Bitbucket | `bitbucket-pipelines.yml` |
| **CD**: installer → SignPath code signing → update signature → draft GitHub release | GitHub Actions | tag `v<version>` or run by hand | `.github/workflows/release.yml` (D48, `docs/signpath.md`) |
| Microsoft Store MSIX | this PC | by hand | `python tools/build_msix.py` (D47, `docs/microsoft-store.md`) |

Both CI systems run one script, `tools/ci.ps1`, so they always check the same things. Run it
yourself before pushing:

```
powershell -ExecutionPolicy Bypass -File tools\ci.ps1
```

`npm run lint` is not part of CI: the copied frontend has 57 problems inherited from the
reference (CLAUDE.md §10). `cargo audit` is not either: it lists advisories that do not apply
(§10); run it by hand before a release.

## GitHub Actions

Nothing to set up for CI: push to the `github` remote (`git push github main`) and look under
the repository's **Actions** tab. Windows minutes are free for a public repository.

## Bitbucket Pipelines: self-hosted Windows runner (one-time setup)

Bitbucket's own build machines are Linux, and TallyCheck builds only on Windows, so the
pipeline runs on a Windows PC of yours that stays switched on (a spare PC is better than a
developer's PC: builds use a lot of CPU).

1. **Turn Pipelines on**: repository → Repository settings → Pipelines → Settings → Enable.
2. **Prepare the PC**: Git for Windows, rustup, Node.js 24, Visual Studio Build Tools with
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

1. Change the version (`src-tauri/tauri.conf.json`) and commit. CI must be green.
2. Push the commit and a tag `v<version>` to GitHub: `git tag v1.0.1 && git push github main v1.0.1`.
3. `release.yml` builds, asks SignPath to sign (someone approves it in SignPath), and makes a
   **draft** release. Check it, then publish it: only a published release reaches the app.
4. For the Microsoft Store, build the MSIX on this PC and upload it in Partner Center.
