#!/usr/bin/env bash
# D58 (in progress): builds the Mac App Store version of TallyCheck, sandboxed, as a signed
# installer package to upload with Transporter:
#   target/macos-appstore/TallyCheck_<version>.pkg
#
# This is NOT the website download: that is the Developer ID .dmg from tools/build_macos.sh,
# which stays unsandboxed. See docs/macos-app-store.md before using this.
#
# Run on a Mac (Xcode Command Line Tools, rustup, Node.js, Python 3) with, in the login Keychain,
# the "Apple Distribution" and "Mac Installer Distribution" certificates, and these variables
# (never in this repository):
#   APPLE_SIGNING_IDENTITY      "Apple Distribution: <Company> (<TEAM ID>)"
#   APPLE_INSTALLER_IDENTITY    "3rd Party Mac Developer Installer: <Company> (<TEAM ID>)"
#   APPLE_TEAM_ID               the 10-character Team ID
#   APPLE_PROVISIONING_PROFILE  path to the downloaded Mac App Store profile (.provisionprofile)
set -euo pipefail
cd "$(dirname "$0")/.."

for v in APPLE_SIGNING_IDENTITY APPLE_INSTALLER_IDENTITY APPLE_TEAM_ID APPLE_PROVISIONING_PROFILE; do
  if [ -z "${!v:-}" ]; then
    echo "Set $v first (docs/macos-app-store.md)." >&2
    exit 1
  fi
done
case "$APPLE_SIGNING_IDENTITY" in
  "Apple Distribution:"*|"3rd Party Mac Developer Application:"*) ;;
  *) echo "APPLE_SIGNING_IDENTITY must be the Apple Distribution certificate, not Developer ID." >&2; exit 1 ;;
esac
# Notarization is for the .dmg only; the App Store checks the package itself.
unset APPLE_API_ISSUER APPLE_API_KEY APPLE_API_KEY_PATH APPLE_CERTIFICATE APPLE_CERTIFICATE_PASSWORD

OUT=target/macos-appstore
mkdir -p "$OUT"

echo "==> Entitlements and provisioning profile"
cp src-tauri/macos/appstore.entitlements "$OUT/appstore.entitlements"
BUNDLE_ID=$(python3 -c "import json; print(json.load(open('src-tauri/tauri.conf.json'))['identifier'])")
plutil -insert com.apple.application-identifier -string "$APPLE_TEAM_ID.$BUNDLE_ID" "$OUT/appstore.entitlements"
plutil -insert com.apple.developer.team-identifier -string "$APPLE_TEAM_ID" "$OUT/appstore.entitlements"
plutil -lint "$OUT/appstore.entitlements"
cp "$APPLE_PROVISIONING_PROFILE" "$OUT/embedded.provisionprofile"

TARGETS=(aarch64-apple-darwin x86_64-apple-darwin)
rustup target add "${TARGETS[@]}"

echo "==> Frontend"
(cd frontend && npm ci --ignore-scripts --no-audit --no-fund && npm run build)

echo "==> Third-party notices"
python3 tools/third_party_notices.py

# The Claude connector (tallycheck-mcp) is left out of this build for now: Claude Desktop, not
# TallyCheck, starts it, which a sandboxed helper may not allow (docs/macos-app-store.md, open item).

echo "==> App (sandboxed, store feature: no in-app updater)"
npm ci --ignore-scripts --no-audit --no-fund
npx tauri build --target universal-apple-darwin --bundles app --features store \
  --config src-tauri/tauri.macos-appstore.json

APP=target/universal-apple-darwin/release/bundle/macos/TallyCheck.app
codesign --display --entitlements - "$APP" | grep -q "com.apple.security.app-sandbox" \
  || { echo "The app is not sandboxed: check the entitlements." >&2; exit 1; }

echo "==> Installer package"
VERSION=$(python3 -c "import json; print(json.load(open('src-tauri/tauri.conf.json'))['version'])")
PKG="$OUT/TallyCheck_$VERSION.pkg"
xcrun productbuild --sign "$APPLE_INSTALLER_IDENTITY" --component "$APP" /Applications "$PKG"
ls -l "$PKG"
echo "Upload $PKG with Transporter (or: xcrun altool --upload-app --type macos ...)."
