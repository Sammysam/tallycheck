# Contributing to TallyCheck

Thank you for helping. Read [docs/design.md](docs/design.md) first: it holds the accounting
rules, the security model and the numbered decisions that the code refers to.

## Workflow

1. Create a branch from `main` (`feature/…`, `fix/…`, `docs/…`).
2. Keep each pull request to one purpose. Separate refactoring from behaviour changes.
3. Run the checks below; CI runs them again on Windows and macOS.
4. Open a pull request against `main` and fill in the template. `main` is changed only through
   reviewed pull requests.

## Commit messages

- A short summary in the imperative, at most 72 characters, with a type prefix:
  `feat:`, `fix:`, `docs:`, `test:`, `ci:`, `build:`, `refactor:`, `chore:`.
  Example: `fix: keep the file key readable after a restore on macOS`.
- A blank line, then a body that says **why** the change is needed and anything a reviewer
  should know. Mention the decision number (e.g. D53) when a change follows one.
- One logical change per commit.

## Checks

```
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
cd frontend && npm ci && npm run build
```

Or all at once on Windows: `powershell -ExecutionPolicy Bypass -File tools\ci.ps1`.

## Rules

- **Accounting:** every financial change posts a balanced journal entry in the same database
  transaction as its document. Posted entries are never edited or deleted; corrections are
  reversals. Money is integer cents, never floating point.
- **Migrations** in `migrations/sqlite` are append-only: never edit one that has shipped; add a
  new one.
- **Secrets and data:** never commit keys, passwords, certificates, `.env` files or real company
  data. Never log passwords, tokens, keys or request bodies. Signing keys and Apple / Microsoft
  credentials live only in the CI secret store or the company password manager.
- **Dependencies:** must allow use in a permissively licensed product (no GPL, AGPL, LGPL-only,
  SSPL, BUSL or proprietary). Prefer the standard library and existing crates; no extra servers
  or runtimes (the app is fully offline).
- **Tests:** new behaviour comes with tests; a bug fix comes with a test that failed before it.
  Never change an accounting or payroll formula to make a test pass.
- **Behaviour changes** that differ from the web edition are recorded as a new decision in
  `docs/design.md` §8.

## Releases

Update [CHANGELOG.md](CHANGELOG.md), bump the version in `Cargo.toml`,
`src-tauri/tauri.conf.json` and `frontend/package.json`, then push a tag `v<version>`. See
[docs/installer-and-updates.md](docs/installer-and-updates.md) and [docs/ci-cd.md](docs/ci-cd.md).
