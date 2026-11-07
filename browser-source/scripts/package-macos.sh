#!/bin/bash
# Packages a hosted-runner build; this does not build or claim public signing.
set -euo pipefail
cd "$(dirname "$0")/.."
target="${1:?Usage: package-macos.sh <aarch64-apple-darwin|x86_64-apple-darwin>}"
case "$target" in
  aarch64-apple-darwin) arch="arm64" ;;
  x86_64-apple-darwin) arch="x64" ;;
  *) echo "Unsupported macOS target: $target" >&2; exit 2 ;;
esac
binary="target/$target/release/pip-macos"
test -f "$binary" || { echo "Build missing: $binary" >&2; exit 1; }
version="$(sed -n 's/^version = "\([0-9][0-9.]*\)"$/\1/p' crates/app/Cargo.toml | head -n 1)"
test -n "$version" || { echo "Could not read package version" >&2; exit 1; }
mkdir -p dist
stage="$(mktemp -d "$PWD/dist/macos-stage.XXXXXX")"
case "$stage" in
  "$PWD"/dist/macos-stage.*) ;;
  *) echo "Unexpected staging path; refusing cleanup setup" >&2; exit 1 ;;
esac
trap 'rm -rf -- "$stage"' EXIT
app="$stage/Pip.app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$binary" "$app/Contents/MacOS/Pip"
chmod 755 "$app/Contents/MacOS/Pip"
cat > "$app/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleName</key><string>Pip</string>
<key>CFBundleDisplayName</key><string>Pip</string>
<key>CFBundleIdentifier</key><string>com.psharehub.pip.preview</string>
<key>CFBundleExecutable</key><string>Pip</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleShortVersionString</key><string>$version</string>
<key>CFBundleVersion</key><string>$version</string>
<key>CFBundleIconFile</key><string>Pip</string>
<key>LSMinimumSystemVersion</key><string>12.0</string>
<key>NSHighResolutionCapable</key><true/>
<key>NSCameraUsageDescription</key><string>Pip chỉ dùng camera sau khi bạn chấp thuận yêu cầu của website.</string>
<key>NSMicrophoneUsageDescription</key><string>Pip chỉ dùng micrô sau khi bạn chấp thuận yêu cầu của website.</string>
<key>NSAppTransportSecurity</key><dict><key>NSAllowsArbitraryLoadsInWebContent</key><true/></dict>
</dict></plist>
EOF
iconset="$stage/Pip.iconset"
mkdir "$iconset"
for size in 16 32 128 256 512; do
  sips -z "$size" "$size" crates/app/ui/logo.png --out "$iconset/icon_${size}x${size}.png" >/dev/null
  double=$((size * 2))
  sips -z "$double" "$double" crates/app/ui/logo.png --out "$iconset/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$iconset" -o "$app/Contents/Resources/Pip.icns"
plutil -lint "$app/Contents/Info.plist"
plutil -extract NSCameraUsageDescription raw -o - "$app/Contents/Info.plist" >/dev/null
plutil -extract NSMicrophoneUsageDescription raw -o - "$app/Contents/Info.plist" >/dev/null
# Apple Silicon requires a valid code signature. Ad-hoc is not Developer ID or notarization.
codesign --force --sign - --timestamp=none "$app"
codesign --verify --deep --strict "$app"
artifact="$PWD/dist/Pip-macOS-$arch-$version-preview.zip"
ditto -c -k --sequesterRsrc --keepParent "$app" "$artifact"
(cd dist && shasum -a 256 "$(basename "$artifact")") > "$artifact.sha256"
printf 'Created %s\nAd-hoc signature only; not notarized or approved for public release.\n' "$artifact"
