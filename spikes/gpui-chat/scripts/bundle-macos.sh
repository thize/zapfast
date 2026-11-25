#!/bin/sh
# Wraps the debug binary in a signed .app so macOS delivers notifications
# (UserNotifications needs a bundle identifier and a code signature; see
# src/platform/notify.rs). Ad-hoc signing is enough on this Mac.
#
#   cargo build && scripts/bundle-macos.sh
#   open target/debug/ZapFast-GPUI-spike.app --args --scene platform
#
# Uses only tools macOS ships (sips, iconutil, codesign); downloads nothing.
set -eu
cd "$(dirname "$0")/.."
app="target/debug/ZapFast-GPUI-spike.app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp target/debug/gpui-chat-spike "$app/Contents/MacOS/gpui-chat-spike"

# ZapFast's own macOS icon (packaging/macos/icon-1024.png) as an .icns.
icons="$(mktemp -d)/icon.iconset"
mkdir -p "$icons"
src="../../packaging/macos/icon-1024.png"
for size in 16 32 128 256 512; do
  sips -z $size $size "$src" --out "$icons/icon_${size}x${size}.png" >/dev/null
  sips -z $((size * 2)) $((size * 2)) "$src" --out "$icons/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$icons" -o "$app/Contents/Resources/icon.icns"

cat > "$app/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleIdentifier</key><string>dev.zapfast.gpui-spike</string>
  <key>CFBundleName</key><string>ZapFast GPUI spike</string>
  <key>CFBundleDisplayName</key><string>ZapFast GPUI spike</string>
  <key>CFBundleExecutable</key><string>gpui-chat-spike</string>
  <key>CFBundleIconFile</key><string>icon</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>0.0.0</string>
  <key>CFBundleVersion</key><string>0</string>
  <key>LSMinimumSystemVersion</key><string>12.0</string>
  <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST

codesign --force --deep --sign - "$app"
echo "$app"
