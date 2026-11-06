#!/usr/bin/env bash
# D53: builds TallyCheck for macOS (one universal app for Apple silicon and Intel Macs):
#   target/universal-apple-darwin/release/bundle/dmg/TallyCheck_<version>_universal.dmg
#
# Run on a Mac with Xcode Command Line Tools, rustup, Node.js and Python 3:
#   bash tools/build_macos.sh
#
# Signing and notarization are switched on by environment variables that Tauri reads itself;
# without them the app is built unsigned (fine for testing on your own Mac):
#   APPLE_SIGNING_IDENTITY   "Developer ID Application: <Company> (<TEAM ID>)"
#   APPLE_CERTIFICATE / APPLE_CERTIFICATE_PASSWORD   base64 .p12 + its password (CI only)
#   APPLE_API_ISSUER / APPLE_API_KEY / APPLE_API_KEY_PATH   App Store Connect API key (notarization)
# Never put any of them in this repository (docs/macos.md).
set -euo pipefail
cd "$(dirname "$0")/.."

# An empty variable (a CI secret that is not set) must not switch signing on.
for v in APPLE_CERTIFICATE APPLE_CERTIFICATE_PASSWORD APPLE_SIGNING_IDENTITY APPLE_API_ISSUER APPLE_API_KEY APPLE_API_KEY_PATH; do
  if [ -z "${!v:-}" ]; then unset "$v"; fi
done
if [ -n "${APPLE_SIGNING_IDENTITY:-}" ]; then
  echo "Signing as the Developer ID in APPLE_SIGNING_IDENTITY"
else
  # Ad-hoc signature ("-"): no Developer ID, but Apple-silicon Macs need every app signed somehow.
  # Gatekeeper still asks once (System Settings → Privacy & Security → Open Anyway).
  echo "Test build: ad-hoc signed, not notarized"
  export APPLE_SIGNING_IDENTITY="-"
fi

TARGETS=(aarch64-apple-darwin x86_64-apple-darwin)
rustup target add "${TARGETS[@]}"

echo "==> Frontend"
(cd frontend && npm ci --ignore-scripts --no-audit --no-fund && npm run build)

# After the frontend packages are installed, so their licenses are listed too.
echo "==> Third-party notices"
python3 tools/third_party_notices.py

echo "==> Claude connector (tallycheck-mcp), both processors"
for t in "${TARGETS[@]}"; do
  cargo build --release --locked -p mcp --bin tallycheck-mcp --target "$t"
done
mkdir -p target/sidecar
# Tauri looks for <name>-<target triple>: a universal build compiles the app once per processor
# (each needs its own copy) and then bundles the universal one.
for t in "${TARGETS[@]}"; do
  cp "target/$t/release/tallycheck-mcp" "target/sidecar/tallycheck-mcp-$t"
done
lipo -create -output target/sidecar/tallycheck-mcp-universal-apple-darwin \
  target/aarch64-apple-darwin/release/tallycheck-mcp target/x86_64-apple-darwin/release/tallycheck-mcp

echo "==> App and disk image"
npm ci --ignore-scripts --no-audit --no-fund
npx tauri build --target universal-apple-darwin --config src-tauri/tauri.macos.json

ls -l target/universal-apple-darwin/release/bundle/dmg/
