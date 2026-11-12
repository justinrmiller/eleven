#!/usr/bin/env bash
# Build dist/eleven.app from a release build. macOS only (needs sips + iconutil for the icon).
set -euo pipefail

cd "$(dirname "$0")/.."

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "bundle-macos.sh must be run on macOS" >&2
  exit 1
fi

cargo build -p eleven --release

version="$(cargo pkgid -p eleven | sed 's/.*[#@]//')"
app="dist/eleven.app"

rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp target/release/eleven "$app/Contents/MacOS/eleven"
sed "s/@VERSION@/${version}/g" packaging/macos/Info.plist.in > "$app/Contents/Info.plist"

# Convert the 1024px PNG icon into an .icns file if it is present.
if [[ -f packaging/icon.png ]]; then
  iconset="$(mktemp -d)/eleven.iconset"
  mkdir -p "$iconset"
  for size in 16 32 128 256 512; do
    sips -z "$size" "$size" packaging/icon.png --out "$iconset/icon_${size}x${size}.png" >/dev/null
    double=$((size * 2))
    sips -z "$double" "$double" packaging/icon.png --out "$iconset/icon_${size}x${size}@2x.png" >/dev/null
  done
  iconutil -c icns "$iconset" -o "$app/Contents/Resources/eleven.icns"
fi

echo "Built $app (version ${version})"
