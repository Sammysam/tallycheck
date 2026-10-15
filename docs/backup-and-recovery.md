# Backup, restore and recovery

## What a backup is

One file, `cenango-accounting-YYYYMMDD-HHMMSS.qbak`, holding:

| Inside | Notes |
|---|---|
| The database | A consistent `VACUUM INTO` snapshot, checked with `PRAGMA integrity_check` and `foreign_key_check` before it is packed. |
| Every stored file | Employee photos, documents and filed payslips, still in their own AES-256-GCM encryption, plus their content-type sidecars. |
| The file-encryption key | So the files can be opened after a restore on another PC. |
| A manifest | Format and app version, database version, record counts, the SHA-256 and size of every entry, and files the records point to that were already missing. |

The whole file is encrypted with AES-256-GCM in 1 MiB authenticated chunks. Each chunk's nonce includes its position and a final-chunk flag, and the header is bound to every chunk. A truncated, reordered, altered or re-headed backup therefore fails to open instead of restoring silently wrong data. Each backup has its own random data key. That key is stored in the header, wrapped with a key derived from the **backup password** using Argon2id (64 MiB, 3 passes).

## Consistency

Every write in the app goes through one SQLite writer connection. A backup holds that connection while it takes the snapshot and hard-links the stored files into a work folder. Stored files are never modified in place. They are written once under a new name and deleted only after the database change that drops them has committed. So no record in the snapshot can point at a file that was not captured. Packing, encryption and verification then run without blocking anyone.

## Making backups

* **Back up now**: *Tools → Backup and restore…* in the desktop app, or `accounting-server backup <folder>`.
* **Daily**: set a folder on the same page. While the app runs it checks every hour. It backs up when the last good backup is more than a day old, so an office PC that is off at night still gets one backup a day. Daily backups go to `<folder>\scheduled`, and only the newest *N* are kept (14 by default). Files you made by hand are never deleted.
* A backup is written as `*.qbak.partial`, read back completely and checked against its manifest, and only then renamed to `*.qbak`. A backup interrupted by a power cut or a full disk never looks finished, and its leftover `.partial` is removed by the next run. The page shows the last success and the last error.
* Only an Administrator **on the main PC** can use backup and restore. Requests from other PCs are refused even in LAN mode.

## Restoring

1. *Tools → Backup and restore… → Restore*, choose the `.qbak` file and type `RESTORE`. For a backup made with a different password, or on another PC, enter its password.
2. The backup is decrypted into `<data>\restore-staging` and every entry is checked against the manifest. The database is integrity-checked, and a backup from a *newer* app version is refused. Nothing live changes yet.
3. Close and reopen the app. Before the database opens, the staged data replaces the live data. The replaced data is kept in `<data>\pre-restore-<time>`. If the PC loses power during the swap, the next start completes it.
4. An older backup is upgraded by the normal database migrations. A copy of the database is taken first (see below).

From the command line, with the app closed: `accounting-server restore <file>`.

## Updates never touch the only copy

When an updated app finds a database that its migrations will change, it first writes `<data>\pre-upgrade\accounting-v<old version>-<time>.db`.

## Keys: what can and cannot be recovered

| Secret | Where | If lost |
|---|---|---|
| Backup password | Only in the administrator's head or on paper. The app keeps a DPAPI-protected key derived from it, so daily backups run unattended. | Backups made with it can still be restored **on this PC** without it. On any other PC they **cannot be opened**. Set a new password and make a new backup at once. |
| File-encryption key | `secrets\storage-key.dpapi` (DPAPI, this Windows user), and inside every backup | Recovered from any backup. `accounting-server export-storage-key` prints it for an extra offline copy. |
| JWT signing key | `secrets\jwt-secret.dpapi` | Not needed. A restore on a new PC creates a new one and users simply sign in again. |

DPAPI ties the `.dpapi` files to the Windows user account. Copying the data folder to another PC therefore does **not** carry the keys. Use a backup and its password instead.

Changing the backup password does not re-encrypt older backups. Each one still needs the password it was made with.

## Recovery scenarios (tested)

| Situation | What to do | Test |
|---|---|---|
| PC lost or broken | Install the app on the new PC, then restore the latest backup with its password. | `backup::tests::a_backup_restores_on_a_new_pc_with_only_the_password` |
| Bad data entered | Restore yesterday's backup; the current state is kept in `pre-restore-…`. | `restore_replaces_live_data_and_keeps_the_old_copy` |
| Backup file damaged or cut short | Refused before anything changes. | `damaged_backups_are_refused_and_nothing_is_staged`, format tests |
| Power cut during a backup or a restore | The next backup removes the partial file; the next start finishes the restore. | `interrupted_backups_and_restores_recover` |
| No backup password set | Daily backup records an error that the page shows. Nothing is written. | `scheduled_backups_need_a_password_and_keep_the_newest` |
| App update | The database is copied before migrating. | `database::tests::an_older_database_is_copied_before_it_is_upgraded` |

Keep at least one recent backup **off this PC** (external drive kept elsewhere, or a second office PC's share). A backup on the same disk does not survive a disk failure or theft.
