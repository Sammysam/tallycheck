"""Tests of tools/desktop_preflight.py (run by tools/ci.ps1). No network."""
import hashlib
import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import desktop_preflight as p  # noqa: E402

A = "20261001000001_create_a.sql"
B = "20261002000001_create_b.sql"
FILES = {A: b"CREATE TABLE a (id INTEGER PRIMARY KEY, name TEXT);\n", B: b"CREATE TABLE b (id INTEGER PRIMARY KEY);\n"}


def lock_of(files):
    return {n: p.sha384(d) for n, d in files.items()}


class Migrations(unittest.TestCase):
    def test_unchanged_files_pass(self):
        self.assertEqual(p.migration_problems(lock_of(FILES), FILES), [])

    def test_a_changed_released_migration_is_blocked(self):
        changed = dict(FILES, **{A: FILES[A] + b"-- edit\n"})
        out = p.migration_problems(lock_of(FILES), changed)
        self.assertEqual(len(out), 1)
        self.assertIn("was changed", out[0])

    def test_a_removed_released_migration_is_blocked(self):
        out = p.migration_problems(lock_of(FILES), {A: FILES[A]})
        self.assertTrue(any("missing" in o for o in out))

    def test_a_new_migration_must_be_newer_than_the_last_release(self):
        older = dict(FILES, **{"20260901000001_old.sql": b"SELECT 1;\n"})
        self.assertTrue(any("newer" in o for o in p.migration_problems(lock_of(FILES), older)))
        newer = dict(FILES, **{"20261003000001_new.sql": b"SELECT 1;\n"})
        self.assertEqual(p.migration_problems(lock_of(FILES), newer), [])

    def test_bad_names_and_duplicate_versions_are_blocked(self):
        files = {"oops.sql": b"x", "20261005000001_a.sql": b"x", "20261005000001_b.sql": b"y"}
        out = p.migration_problems({}, files)
        self.assertTrue(any("oops.sql" in o for o in out))
        self.assertTrue(any("more than one" in o for o in out))

    def test_line_endings_do_not_change_the_checksum(self):
        self.assertEqual(p.sha384(b"a\r\nb\r\n"), p.sha384(b"a\nb\n"))


class Destructive(unittest.TestCase):
    def test_what_is_flagged(self):
        for sql, what in (("DROP TABLE x;", "DROP TABLE"), ("alter table t drop column c;", "DROP COLUMN"),
                          ("DROP TRIGGER t;", "DROP TRIGGER"), ("DELETE FROM x;", "DELETE without WHERE"),
                          ("UPDATE x SET a = 1;", "UPDATE without WHERE"), ("REPLACE INTO x VALUES (1);", "REPLACE INTO (overwrites rows)"),
                          ("INSERT OR REPLACE INTO x VALUES (1);", "REPLACE INTO (overwrites rows)")):
            self.assertIn(what, p.destructive(sql), sql)

    def test_what_is_not_flagged(self):
        for sql in ("DELETE FROM x WHERE id = 1;", "UPDATE x SET a = 1 WHERE b = 2;", "CREATE TABLE x (a INTEGER);",
                    "ALTER TABLE x ADD COLUMN y TEXT;", "-- DROP TABLE x;\nSELECT 1;", "INSERT INTO x VALUES ('DROP TABLE y');",
                    "/* DELETE FROM x; */ SELECT 1;"):
            self.assertEqual(p.destructive(sql), [], sql)

    def test_trigger_bodies_are_not_part_of_the_migration(self):
        sql = "CREATE TRIGGER t BEFORE UPDATE ON x BEGIN DELETE FROM y; UPDATE z SET a = 1; END;"
        self.assertEqual(p.destructive(sql), [])

    def test_released_migrations_are_not_rescanned_and_approvals_are_tied_to_the_exact_file(self):
        bad = {"20261010000001_drop.sql": b"DROP TABLE a;\n"}
        lock = lock_of(FILES)
        problems, _ = p.destructive_problems(lock, dict(FILES, **bad), {"migrations": []})
        self.assertEqual(len(problems), 1)
        self.assertIn(p.sha256(bad["20261010000001_drop.sql"]), problems[0])
        ok = {"migrations": [{"file": "20261010000001_drop.sql", "sha256": p.sha256(bad["20261010000001_drop.sql"]), "reason": "r", "approvedBy": "x"}]}
        problems, notes = p.destructive_problems(lock, dict(FILES, **bad), ok)
        self.assertEqual((problems, len(notes)), ([], 1))
        edited = dict(FILES, **{"20261010000001_drop.sql": b"DROP TABLE a; DROP TABLE b;\n"})
        self.assertEqual(len(p.destructive_problems(lock, edited, ok)[0]), 1)
        self.assertEqual(p.destructive_problems(lock_of(dict(FILES, **bad)), dict(FILES, **bad), {"migrations": []})[0], [])

    def test_approvals_file_refuses_unknown_keys_and_incomplete_entries(self):
        root = Path(tempfile.mkdtemp())
        (root / "preserve").mkdir()
        (root / "preserve" / "approvals.json").write_text(json.dumps({"migration": [], "fixtures": [{"file": "x"}]}), encoding="utf-8")
        _, problems = p.load_approvals(root)
        self.assertEqual(len(problems), 2)


