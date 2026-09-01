#!/bin/bash
# Builds RunZoo.app. LSUIElement keeps it out of the Dock — menu bar only.
set -euo pipefail
cd "$(dirname "$0")/.."
source "$HOME/.cargo/env" 2>/dev/null || true

APP="dist/RunZoo.app"
UNIVERSAL=0
[[ "${1:-}" == "--universal" ]] && UNIVERSAL=1

if ! command -v cargo > /dev/null; then
  echo "cargo not found. Run ./tools/install.sh - it installs Rust for you." >&2
  exit 1
fi

echo "> generating sprites and icon"
python3 tools/gen_sprites.py > /dev/null
python3 tools/gen_icon.py > /dev/null
iconutil -c icns assets/AppIcon.iconset -o assets/AppIcon.icns

echo "> release build"
if [[ $UNIVERSAL == 1 ]]; then
  # One binary that runs on both Apple silicon and Intel.
  rustup target add aarch64-apple-darwin x86_64-apple-darwin > /dev/null
  cargo build --release --target aarch64-apple-darwin
  cargo build --release --target x86_64-apple-darwin
  mkdir -p target/universal
  lipo -create -output target/universal/runzoo \
    target/aarch64-apple-darwin/release/runzoo \
    target/x86_64-apple-darwin/release/runzoo
  BIN=target/universal/runzoo
else
  cargo build --release
  BIN=target/release/runzoo
fi

echo "> assembling bundle"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$BIN" "$APP/Contents/MacOS/RunZoo"
cp assets/AppIcon.icns "$APP/Contents/Resources/"

cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key><string>RunZoo</string>
    <key>CFBundleDisplayName</key><string>RunZoo</string>
    <key>CFBundleIdentifier</key><string>dev.runzoo.RunZoo</string>
    <key>CFBundleExecutable</key><string>RunZoo</string>
    <key>CFBundleIconFile</key><string>AppIcon</string>
    <key>CFBundlePackageType</key><string>APPL</string>
    <key>CFBundleShortVersionString</key><string>$(grep '^version' Cargo.toml | head -1 | cut -d'"' -f2)</string>
    <key>CFBundleVersion</key><string>$(grep '^version' Cargo.toml | head -1 | cut -d'"' -f2)</string>
    <key>CFBundleDevelopmentRegion</key><string>en</string>
    <key>LSMinimumSystemVersion</key><string>11.0</string>
    <!-- The key that makes it live in the menu bar with no Dock icon -->
    <key>LSUIElement</key><true/>
    <key>NSHumanReadableCopyright</key><string>Started from RunCat365 (Takuto Nakamura, Apache-2.0)</string>
</dict>
</plist>
PLIST

# Without a signature Gatekeeper blocks it. An ad-hoc signature is enough locally.
codesign --force --deep --sign - "$APP"
echo "> done: $APP  ($(du -sh "$APP" | cut -f1))"
