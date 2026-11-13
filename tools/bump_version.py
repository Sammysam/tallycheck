"""Sets a new TallyCheck version everywhere at once, then commits and tags it.

    python tools/bump_version.py patch          # 1.0.0 -> 1.0.1  (fixes)
    python tools/bump_version.py minor          # 1.0.0 -> 1.1.0  (new features)
    python tools/bump_version.py major          # 1.0.0 -> 2.0.0
    python tools/bump_version.py 1.2.3          # an exact version
    python tools/bump_version.py patch --dry-run    # show what would change, change nothing
    python tools/bump_version.py patch --no-commit  # change the files only

Then `git push origin main --follow-tags` and `git push github main --follow-tags`: the tag
v<version> starts the Release workflow (installer, update file, GitHub release), the Microsoft
Store package and the macOS app on GitHub (docs/releasing.md).

The version lives in Cargo.toml (all crates), Cargo.lock, src-tauri/tauri.conf.json and
frontend/package.json + package-lock.json. CHANGELOG.md: everything under "## [Unreleased]"
becomes "## [<version>] - <today>" and an empty Unreleased section is left on top.

    python tools/bump_version.py --notes 1.0.1  # prints that version's CHANGELOG section
"""

from __future__ import annotations

import argparse
import datetime as dt
import json
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SEMVER = re.compile(r"^(\d+)\.(\d+)\.(\d+)$")


def read(rel: str) -> str:
    return (ROOT / rel).read_text(encoding="utf-8")


def write(rel: str, text: str, dry: bool) -> None:
    if not dry:
        # newline="" keeps each file's own line endings.
        with open(ROOT / rel, "w", encoding="utf-8", newline="") as f:
            f.write(text)


def current_version() -> str:
    return json.loads(read("src-tauri/tauri.conf.json"))["version"]


def next_version(current: str, how: str) -> str:
    if SEMVER.match(how):
        return how
    m = SEMVER.match(current)
    if not m:
        sys.exit(f"The current version {current!r} is not x.y.z")
    major, minor, patch = map(int, m.groups())
    if how == "major":
        return f"{major + 1}.0.0"
    if how == "minor":
        return f"{major}.{minor + 1}.0"
    if how == "patch":
        return f"{major}.{minor}.{patch + 1}"
    sys.exit("Say patch, minor, major or an exact version such as 1.2.3")


def replace_once(text: str, pattern: str, repl: str, what: str) -> str:
    new, n = re.subn(pattern, repl, text, count=1, flags=re.MULTILINE)
    if n != 1:
        sys.exit(f"Could not find the version in {what}")
    return new


def workspace_crates() -> set[str]:
    out = subprocess.run(
        ["cargo", "metadata", "--no-deps", "--format-version", "1"],
        cwd=ROOT, capture_output=True, text=True, check=True,
    ).stdout
    return {p["name"] for p in json.loads(out)["packages"]}


def bump_lock(text: str, crates: set[str], old: str, new: str) -> str:
    """Our own crates' entries in Cargo.lock (dependencies keep their versions)."""
    def fix(m: re.Match) -> str:
        return m.group(0).replace(f'version = "{old}"', f'version = "{new}"') if m.group(1) in crates else m.group(0)
    return re.sub(r'\[\[package\]\]\nname = "([^"]+)"\nversion = "[^"]+"', fix, text.replace("\r\n", "\n"))


def changelog_with_release(text: str, version: str, today: str) -> str:
    head = "## [Unreleased]"
    if head not in text:
        sys.exit("CHANGELOG.md has no '## [Unreleased]' section")
    if f"## [{version}]" in text:
        sys.exit(f"CHANGELOG.md already has a section for {version}")
    before, after = text.split(head, 1)
    nxt = re.search(r"^## \[", after, flags=re.MULTILINE)
    body = after[: nxt.start()] if nxt else after
    rest = after[nxt.start():] if nxt else ""
    if not body.strip():
        sys.exit("Nothing is written under '## [Unreleased]' in CHANGELOG.md: add what changed first")
    return f"{before}{head}\n\n## [{version}] - {today}\n\n{body.strip()}\n\n{rest}"


