"""Writes THIRD-PARTY-NOTICES.txt: every library the installed app is built from (the Rust crates
of the desktop app and cenango-mcp, and the frontend's production npm packages), with its version,
license and the license text the library ships. Run before building an installer:

    python tools/third_party_notices.py

Needs `cargo` on PATH and frontend/node_modules installed (npm ci)."""
import json
import os
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "THIRD-PARTY-NOTICES.txt"
TARGET = "x86_64-pc-windows-msvc"
APP_ROOTS = {"accounting-desktop", "mcp"}
LICENSE_NAMES = ("license", "licence", "copying", "notice", "unlicense")


def license_texts(folder: Path) -> list[tuple[str, str]]:
    out = []
    if not folder.is_dir():
        return out
    for f in sorted(folder.iterdir()):
        if f.is_file() and f.name.lower().startswith(LICENSE_NAMES):
            try:
                out.append((f.name, f.read_text(encoding="utf-8", errors="replace").strip()))
            except OSError:
                pass
    return out


def rust_packages() -> list[dict]:
    meta = json.loads(subprocess.check_output(
        ["cargo", "metadata", "--format-version", "1", "--filter-platform", TARGET], cwd=ROOT))
    pkgs = {p["id"]: p for p in meta["packages"]}
    nodes = {n["id"]: n for n in meta["resolve"]["nodes"]}
    # Normal (not dev / build-only) dependencies reachable from the shipped binaries.
    todo = [i for i, p in pkgs.items() if p["name"] in APP_ROOTS]
    seen = set()
    while todo:
        i = todo.pop()
        if i in seen:
            continue
        seen.add(i)
        for d in nodes[i]["deps"]:
            if any(k["kind"] in (None, "build") for k in d["dep_kinds"]):
                todo.append(d["pkg"])
    out = []
    for i in seen:
        p = pkgs[i]
        if p["source"] is None:  # our own crates
            continue
        out.append({
            "name": p["name"], "version": p["version"], "license": p.get("license") or "see files",
            "url": p.get("repository") or p.get("homepage") or "",
            "texts": license_texts(Path(p["manifest_path"]).parent),
        })
    return out


def npm_packages() -> list[dict]:
    front = ROOT / "frontend"
    tree = json.loads(subprocess.run(
        ["npm", "ls", "--omit=dev", "--all", "--json"], cwd=front, capture_output=True, text=True,
        shell=os.name == "nt").stdout or "{}")
    found = {}

    def walk(deps: dict) -> None:
        for name, d in (deps or {}).items():
            key = (name, d.get("version"))
            if d.get("version") and key not in found:
                folder = front / "node_modules" / name
                manifest = folder / "package.json"
                info = json.loads(manifest.read_text(encoding="utf-8")) if manifest.is_file() else {}
                lic = info.get("license")
                if isinstance(lic, dict):
                    lic = lic.get("type")
                repo = info.get("repository")
                if isinstance(repo, dict):
                    repo = repo.get("url")
                found[key] = {"name": name, "version": d["version"], "license": lic or "see files",
                              "url": repo or info.get("homepage") or "", "texts": license_texts(folder)}
            walk(d.get("dependencies"))

    walk(tree.get("dependencies"))
    return list(found.values())


def main() -> int:
    rust = sorted(rust_packages(), key=lambda p: (p["name"].lower(), p["version"]))
    npm = sorted(npm_packages(), key=lambda p: (p["name"].lower(), p["version"]))
    lines = [
        "TallyCheck — third-party software notices",
        "",
        "TallyCheck is Copyright (C) 2026 Cenango (Pvt) Ltd and is licensed under the MIT License",
        "(LICENSE-MIT) or the Apache License 2.0 (LICENSE-APACHE), at your option; see NOTICE.",
        "It is built with the libraries below, each under its own license, reproduced here as shipped.",
        "",
        f"Rust crates: {len(rust)}    npm packages: {len(npm)}",
        "",
    ]
    for title, items in (("RUST CRATES", rust), ("NPM PACKAGES (frontend)", npm)):
        lines += ["=" * 78, title, "=" * 78, ""]
        for p in items:
            lines.append(f"{p['name']} {p['version']} — {p['license']}")
            if p["url"]:
                lines.append(f"  {p['url']}")
            for fname, text in p["texts"]:
                lines += ["", f"  --- {fname} ---", "", text, ""]
            lines += ["-" * 78, ""]
    OUT.write_text("\n".join(lines) + "\n", encoding="utf-8", newline="\n")
    missing = [f"{p['name']} {p['version']}" for p in rust + npm if not p["texts"]]
    print(f"wrote {OUT.name}: {len(rust)} crates, {len(npm)} npm packages; {len(missing)} without a license file")
    for m in missing:
        print("  no license file:", m)
    return 0


if __name__ == "__main__":
    sys.exit(main())
