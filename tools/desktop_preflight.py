"""The "Preserve" pre-flight of the fully local TallyCheck desktop app (docs/preserve.md).

It protects what cannot be replaced once the app is installed on a PC: the books in the user's
SQLite database (migrations), the data folder, the update key and address, the encrypted-file
format, and the installed copies' identity. Run on every push by tools/ci.ps1 (offline), and with
the release's tag at the start of a release (adds the checks that need git tags or the network):

    python tools/desktop_preflight.py                  # offline rules (CI)
    python tools/desktop_preflight.py v1.2.0           # + tags and the update feed (release)
    python tools/desktop_preflight.py --update-lock    # after a release: lock its migrations

Rules (same numbers as the web server's docs/preserve.md where they mean the same thing):

* P4  migrations append-only: every migration released so far (preserve/migrations.lock.json) is
      still here, byte for byte (a release also checks every earlier v* tag); new ones are newer.
* P5  no unapproved destructive migration: DROP TABLE/COLUMN/TRIGGER/VIEW, DELETE or UPDATE without
      WHERE, REPLACE INTO, in a migration not yet released, unless preserve/approvals.json approves
      that exact file.
* P6  migrations on a scratch copy: the released migrations are applied to an empty scratch SQLite
      database, then the new ones; no table, column, trigger or view may be lost; integrity check.
* P8  app identity unchanged: identifier, product name, update key, update address, data folder
      names, installer mode and Store package name equal preserve/desktop-identity.json. No override.
* P9  versions only go up: the app's two version numbers agree and are not below the last release;
      for a release: equal to the tag and higher than every earlier tag.
* P10 (release, warning only) the update feed answers with a valid latest.json.
* P12 data formats unchanged: the encrypted-file test fixture equals preserve/format-fixtures.json
      (files sealed by older versions must still open), unless approved.

Writes preflight-out/preflight.json, .md and .html. Exit 0 = passed, 1 = blocked, 2 = bad usage.
Never touches the real data folder: only a scratch database in memory.
"""
from __future__ import annotations

import hashlib
import html
import json
import re
import sqlite3
import subprocess
import sys
import urllib.error
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
OUT_DIR = ROOT / "preflight-out"
MIGRATIONS = "migrations/sqlite"
FIXTURE_FILES = ("crates/documents/tests/fixtures/reference_encryption.json",)
IDENTITY_FIELDS = ("identifier", "productName", "updaterPubkey", "updaterEndpoints", "updateFeed",
                   "dataDirName", "legacyDataDirName", "nsisInstallMode", "msixIdentityName")
APPROVAL_KEYS = {"migrations", "fixtures"}
NAME_RE = re.compile(r"^(\d{14})_[a-z0-9_]+\.sql$")


# ---- helpers ---------------------------------------------------------------------------------
def norm(data: bytes) -> bytes:
    """The file as git stores it (LF): a Windows checkout with CRLF must give the same hash."""
    return data.replace(b"\r\n", b"\n")


def sha384(data: bytes) -> str:
    return hashlib.sha384(norm(data)).hexdigest()


def sha256(data: bytes) -> str:
    return hashlib.sha256(norm(data)).hexdigest()


def read_json(path: Path):
    return json.loads(path.read_text(encoding="utf-8"))


def migration_files(root: Path) -> dict[str, bytes]:
    folder = root / MIGRATIONS
    return {p.name: p.read_bytes() for p in sorted(folder.glob("*.sql"))} if folder.is_dir() else {}


def git(root: Path, *args: str) -> subprocess.CompletedProcess:
    return subprocess.run(["git", *args], cwd=root, capture_output=True, check=False)


