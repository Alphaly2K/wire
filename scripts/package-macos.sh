#!/bin/sh
# Build a local unsigned AppKit application bundle. Guest assets stay separate.
set -eu
cd "$(dirname "$0")/.."
cargo build --release
bundle="wire-out/Wire.app"
mkdir -p "$bundle/Contents/MacOS"
cp target/release/wire "$bundle/Contents/MacOS/wire"
cat > "$bundle/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleExecutable</key><string>wire</string>
<key>CFBundleIdentifier</key><string>org.wire.runtime</string>
<key>CFBundleName</key><string>Wire</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleVersion</key><string>1</string>
<key>NSHighResolutionCapable</key><true/>
</dict></plist>
PLIST
printf '%s\n' "$bundle/Contents/MacOS/wire"