def notes(version: str) -> str:
    """The CHANGELOG section of `version`, without its heading."""
    text = read("CHANGELOG.md").replace("\r\n", "\n")
    m = re.search(rf"^## \[{re.escape(version)}\][^\n]*\n(.*?)(?=^## \[|\Z)", text, flags=re.MULTILINE | re.DOTALL)
    return m.group(1).strip() if m else ""


def git(*args: str) -> str:
    return subprocess.run(["git", *args], cwd=ROOT, capture_output=True, text=True, check=True).stdout.strip()


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("how", nargs="?", help="patch, minor, major or x.y.z")
    ap.add_argument("--dry-run", action="store_true", help="show what would change, change nothing")
    ap.add_argument("--no-commit", action="store_true", help="change the files but do not commit or tag")
    ap.add_argument("--notes", metavar="VERSION", help="print that version's CHANGELOG section and stop")
    a = ap.parse_args()

    if a.notes:
        sys.stdout.reconfigure(encoding="utf-8")
        print(notes(a.notes))
        return
    if not a.how:
        ap.error("say patch, minor, major or x.y.z")

    old = current_version()
    new = next_version(old, a.how)
    if new == old:
        sys.exit(f"The version is already {old}")
    tag = f"v{new}"
    if not a.dry_run and not a.no_commit:
        if git("status", "--porcelain", "--untracked-files=no"):
            sys.exit("Commit or stash your changes first: the version commit must contain only the version")
        if git("tag", "--list", tag):
            sys.exit(f"The tag {tag} already exists")

    today = dt.date.today().isoformat()
    changes = {
        "Cargo.toml": replace_once(read("Cargo.toml"), r'^(\[workspace\.package\]\r?\nversion = )"[^"]+"',
                                   rf'\g<1>"{new}"', "Cargo.toml [workspace.package]"),
        "Cargo.lock": bump_lock(read("Cargo.lock"), workspace_crates(), old, new),
        "src-tauri/tauri.conf.json": replace_once(read("src-tauri/tauri.conf.json"),
                                                  r'^(  "version": )"[^"]+"', rf'\g<1>"{new}"', "tauri.conf.json"),
        "frontend/package.json": replace_once(read("frontend/package.json"),
                                              r'^(  "version": )"[^"]+"', rf'\g<1>"{new}"', "frontend/package.json"),
        "CHANGELOG.md": changelog_with_release(read("CHANGELOG.md"), new, today),
    }
    lock = json.loads(read("frontend/package-lock.json"))
    lock["version"] = new
    lock["packages"][""]["version"] = new
    changes["frontend/package-lock.json"] = json.dumps(lock, indent=2, ensure_ascii=False) + "\n"

    print(f"TallyCheck {old} -> {new}")
    for rel, text in changes.items():
        changed = text != read(rel)
        print(f"  {'changed' if changed else 'same   '}  {rel}")
        write(rel, text, a.dry_run)
    if a.dry_run:
        print("\nDry run: nothing was changed. Notes for this version:\n")
        print(changelog_with_release(read("CHANGELOG.md"), new, today).split(f"## [{new}] - {today}\n", 1)[1].split("\n## [", 1)[0].strip())
        return
    if a.no_commit:
        print("\nFiles changed; nothing committed.")
        return
    git("add", *changes.keys())
    git("commit", "-m", f"Release {tag}")
    git("tag", "-a", tag, "-m", f"TallyCheck {new}")
    print(f"\nCommitted and tagged {tag}. To publish it:")
    print("  git push origin main --follow-tags")
    print("  git push github main --follow-tags")


if __name__ == "__main__":
    main()