# ---- P4 ---------------------------------------------------------------------------------------
def migration_problems(lock: dict[str, str], files: dict[str, bytes]) -> list[str]:
    out = []
    for name in files:
        if not NAME_RE.match(name):
            out.append(f"{name}: not a migration name like 20261231000001_what_it_does.sql")
    versions = [n.split("_")[0] for n in files]
    for v in set(versions):
        if versions.count(v) > 1:
            out.append(f"version {v} is used by more than one migration")
    for name, digest in sorted(lock.items()):
        if name not in files:
            out.append(f"{name}: a released migration is missing (installed apps have applied it)")
        elif sha384(files[name]) != digest:
            out.append(f"{name}: a released migration was changed (installed apps recorded its checksum; add a new migration instead)")
    newest_locked = max((n.split("_")[0] for n in lock), default="")
    for name in files:
        if name not in lock and name.split("_")[0] <= newest_locked:
            out.append(f"{name}: a new migration must be newer than the last released one ({newest_locked})")
    return out


def baseline_exception(root: Path) -> tuple[str, str] | None:
    """D74: the baseline was edited once; the app accepts exactly the old file's SHA-384."""
    try:
        src = (root / "crates" / "database" / "src" / "lib.rs").read_text(encoding="utf-8")
    except OSError:
        return None
    v = re.search(r"BASELINE_VERSION:\s*i64\s*=\s*(\d+)", src)
    c = re.search(r'BASELINE_CHECKSUM_BEFORE_D74:\s*&str\s*=\s*"([0-9a-f]{96})"', src)
    return (v.group(1), c.group(1)) if v and c else None


def tag_migration_problems(root: Path, tags: list[str]) -> list[str]:
    """Every earlier release's migrations must still be here unchanged (except D74)."""
    now = {n: sha384(d) for n, d in migration_files(root).items()}
    exc = baseline_exception(root)
    out = []
    for tag in tags:
        listed = git(root, "ls-tree", "-r", "--name-only", tag, "--", MIGRATIONS)
        for path in listed.stdout.decode().split():
            name = path.rsplit("/", 1)[-1]
            blob = git(root, "show", f"{tag}:{path}")
            if blob.returncode != 0:
                continue
            old = hashlib.sha384(norm(blob.stdout)).hexdigest()
            if name not in now:
                out.append(f"{tag}: {name} is missing now")
            elif now[name] != old and not (exc and name.startswith(exc[0]) and old == exc[1]):
                out.append(f"{tag}: {name} differs from the released file")
    return out


# ---- P5 ---------------------------------------------------------------------------------------
def strip_sql(text: str) -> str:
    text = re.sub(r"/\*.*?\*/", " ", text, flags=re.S)
    text = re.sub(r"--[^\n]*", " ", text)
    return re.sub(r"'(?:[^']|'')*'", "''", text)


def destructive(text: str) -> list[str]:
    s = strip_sql(text)
    # Trigger bodies run later, when rows change; they are not part of the migration itself.
    s = re.sub(r"CREATE\s+(?:TEMP(?:ORARY)?\s+)?TRIGGER\b.*?\bEND\s*;", "CREATE TRIGGER x;", s, flags=re.S | re.I)
    found = []
    for m in re.finditer(r"\bDROP\s+(TABLE|COLUMN|TRIGGER|VIEW)\b", s, flags=re.I):
        found.append(f"DROP {m.group(1).upper()}")
    if re.search(r"\bALTER\s+TABLE\s+\S+\s+DROP\b", s, flags=re.I) and "DROP COLUMN" not in found:
        found.append("DROP COLUMN")
    if re.search(r"\bREPLACE\s+INTO\b|\bINSERT\s+OR\s+REPLACE\b", s, flags=re.I):
        found.append("REPLACE INTO (overwrites rows)")
    for stmt in s.split(";"):
        t = stmt.strip()
        if re.match(r"DELETE\s+FROM\b", t, flags=re.I) and not re.search(r"\bWHERE\b", t, flags=re.I):
            found.append("DELETE without WHERE")
        if re.match(r"UPDATE\b", t, flags=re.I) and not re.search(r"\bWHERE\b", t, flags=re.I):
            found.append("UPDATE without WHERE")
    return sorted(set(found))


