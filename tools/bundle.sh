#!/bin/bash
# RunZoo.app 을 만든다. LSUIElement 로 Dock 에 뜨지 않고 메뉴바에만 산다.
set -euo pipefail
cd "$(dirname "$0")/.."
source "$HOME/.cargo/env" 2>/dev/null || true

APP="dist/RunZoo.app"
echo "▸ 스프라이트·아이콘 생성"
python3 tools/gen_sprites.py > /dev/null
python3 tools/gen_icon.py > /dev/null
iconutil -c icns assets/AppIcon.iconset -o assets/AppIcon.icns

echo "▸ 릴리스 빌드"
cargo build --release

echo "▸ 번들 구성"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp target/release/runzoo "$APP/Contents/MacOS/RunZoo"
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
    <key>LSMinimumSystemVersion</key><string>11.0</string>
    <!-- Dock 아이콘 없이 메뉴바 전용으로 살게 하는 열쇠 -->
    <key>LSUIElement</key><true/>
    <key>NSHumanReadableCopyright</key><string>RunCat365 (Takuto Nakamura, Apache-2.0) 에서 출발</string>
</dict>
</plist>
PLIST

# 서명이 없으면 Gatekeeper 가 막는다. 로컬용 임시 서명(ad-hoc)이면 충분하다.
codesign --force --deep --sign - "$APP"
echo "▸ 완료: $APP  ($(du -sh "$APP" | cut -f1))"
