"""Builds TallyCheck's MSIX package for the Microsoft Store (D47).

    python tools/build_msix.py --identity-name <Package/Identity/Name> \
        --publisher "CN=<…>" --publisher-display-name "Cenango (Pvt) Ltd"

The three identity values are on Partner Center → the app → Product management → Product
identity; they must be copied exactly (case and spaces count). The package is not signed: the
Store signs it with a Microsoft certificate after certification.

    python tools/build_msix.py --test [--version 1.0.0.0]

builds a TEST package instead: a made-up identity ("TallyCheck.StoreTest"), the display name
"TallyCheck (test)" and its own data folder %LOCALAPPDATA%\\TallyCheckStoreTest, so installing,
upgrading and uninstalling it can never touch real books. Install it with Developer Mode on:

    Add-AppxPackage -Register <out>\\layout\\AppxManifest.xml

What it does: builds the desktop app with `--features store` (no in-app updater) and
cenango-mcp in a separate target folder (target/msix-build, so the normal installer build is
untouched), lays out the package (TallyCheck.exe, cenango-mcp.exe, frontend/, license files,
tile images made from src-tauri/icons/icon-512.png, AppxManifest.xml) and packs it with the
Windows SDK's makeappx.exe. Needs: frontend/dist built (npm run build), the Windows 10/11 SDK,
Pillow for the tile images."""
import argparse
import glob
import json
import os
import re
import shutil
import subprocess
import sys
from pathlib import Path

from PIL import Image

ROOT = Path(__file__).resolve().parent.parent
TARGET = ROOT / "target" / "msix-build"
TEST = {
    "identity": "TallyCheck.StoreTest",
    "publisher": "CN=TallyCheck Test",
    "publisher_display": "TallyCheck test build",
    "display": "TallyCheck (test)",
    "data_dir": "TallyCheckStoreTest",
}
DATA_DIRS = ["TallyCheckData", "CenangoAccounting"]  # D46: current name, and the one from before the rename
TILES = {
    "Square44x44Logo.png": (44, 44),
    "Square150x150Logo.png": (150, 150),
    "Wide310x150Logo.png": (310, 150),
    "StoreLogo.png": (50, 50),
}


def find_makeappx() -> Path:
    hits = sorted(glob.glob(r"C:\Program Files (x86)\Windows Kits\10\bin\10.*\x64\makeappx.exe"))
    if not hits:
        sys.exit("makeappx.exe not found: install the Windows 10/11 SDK")
    return Path(hits[-1])


def store_version(text: str) -> str:
    parts = [int(p) for p in text.split(".")]
    parts = (parts + [0, 0, 0, 0])[:4]
    if parts[0] == 0:
        sys.exit("The Store refuses versions starting with 0 (use 1.0.0 or later)")
    if parts[3] != 0:
        sys.exit("The fourth part of a Store version is reserved and must be 0")
    if any(p > 65535 for p in parts):
        sys.exit("Each part of the version must be 0-65535")
    return ".".join(map(str, parts))