def load_approvals(root: Path) -> tuple[dict, list[str]]:
    path = root / "preserve" / "approvals.json"
    if not path.is_file():
        return {"migrations": [], "fixtures": []}, []
    data = read_json(path)
    problems = [f"approvals.json: unknown key {k!r}" for k in data if k not in APPROVAL_KEYS]
    for kind in APPROVAL_KEYS:
        for item in data.get(kind, []):
            if not {"file", "sha256", "reason", "approvedBy"} <= set(item):
                problems.append(f"approvals.json: an entry in {kind!r} needs file, sha256, reason and approvedBy")
    return {k: data.get(k, []) for k in APPROVAL_KEYS}, problems


def approved(approvals: dict, kind: str, name: str, data: bytes) -> bool:
    return any(a["file"] == name and a["sha256"] == sha256(data) for a in approvals.get(kind, []))


def destructive_problems(lock: dict[str, str], files: dict[str, bytes], approvals: dict) -> tuple[list[str], list[str]]:
    problems, notes = [], []
    for name, data in files.items():
        if name in lock:
            continue
        found = destructive(data.decode("utf-8", "replace"))
        if not found:
            continue
        if approved(approvals, "migrations", name, data):
            notes.append(f"{name}: {', '.join(found)} (approved)")
        else:
            problems.append(f"{name}: {', '.join(found)}; add it to preserve/approvals.json (sha256 {sha256(data)}) only if the data it removes is truly unused")
    return problems, notes


# ---- P6 ---------------------------------------------------------------------------------------
def snapshot(db: sqlite3.Connection) -> dict:
    tables, triggers, views = {}, set(), set()
    for kind, name in db.execute("SELECT type, name FROM sqlite_master WHERE name NOT LIKE 'sqlite_%'"):
        if kind == "table":
            tables[name] = {r[1] for r in db.execute(f'PRAGMA table_info("{name}")')}
        elif kind == "trigger":
            triggers.add(name)
        elif kind == "view":
            views.add(name)
    return {"tables": tables, "triggers": triggers, "views": views}


def lost(before: dict, after: dict) -> list[str]:
    out = []
    for t, cols in before["tables"].items():
        if t not in after["tables"]:
            out.append(f"table {t} was lost")
        else:
            out.extend(f"column {t}.{c} was lost" for c in sorted(cols - after["tables"][t]))
    out.extend(f"trigger {t} was lost" for t in sorted(before["triggers"] - after["triggers"]))
    out.extend(f"view {v} was lost" for v in sorted(before["views"] - after["views"]))
    return out


def scratch_problems(lock: dict[str, str], files: dict[str, bytes], approvals: dict) -> tuple[list[str], list[str]]:
    db = sqlite3.connect(":memory:")
    problems, notes = [], []
    try:
        released = [n for n in sorted(files) if n in lock]
        fresh = [n for n in sorted(files) if n not in lock]
        for name in released:
            try:
                db.executescript(files[name].decode("utf-8"))
            except sqlite3.Error as e:
                return [f"{name} does not apply to an empty database: {e}"], notes
        before = snapshot(db)
        for name in fresh:
            try:
                db.executescript(files[name].decode("utf-8"))
            except sqlite3.Error as e:
                return [f"{name} does not apply on top of the released migrations: {e}"], notes
        gone = lost(before, snapshot(db))
        if gone:
            if any(approved(approvals, "migrations", n, files[n]) for n in fresh):
                notes.extend(f"{g} (an approved migration)" for g in gone)
            else:
                problems.extend(f"{g} after the new migrations: nothing may disappear from installed books" for g in gone)
        check = db.execute("PRAGMA integrity_check").fetchone()[0]
        if check != "ok":
            problems.append(f"integrity check of the migrated scratch database: {check}")
        bad = db.execute("PRAGMA foreign_key_check").fetchall()
        if bad:
            problems.append(f"foreign key check of the migrated scratch database found {len(bad)} problem(s)")
    finally:
        db.close()
    return problems, notes