class Scratch(unittest.TestCase):
    def test_clean_upgrade_passes(self):
        new = {"20261003000001_add.sql": b"ALTER TABLE a ADD COLUMN extra TEXT;\n"}
        problems, _ = p.scratch_problems(lock_of(FILES), dict(FILES, **new), {"migrations": []})
        self.assertEqual(problems, [])

    def test_a_lost_table_column_or_trigger_is_blocked(self):
        files = {A: b"CREATE TABLE a (id INTEGER, name TEXT);\nCREATE TRIGGER keep BEFORE DELETE ON a BEGIN SELECT RAISE(ABORT, 'no'); END;\n"}
        for sql, what in ((b"DROP TABLE a;\n", "table a"), (b"ALTER TABLE a DROP COLUMN name;\n", "column a.name"), (b"DROP TRIGGER keep;\n", "trigger keep")):
            new = {"20261003000001_x.sql": sql}
            problems, _ = p.scratch_problems(lock_of(files), dict(files, **new), {"migrations": []})
            self.assertTrue(any(what in x for x in problems), (what, problems))

    def test_an_approved_migration_may_remove_things(self):
        sql = b"ALTER TABLE a DROP COLUMN name;\n"
        name = "20261003000001_x.sql"
        approvals = {"migrations": [{"file": name, "sha256": p.sha256(sql), "reason": "r", "approvedBy": "x"}]}
        problems, notes = p.scratch_problems(lock_of(FILES), dict(FILES, **{name: sql}), approvals)
        self.assertEqual(problems, [])
        self.assertTrue(any("a.name" in n for n in notes))

    def test_sql_that_does_not_apply_is_reported_not_raised(self):
        problems, _ = p.scratch_problems(lock_of(FILES), dict(FILES, **{"20261003000001_x.sql": b"CREATE TABEL oops;\n"}), {"migrations": []})
        self.assertEqual(len(problems), 1)
        self.assertIn("does not apply", problems[0])

    def test_the_real_migrations_apply(self):
        files = p.migration_files(p.ROOT)
        problems, _ = p.scratch_problems({n: "x" for n in files}, files, {"migrations": []})
        self.assertEqual(problems, [])


def project(**changes) -> Path:
    root = Path(tempfile.mkdtemp())
    for d in ("src-tauri/src", "crates/app-core/src", ".github/workflows"):
        (root / d).mkdir(parents=True)
    conf = {"identifier": changes.get("identifier", "lk.x.app"), "productName": "App",
            "plugins": {"updater": {"pubkey": changes.get("pubkey", "KEY"), "endpoints": []}},
            "bundle": {"windows": {"nsis": {"installMode": changes.get("mode", "currentUser")}}}}
    (root / "src-tauri" / "tauri.conf.json").write_text(json.dumps(conf), encoding="utf-8")
    (root / "src-tauri" / "src" / "updates.rs").write_text(
        f'const UPDATE_FEED: Option<&str> = Some("{changes.get("feed", "https://example.com/latest.json")}");', encoding="utf-8")
    (root / "crates" / "app-core" / "src" / "config.rs").write_text(
        f'pub const APP_DIR_NAME: &str = "{changes.get("dir", "AppData")}";\npub const LEGACY_APP_DIR_NAME: &str = "Old";', encoding="utf-8")
    (root / ".github" / "workflows" / "msix.yml").write_text(f'  MSIX_IDENTITY_NAME: {changes.get("msix", "Co.App")}\n', encoding="utf-8")
    return root