def tile(src: Image.Image, size: tuple[int, int]) -> Image.Image:
    w, h = size
    side = min(w, h)
    icon = src.resize((side, side), Image.LANCZOS)
    out = Image.new("RGBA", size, (0, 0, 0, 0))
    out.paste(icon, ((w - side) // 2, (h - side) // 2), icon)
    return out


def xml_escape(s: str) -> str:
    return s.replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;").replace('"', "&quot;")


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--identity-name")
    ap.add_argument("--publisher", help='e.g. "CN=12345678-ABCD-…" from Partner Center')
    ap.add_argument("--publisher-display-name")
    ap.add_argument("--display-name", default="TallyCheck",
                    help="must match a product name reserved in Partner Center exactly (case counts)")
    ap.add_argument("--test", action="store_true", help="test identity and test data folder")
    ap.add_argument("--version", help="defaults to tauri.conf.json's version, as x.y.z.0")
    ap.add_argument("--out", default=str(ROOT / "target" / "msix"))
    ap.add_argument("--skip-build", action="store_true")
    a = ap.parse_args()

    if a.test:
        ident, publisher, publisher_display, display = TEST["identity"], TEST["publisher"], TEST["publisher_display"], TEST["display"]
        data_dirs = [TEST["data_dir"]]
    else:
        if not (a.identity_name and a.publisher and a.publisher_display_name):
            sys.exit("Give --identity-name, --publisher and --publisher-display-name from Partner Center (or --test)")
        ident, publisher, publisher_display, display = a.identity_name, a.publisher, a.publisher_display_name, a.display_name
        data_dirs = DATA_DIRS
    conf = json.loads((ROOT / "src-tauri" / "tauri.conf.json").read_text(encoding="utf-8"))
    version = store_version(a.version or conf["version"])

    dist = ROOT / "frontend" / "dist"
    if not (dist / "index.html").is_file():
        sys.exit("frontend/dist is missing: run `npm run build` in frontend/")

    if not a.skip_build:
        subprocess.run([sys.executable, str(ROOT / "tools" / "third_party_notices.py")], check=True)
        env = dict(os.environ)
        env.pop("ACCOUNTING_APP_DIR_NAME", None)
        if a.test:
            env["ACCOUNTING_APP_DIR_NAME"] = TEST["data_dir"]
        target = ["--target-dir", str(TARGET)]
        subprocess.run(["cargo", "build", "--release", "-p", "accounting-desktop", "--features", "store", *target],
                       cwd=ROOT, env=env, check=True)
        subprocess.run(["cargo", "build", "--release", "-p", "mcp", *target], cwd=ROOT, env=env, check=True)

    out = Path(a.out)
    layout = out / ("layout-test" if a.test else "layout")
    if layout.exists():
        shutil.rmtree(layout)
    (layout / "Assets").mkdir(parents=True)
    shutil.copy2(TARGET / "release" / "accounting-desktop.exe", layout / "TallyCheck.exe")
    shutil.copy2(TARGET / "release" / "cenango-mcp.exe", layout / "cenango-mcp.exe")
    shutil.copytree(dist, layout / "frontend")
    for src, dst in [("LICENSE-MIT", "LICENSE-MIT.txt"), ("LICENSE-APACHE", "LICENSE-APACHE.txt"),
                     ("NOTICE", "NOTICE.txt"), ("THIRD-PARTY-NOTICES.txt", "THIRD-PARTY-NOTICES.txt")]:
        shutil.copy2(ROOT / src, layout / dst)
    icon = Image.open(ROOT / "src-tauri" / "icons" / "icon-512.png").convert("RGBA")
    for name, size in TILES.items():
        tile(icon, size).save(layout / "Assets" / name)

    excluded = "\n".join(
        f"        <virtualization:ExcludedDirectory>$(KnownFolder:LocalAppData)\\{d}</virtualization:ExcludedDirectory>"
        for d in data_dirs
    )
    manifest = (ROOT / "src-tauri" / "msix" / "AppxManifest.xml").read_text(encoding="utf-8")
    for key, value in {
        "IDENTITY_NAME": xml_escape(ident),
        "PUBLISHER": xml_escape(publisher),
        "PUBLISHER_DISPLAY_NAME": xml_escape(publisher_display),
        "DISPLAY_NAME": xml_escape(display),
        "VERSION": version,
        "EXCLUDED_DIRECTORIES": excluded,
    }.items():
        manifest = manifest.replace("{{" + key + "}}", value)
    left = re.findall(r"\{\{[A-Z_]+\}\}", manifest)
    if left:
        sys.exit(f"manifest template has unfilled values: {left}")
    (layout / "AppxManifest.xml").write_text(manifest, encoding="utf-8")

    package = out / f"TallyCheck_{version}_x64{'_test' if a.test else ''}.msix"
    subprocess.run([str(find_makeappx()), "pack", "/d", str(layout), "/p", str(package), "/o", "/h", "SHA256"], check=True)
    print(f"\nPackage: {package}\nLayout:  {layout}")
    if a.test:
        print(f"Install for testing (Developer Mode): Add-AppxPackage -Register \"{layout / 'AppxManifest.xml'}\"")
    else:
        print("Upload the .msix in Partner Center → the app → Submission → Packages.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