# ---- P8 ---------------------------------------------------------------------------------------
def current_identity(root: Path) -> dict:
    conf = read_json(root / "src-tauri" / "tauri.conf.json")
    updater = conf.get("plugins", {}).get("updater", {})

    def find(path: str, pattern: str) -> str | None:
        try:
            m = re.search(pattern, (root / path).read_text(encoding="utf-8"))
        except OSError:
            return None
        return m.group(1).strip().strip("'\"") if m else None

    return {
        "identifier": conf.get("identifier"),
        "productName": conf.get("productName"),
        "updaterPubkey": updater.get("pubkey"),
        "updaterEndpoints": updater.get("endpoints"),
        "updateFeed": find("src-tauri/src/updates.rs", r'UPDATE_FEED:\s*Option<&str>\s*=\s*Some\("([^"]+)"\)'),
        "dataDirName": find("crates/app-core/src/config.rs", r'pub const APP_DIR_NAME:\s*&str\s*=\s*"([^"]+)"'),
        "legacyDataDirName": find("crates/app-core/src/config.rs", r'pub const LEGACY_APP_DIR_NAME:\s*&str\s*=\s*"([^"]+)"'),
        "nsisInstallMode": conf.get("bundle", {}).get("windows", {}).get("nsis", {}).get("installMode"),
        "msixIdentityName": find(".github/workflows/msix.yml", r"MSIX_IDENTITY_NAME:\s*(\S+)"),
    }


def identity_problems(locked: dict, now: dict) -> list[str]:
    out = []
    for field in IDENTITY_FIELDS:
        if locked.get(field) != now.get(field):
            out.append(f"{field} changed ({str(locked.get(field))[:60]!r} -> {str(now.get(field))[:60]!r}): "
                       "installed copies would stop updating, lose their data folder or become another program")
    return out


# ---- P9 / P10 ---------------------------------------------------------------------------------
def version_tuple(v: str) -> tuple[int, int, int] | None:
    m = re.fullmatch(r"v?(\d+)\.(\d+)\.(\d+)", v.strip())
    return (int(m[1]), int(m[2]), int(m[3])) if m else None


def app_versions(root: Path) -> tuple[str | None, str | None]:
    conf = read_json(root / "src-tauri" / "tauri.conf.json").get("version")
    cargo = None
    try:
        text = (root / "Cargo.toml").read_text(encoding="utf-8")
        section = re.search(r"\[workspace\.package\](.*?)(?:\n\[|\Z)", text, flags=re.S)
        m = re.search(r'^version\s*=\s*"([^"]+)"', section.group(1) if section else "", flags=re.M)
        cargo = m.group(1) if m else None
    except OSError:
        pass
    return conf, cargo


def version_problems(conf: str | None, cargo: str | None, locked: str | None, tag: str | None, earlier: list[str]) -> list[str]:
    out = []
    if conf != cargo:
        out.append(f"src-tauri/tauri.conf.json says {conf} but Cargo.toml says {cargo}")
    mine = version_tuple(conf or "")
    if mine is None:
        return out + [f"{conf!r} is not a version like 1.2.3"]
    last = version_tuple(locked or "")
    if last and mine < last:
        out.append(f"version {conf} is lower than the last release {locked}")
    if tag is not None:
        if version_tuple(tag) is None:
            out.append(f"tag {tag!r} is not a version like v1.2.3")
        elif version_tuple(tag) != mine:
            out.append(f"tag {tag} does not match the app version {conf}")
        others = [t for t in (version_tuple(x) for x in earlier) if t is not None and t != mine]
        if others and mine <= max(others):
            out.append(f"version {conf} is not higher than the newest earlier release {'.'.join(map(str, max(others)))}")
    return out


def feed_problems(feed: str | None, timeout: float = 20) -> list[str]:
    if not feed:
        return ["no update feed address found in src-tauri/src/updates.rs"]
    try:
        with urllib.request.urlopen(urllib.request.Request(feed, headers={"User-Agent": "tallycheck-preflight"}), timeout=timeout) as r:
            data = json.loads(r.read().decode("utf-8"))
    except urllib.error.HTTPError as e:
        return [f"the update feed answers HTTP {e.code} (no release published yet?)"]
    except (OSError, ValueError) as e:
        return [f"could not read the update feed: {e}"]
    problems = []
    if not isinstance(data.get("version"), str):
        problems.append("latest.json has no version")
    platforms = data.get("platforms") or {}
    if not platforms:
        problems.append("latest.json lists no platform")
    for name, p in platforms.items():
        if not p.get("url") or not p.get("signature"):
            problems.append(f"latest.json platform {name} has no url or signature")
    return problems