class Identity(unittest.TestCase):
    def test_an_unchanged_app_passes_and_every_stranding_change_is_caught(self):
        locked = p.current_identity(project())
        self.assertTrue(all(v is not None for v in locked.values()), locked)
        self.assertEqual(p.identity_problems(locked, p.current_identity(project())), [])
        for change, field in (({"identifier": "lk.other"}, "identifier"), ({"pubkey": "NEW"}, "updaterPubkey"),
                              ({"feed": "https://evil.example/x"}, "updateFeed"), ({"dir": "Other"}, "dataDirName"),
                              ({"mode": "perMachine"}, "nsisInstallMode"), ({"msix": "Co.Other"}, "msixIdentityName")):
            out = p.identity_problems(locked, p.current_identity(project(**change)))
            self.assertEqual(len(out), 1, change)
            self.assertTrue(out[0].startswith(field), out)

    def test_the_locked_record_matches_this_checkout(self):
        locked = json.loads((p.ROOT / "preserve" / "desktop-identity.json").read_text(encoding="utf-8"))
        self.assertEqual(p.identity_problems(locked, p.current_identity(p.ROOT)), [])


class Versions(unittest.TestCase):
    def test_rules(self):
        self.assertEqual(p.version_problems("1.2.0", "1.2.0", "1.1.1", None, []), [])
        self.assertTrue(p.version_problems("1.2.0", "1.1.9", "1.1.1", None, []))      # the two files disagree
        self.assertTrue(p.version_problems("1.0.9", "1.0.9", "1.1.1", None, []))      # below the last release
        self.assertEqual(p.version_problems("1.2.0", "1.2.0", "1.1.1", "v1.2.0", ["v1.1.1", "v1.0.0"]), [])
        self.assertTrue(p.version_problems("1.2.0", "1.2.0", "1.1.1", "v1.2.1", []))  # tag is not the app version
        self.assertTrue(p.version_problems("1.1.0", "1.1.0", None, "v1.1.0", ["v1.1.1"]))  # not above the newest tag
        self.assertTrue(p.version_problems("one", "one", None, None, []))

    def test_this_checkout_has_matching_versions(self):
        conf, cargo = p.app_versions(p.ROOT)
        self.assertEqual(conf, cargo)


class Fixtures(unittest.TestCase):
    def test_a_changed_fixture_is_blocked_unless_approved(self):
        root = Path(tempfile.mkdtemp())
        f = root / p.FIXTURE_FILES[0]
        f.parent.mkdir(parents=True)
        f.write_text('{"a": 1}', encoding="utf-8")
        locked = {p.FIXTURE_FILES[0]: p.sha256(f.read_bytes())}
        self.assertEqual(p.fixture_problems(root, locked, {"fixtures": []}), ([], []))
        f.write_text('{"a": 2}', encoding="utf-8")
        self.assertEqual(len(p.fixture_problems(root, locked, {"fixtures": []})[0]), 1)
        ok = {"fixtures": [{"file": p.FIXTURE_FILES[0], "sha256": p.sha256(f.read_bytes()), "reason": "r", "approvedBy": "x"}]}
        self.assertEqual(p.fixture_problems(root, locked, ok)[0], [])


class Whole(unittest.TestCase):
    def test_this_checkout_passes_offline(self):
        report = p.run(None)
        failed = [r for r in report["rules"] if r["status"] == "fail"]
        self.assertEqual(failed, [], p.markdown(report))

    def test_the_d74_exception_is_the_real_old_checksum(self):
        exc = p.baseline_exception(p.ROOT)
        self.assertIsNotNone(exc)
        blob = subprocess.run(["git", "show", f"v1.0.0:{p.MIGRATIONS}/{exc[0]}_baseline.sql"], cwd=p.ROOT, capture_output=True, check=False)
        if blob.returncode != 0:
            self.skipTest("tag v1.0.0 is not in this clone")
        self.assertEqual(hashlib.sha384(p.norm(blob.stdout)).hexdigest(), exc[1])

    def test_reports_are_written_in_three_forms(self):
        report = {"tag": None, "passed": False, "rules": [{"id": "P4", "title": "T", "status": "fail", "details": ["<bad>"]}]}
        self.assertIn("BLOCKED", p.markdown(report))
        self.assertIn("&lt;bad&gt;", p.page(report))


if __name__ == "__main__":
    unittest.main(verbosity=1)
