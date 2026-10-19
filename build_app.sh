#!/bin/zsh
set -eu
cd "$(dirname "$0")"
export DEVELOPER_DIR="${DEVELOPER_DIR:-/Library/Developer/CommandLineTools}"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-/private/tmp/renamer-rust-target}"
cargo build --release --locked
stage=$(mktemp -d /private/tmp/renamer-package.XXXXXX)
trap 'rm -rf "$stage"' EXIT
bundle="$stage/Renamer.app"
mkdir -p "$bundle/Contents/MacOS" "$bundle/Contents/Resources" dist
cp "$CARGO_TARGET_DIR/release/renamer" "$bundle/Contents/MacOS/Renamer"
chmod 755 "$bundle/Contents/MacOS/Renamer"
cp assets/Renamer.icns "$bundle/Contents/Resources/RenamerRounded-v3.icns"
cat > "$bundle/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleName</key><string>Renamer</string>
<key>CFBundleDisplayName</key><string>Renamer</string>
<key>CFBundleIdentifier</key><string>local.renamer.desktop</string>
<key>CFBundleExecutable</key><string>Renamer</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleShortVersionString</key><string>2.0.0</string>
<key>CFBundleVersion</key><string>201</string>
<key>CFBundleIconFile</key><string>RenamerRounded-v3.icns</string>
<key>LSMinimumSystemVersion</key><string>12.0</string>
<key>NSHighResolutionCapable</key><true/>
</dict></plist>
PLIST
codesign --force --sign - "$bundle"
codesign --verify --deep --strict "$bundle"
"$bundle/Contents/MacOS/Renamer" --cli catalog > "$stage/catalog.json"
# Keep the previous deliverable available for rollback, outside the active bundle.
if [[ -d dist/Renamer.app ]]; then
  if [[ ! -f dist/Renamer-before-Rust.zip ]]; then
    ditto -c -k --sequesterRsrc --keepParent dist/Renamer.app dist/Renamer-before-Rust.zip
  fi
  mv dist/Renamer.app "$stage/previous.app"
fi
ditto "$bundle" dist/Renamer.app
ditto -c -k --sequesterRsrc --keepParent "$bundle" "$stage/Renamer.zip"
mv -f "$stage/Renamer.zip" dist/Renamer.zip
xattr -d com.apple.FinderInfo dist/Renamer.app 2>/dev/null || true
codesign --verify --deep --strict dist/Renamer.app
print "Built: $PWD/dist/Renamer.app"
du -sh dist/Renamer.app dist/Renamer.zip