# ---- P12 --------------------------------------------------------------------------------------
def fixture_problems(root: Path, locked: dict[str, str], approvals: dict) -> tuple[list[str], list[str]]:
    problems, notes = [], []
    for rel in sorted(set(locked) | set(FIXTURE_FILES)):
        path = root / rel
        if rel not in locked:
            problems.append(f"{rel} is not in preserve/format-fixtures.json")
        elif not path.is_file():
            problems.append(f"{rel} is missing")
        elif sha256(path.read_bytes()) != locked[rel]:
            if approved(approvals, "fixtures", rel, path.read_bytes()):
                notes.append(f"{rel} changed (approved)")
            else:
                problems.append(f"{rel} changed: files written by older versions must still open (sha256 now {sha256(path.read_bytes())})")
    return problems, notes


# ---- the run ----------------------------------------------------------------------------------
def run(tag: str | None, root: Path = ROOT) -> dict:
    rules = []

    def rule(rid: str, title: str, problems: list[str], ok_note: str, notes: list[str] | None = None, skipped: bool = False) -> None:
        status = "skipped" if skipped else ("fail" if problems else ("warn" if notes else "pass"))
        details = problems or ([ok_note] if ok_note else [])
        rules.append({"id": rid, "title": title, "status": status, "details": details + ([] if problems else (notes or []))})

    pre = root / "preserve"
    try:
        lock_doc = read_json(pre / "migrations.lock.json")
        lock = lock_doc["migrations"]
        locked_version = lock_doc.get("version")
    except (OSError, ValueError, KeyError) as e:
        rule("P4", "Migrations append-only", [f"preserve/migrations.lock.json is missing or unreadable: {e}"], "")
        return {"tag": tag, "passed": False, "rules": rules}
    files = migration_files(root)
    approvals, approval_problems = load_approvals(root)

    problems = migration_problems(lock, files)
    note = None
    if tag is not None:
        git(root, "fetch", "--tags", "--quiet", "origin")
        tags = [t for t in git(root, "tag", "--list", "v*").stdout.decode().split() if t != tag]
        problems += tag_migration_problems(root, tags)
        if not tags:
            note = "no earlier release tags in this clone: checked only against the lock file"
    rule("P4", "Migrations append-only", problems,
         f"All {len(lock)} released migrations are present and unchanged; {len(files) - len(lock)} new.",
         [note] if note else None)

    p5, n5 = destructive_problems(lock, files, approvals)
    rule("P5", "No unapproved destructive migration", p5 + approval_problems, "No new migration deletes or overwrites data.", n5)

    p6, n6 = scratch_problems(lock, files, approvals)
    rule("P6", "Migrations on a scratch copy", p6,
         f"{len(lock)} released and {len(files) - len(lock)} new migration(s) applied to an empty scratch database; nothing was lost.", n6)

    try:
        locked_identity = read_json(pre / "desktop-identity.json")
        p8 = identity_problems(locked_identity, current_identity(root))
    except (OSError, ValueError) as e:
        p8 = [f"preserve/desktop-identity.json is missing or unreadable: {e}"]
    rule("P8", "App identity unchanged", p8, "Identifier, update key and address, data folder, installer mode and Store package match the locked record.")

    conf, cargo = app_versions(root)
    earlier: list[str] = []
    if tag is not None:
        earlier = [t for t in git(root, "tag", "--list", "v*").stdout.decode().split() if t != tag]
    rule("P9", "Versions only go up", version_problems(conf, cargo, locked_version, tag, earlier),
         f"Version {conf} is consistent" + (f" and higher than {len(earlier)} earlier release(s)." if tag else " and not below the last release."))

    if tag is None:
        rule("P10", "The update feed answers", [], "Not checked offline.", skipped=True)
    else:
        feed = current_identity(root).get("updateFeed")
        fp = feed_problems(feed)
        rules.append({"id": "P10", "title": "The update feed answers", "status": "warn" if fp else "pass",
                      "details": fp or [f"{feed} answers with a valid latest.json."]})

    try:
        fx = read_json(pre / "format-fixtures.json")
        p12, n12 = fixture_problems(root, fx, approvals)
    except (OSError, ValueError) as e:
        p12, n12 = [f"preserve/format-fixtures.json is missing or unreadable: {e}"], []
    rule("P12", "Data formats unchanged", p12, "The encrypted-file fixture is unchanged.", n12)

    return {"tag": tag, "passed": all(r["status"] != "fail" for r in rules), "rules": rules}


