#!/usr/bin/env bash
# Package the release binary as build/Live Slides.app (double-clickable; no terminal needed).
# Settings come from <repo>/.env (loaded at startup): LS_SOURCE, LS_FULLSCREEN, LS_DISPLAY, INDEX, OPENROUTER_API_KEY.
# The binary finds the repo (models, library, .env) via the path baked in at build time, or LS_ROOT.
set -euo pipefail
cd "$(dirname "$0")/.."
[ -x target/release/live-slides ] || { echo "build first: CARGO_BUILD_JOBS=2 cargo build --release"; exit 1; }
APP="build/Live Slides.app"
rm -rf "$APP"; mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp target/release/live-slides "$APP/Contents/MacOS/live-slides"
# Icon: render once at 1024 and derive the iconset sizes.
SET=build/icon.iconset; rm -rf "$SET"; mkdir -p "$SET"
[ -f build/icon-1024.png ] || python3 scripts/make_icon.py build/icon-1024.png 1024
for sz in 16 32 128 256 512; do
  sips -z $sz $sz build/icon-1024.png --out "$SET/icon_${sz}x${sz}.png" >/dev/null
  sips -z $((sz*2)) $((sz*2)) build/icon-1024.png --out "$SET/icon_${sz}x${sz}@2x.png" >/dev/null
done
iconutil -c icns "$SET" -o "$APP/Contents/Resources/icon.icns"
cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleName</key><string>Live Slides</string>
  <key>CFBundleDisplayName</key><string>Live Slides</string>
  <key>CFBundleIdentifier</key><string>dev.liveslides.poc</string>
  <key>CFBundleExecutable</key><string>live-slides</string>
  <key>CFBundleIconFile</key><string>icon</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleVersion</key><string>0.1.0</string>
  <key>CFBundleShortVersionString</key><string>0.1.0</string>
  <key>LSMinimumSystemVersion</key><string>13.0</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSMicrophoneUsageDescription</key><string>Live Slides listens to the presenter to choose the picture on screen. Audio is transcribed on this Mac.</string>
</dict></plist>
PLIST
echo "built $APP ($(du -sh "$APP" | cut -f1))"
