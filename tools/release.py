"""Builds a TallyCheck release that installed copies can update to (D45).

    python tools/release.py --notes "What's new in this version" \
        --download-base https://example.com/releases/v{version}

1. Runs `npm run installer` (third-party notices, cenango-mcp, the NSIS installer) WITHOUT the
   update key: npm, cargo and the build scripts never see it (`createUpdaterArtifacts` is turned
   off for this build only).
2. Signs the finished installer with `tauri signer sign` (writes the .sig) using the private
   update key. TAURI_SIGNING_PRIVATE_KEY (the key file's contents) and
   TAURI_SIGNING_PRIVATE_KEY_PASSWORD are used if set; otherwise
   %USERPROFILE%\\.tallycheck-signing\\updater.key and its password file. The key is never copied
   into the project and is in the environment of that one command only.
3. Writes latest.json next to the installer: the version, the notes, the date, the download
   address of the installer and its signature.

With --skip-build, steps 1 and 2 are skipped and the existing installer + .sig are used (the
GitHub workflow signs the code-signed installer itself, then runs this with --skip-build).

Upload the installer and latest.json to the release page; the app's update feed (UPDATE_FEED in
src-tauri/src/updates.rs) must point at that latest.json. Bump the version first (Cargo.toml
[workspace.package] and src-tauri/tauri.conf.json), or installed copies see nothing new."""
import argparse
import hashlib
import datetime
import json
import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
BUNDLE = ROOT / "target" / "release" / "bundle" / "nsis"
KEY_DIR = Path(os.path.expanduser("~")) / ".tallycheck-signing"


# Environment variables that hold the update key; removed from the build's environment.
KEY_VARS = ("TAURI_SIGNING_PRIVATE_KEY", "TAURI_SIGNING_PRIVATE_KEY_PATH", "TAURI_SIGNING_PRIVATE_KEY_PASSWORD")


def build_env() -> dict:
    """This process's environment without the update key, for npm / cargo / tauri build."""
    return {k: v for k, v in os.environ.items() if k.upper() not in KEY_VARS}


def signing_env() -> dict:
    env = dict(os.environ)
    if not env.get("TAURI_SIGNING_PRIVATE_KEY"):
        key = KEY_DIR / "updater.key"
        if not key.is_file():
            sys.exit(f"No update signing key: set TAURI_SIGNING_PRIVATE_KEY or put it at {key}")
        env["TAURI_SIGNING_PRIVATE_KEY"] = key.read_text(encoding="utf-8").strip()
    if "TAURI_SIGNING_PRIVATE_KEY_PASSWORD" not in env:
        pw = KEY_DIR / "updater.key.password.txt"
        if not pw.is_file():
            sys.exit("No key password: set TAURI_SIGNING_PRIVATE_KEY_PASSWORD")
        env["TAURI_SIGNING_PRIVATE_KEY_PASSWORD"] = pw.read_text(encoding="utf-8").splitlines()[0].strip()
    return env


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--notes", required=True, help="what's new, shown in the app's update window")
    ap.add_argument("--download-base", required=True,
                    help="https address the installer will be downloaded from; {version} is replaced")
    ap.add_argument("--skip-build", action="store_true", help="only write latest.json for the last build")
    a = ap.parse_args()

    version = json.loads((ROOT / "src-tauri" / "tauri.conf.json").read_text(encoding="utf-8"))["version"]
    base = a.download_base.replace("{version}", version).rstrip("/")
    if not base.startswith("https://"):
        sys.exit("--download-base must be an https:// address")
    setup = BUNDLE / f"TallyCheck_{version}_x64-setup.exe"
    sig = setup.with_name(setup.name + ".sig")
    if not a.skip_build:
        # Read the key first, so a missing key stops the release before a long build.
        sign_env = signing_env()
        with tempfile.TemporaryDirectory() as tmp:
            override = Path(tmp) / "no-updater-artifacts.json"
            override.write_text('{"bundle":{"createUpdaterArtifacts":false}}', encoding="utf-8")
            subprocess.run(f'npm run installer -- --config "{override}"', cwd=ROOT, env=build_env(),
                           shell=True, check=True)
        if not setup.is_file():
            sys.exit(f"missing {setup}")
        sig.unlink(missing_ok=True)
        subprocess.run(["npx", "--no-install", "tauri", "signer", "sign", str(setup)], cwd=ROOT,
                       env=sign_env, shell=(os.name == "nt"), check=True)
    for f in (setup, sig):
        if not f.is_file():
            sys.exit(f"missing {f}")
    manifest = {
        "version": version,
        "notes": a.notes,
        "pub_date": datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
        "platforms": {
            "windows-x86_64": {"signature": sig.read_text(encoding="utf-8").strip(), "url": f"{base}/{setup.name}"},
        },
    }
    out = BUNDLE / "latest.json"
    out.write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
    # The website's download button links to .../releases/latest/download/TallyCheck-setup.exe, which
    # always gives the newest release. Same bytes as the versioned file, so the same SmartScreen
    # reputation and the same SHA-256.
    stable = BUNDLE / "TallyCheck-setup.exe"
    shutil.copy2(setup, stable)
    digest = hashlib.sha256(setup.read_bytes()).hexdigest()
    print(f"Upload to the release page:\n  {setup}\n  {stable}\n  {out}\nSHA-256: {digest}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