def update_lock(root: Path = ROOT) -> int:
    """After a release: lock its migrations (append-only: nothing already locked may change)."""
    pre = root / "preserve"
    pre.mkdir(exist_ok=True)
    path = pre / "migrations.lock.json"
    old = read_json(path)["migrations"] if path.is_file() else {}
    files = migration_files(root)
    bad = migration_problems(old, files)
    if bad:
        print("\n".join(bad))
        return 1
    conf, _ = app_versions(root)
    doc = {"version": conf, "migrations": {n: sha384(d) for n, d in sorted(files.items())}}
    path.write_text(json.dumps(doc, indent=2) + "\n", encoding="utf-8")
    print(f"locked {len(files)} migrations at version {conf}")
    return 0


def markdown(report: dict) -> str:
    lines = [f"# Desktop pre-flight (Preserve): {'PASSED' if report['passed'] else 'BLOCKED'}", ""]
    label = {"pass": "passed", "warn": "warning", "skipped": "skipped", "fail": "BLOCKED"}
    for r in report["rules"]:
        lines.append(f"- **{r['id']} {r['title']}**: {label[r['status']]}")
        lines.extend(f"  - {d}" for d in r["details"])
    return "\n".join(lines) + "\n"


def page(report: dict) -> str:
    colour = {"pass": "#2e9d5b", "warn": "#d69a00", "skipped": "#7a869a", "fail": "#d64545"}
    label = {"pass": "passed", "warn": "warning", "skipped": "skipped", "fail": "BLOCKED"}
    rows = "".join(
        f'<tr><td><b>{r["id"]}</b></td><td>{html.escape(r["title"])}</td><td style="color:{colour[r["status"]]};font-weight:700">{label[r["status"]]}</td>'
        f'<td>{"<br>".join(html.escape(d) for d in r["details"])}</td></tr>' for r in report["rules"])
    head = "PASSED" if report["passed"] else "BLOCKED"
    return (f'<!doctype html><meta charset="utf-8"><title>Desktop pre-flight</title>'
            f'<style>body{{font:16px Segoe UI,Arial,sans-serif;margin:32px;color:#1c2433}}table{{border-collapse:collapse;width:100%}}'
            f'td{{border-bottom:1px solid #d8dee9;padding:10px;vertical-align:top}}</style>'
            f'<h1>Desktop pre-flight (Preserve): {head}</h1>{"<p>Release " + html.escape(report["tag"]) + "</p>" if report["tag"] else ""}'
            f'<table>{rows}</table>')


def main(argv: list[str]) -> int:
    if "--update-lock" in argv:
        return update_lock()
    tags = [a for a in argv[1:] if a.startswith("v")]
    if len(tags) > 1 or any(a.startswith("-") and a not in ("--offline",) for a in argv[1:]):
        print(__doc__)
        return 2
    report = run(tags[0] if tags else None)
    OUT_DIR.mkdir(exist_ok=True)
    (OUT_DIR / "preflight.json").write_text(json.dumps(report, indent=2) + "\n", encoding="utf-8")
    text = markdown(report)
    (OUT_DIR / "preflight.md").write_text(text, encoding="utf-8")
    (OUT_DIR / "preflight.html").write_text(page(report), encoding="utf-8")
    print(text)
    return 0 if report["passed"] else 1


if __name__ == "__main__":
    sys.exit(main(sys.argv))
