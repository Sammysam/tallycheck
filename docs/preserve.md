# Preserve: protecting installed copies and their books

This app runs entirely on the user's PC: the books are a SQLite file in the data folder, and the app
program is replaced by every update. **Preserve** is a pre-flight that runs on every push and at the start
of every release and blocks the change when something that cannot be replaced is at risk. It is not the
code CI (that checks that the code works); it checks that **an installed copy, with real books, will
still work after the update**. The security audit of dependencies is separate (`tools/audit.ps1`, `docs/ci-cd.md`) and runs in the same
step. (The other two TallyCheck apps have the same idea; the web server's
`docs/preserve.md` has their full list. This app has no server, so it has no keys to check and no
production database to copy.)

What cannot be replaced once installed: the user's database (its shape, as migrations), the data folder,
the update key and address, the format of encrypted files, and the app's identity on the PC and in the Store.

## The rules

| # | Rule | When | Override |
|---|---|---|---|
| P4 | **Migrations append-only.** Every migration released so far (`preserve/migrations.lock.json`) is still here, byte for byte; new ones are newer than the last released one; names are well formed and versions unique. A release also compares with every earlier `v*` tag (the one known edit, D74, is allowed exactly as the app allows it) | every push, every release | none |
| P5 | **No unapproved destructive migration.** A migration not yet released may not `DROP TABLE/COLUMN/TRIGGER/VIEW`, `DELETE` or `UPDATE` without `WHERE`, or `REPLACE INTO`. (Trigger bodies are not part of the migration.) | every push, every release | approval file |
| P6 | **Migrations on a scratch copy.** The released migrations are applied to an empty in-memory SQLite database, then the new ones; no table, column, trigger or view may disappear; `integrity_check` and `foreign_key_check` must be clean | every push, every release | an approved P5 migration |
| P8 | **App identity unchanged.** `identifier`, `productName`, the update public key, the update address, the data folder names (`TallyCheckData`, `CenangoAccounting`), the installer mode and the Microsoft Store package name equal `preserve/desktop-identity.json` | every push, every release | none |
| P9 | **Versions only go up.** `tauri.conf.json` and `Cargo.toml` agree and are not below the last release; for a release: equal to the tag and higher than every earlier tag | every push (first half), every release | none |
| P10 | **The update feed answers** with a valid `latest.json` (url and signature for each platform). A warning only: the release itself replaces the feed | every release | warning only |
| P12 | **Data formats unchanged.** `crates/documents/tests/fixtures/reference_encryption.json` equals `preserve/format-fixtures.json`: files sealed by an older version must still open | every push, every release | approval file |

`python tools/desktop_preflight.py` runs the offline rules (what CI runs); `python tools/desktop_preflight.py v1.2.0`
adds the tag and feed checks. Both write `preflight-out/preflight.html`, `.md` and `.json`, which the Bitbucket
pipeline keeps as a build artifact (and GitHub's release prints). It never touches the real data folder.

## Where it runs

* **Every push** (Bitbucket Pipelines and GitHub Actions): `tools/ci.ps1` runs the pre-flight's own tests, then the
  offline rules, before the long build and test steps, so a blocked change fails in seconds.
* **A release** `v<version>`: the Bitbucket tag pipeline runs **Preserve pre-flight (release)** after the checks;
  GitHub's `release.yml` runs it before anything is built (`Preserve pre-flight`).

## Why P8 has no override

Each value is built into every installed copy. Change the identifier and Windows treats the app as another
program; change the data folder name and the app no longer finds the books; change the update key and no
installed copy accepts an update again; change the update address and copies look somewhere else; change the Store
package name and the Store treats it as a different app. No release can repair that afterwards. If one is truly
needed (a new edition, a new key after a leak), plan it outside a release with a way for existing installs to
follow, and only then change `preserve/desktop-identity.json`, in its own reviewed pull request.

## Locking the migrations after a release

The lock holds the SHA-384 of every migration that has been **released** (the checksum SQLx records in
every installed database). After a release is published:

```
python tools/desktop_preflight.py --update-lock
```

It only adds: it refuses to run when a locked file changed or disappeared. Commit
`preserve/migrations.lock.json` in a pull request of its own. Until then, migrations added since the last release are
"new": they may still be edited, and P5 scans them. After the lock, they are frozen for good: to change one, add a
newer migration.

## Approvals (P5, P6, P12)

`preserve/approvals.json`, changed in the same pull request and reviewed like code:

```json
{
  "migrations": [
    { "file": "20261230000001_drop_old_notes.sql", "sha256": "<the file's SHA-256>",
      "reason": "notes was replaced by memo in 1.2.0 and has been empty since", "approvedBy": "saman" }
  ],
  "fixtures": []
}
```

The report prints the SHA-256 to use. An approval covers exactly that file's content: edit the migration and it
no longer applies. Unknown keys and incomplete entries are refused. A table rebuild (create new, copy, drop old,
rename) contains `DROP TABLE` and needs an approval too; say in `reason` that the data is copied. Approve a
migration that removes data only when the data is truly unused: the app copies the database to `pre-upgrade\`
before migrating, but a user may not notice a loss for weeks.

## What this does not check

* The real data of any PC: only a scratch database made from the migrations. Rows that migrations convert
  (`UPDATE ... WHERE`) are covered by the Rust tests, not here.
* Behaviour (that totals are right): that is `cargo test`.
* The backup file format and the sealed key files beyond the one fixture: a change there is caught only if
  the fixture changes. Keep a test for any format you add.

## Tests

`tools/test_desktop_preflight.py` (24 tests, run by CI): changed, missing and misordered migrations, every
destructive pattern and its look-alikes, approvals tied to exact content, a lost table, column or trigger, SQL that
does not apply, every identity change, version rules, a changed fixture, the real migrations applying, the locked
identity matching this checkout, and the D74 checksum being the real old file's.
