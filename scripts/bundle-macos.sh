#!/usr/bin/env bash
# Build dist/eleven.app from a release build. macOS only (needs sips + iconutil for the icon).
#   --universal   build for Apple Silicon and Intel and merge them with lipo
set -euo pipefail

universal=false
if [[ "${1:-}" == "--universal" ]]; then
  universal=true
fi

cd "$(dirname "$0")/.."

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "bundle-macos.sh must be run on macOS" >&2
  exit 1
fi

if $universal; then
  for target in aarch64-apple-darwin x86_64-apple-darwin; do
    rustup target add "$target" >/dev/null
    cargo build -p eleven --release --locked --target "$target"
  done
  bin="target/universal/eleven"
  mkdir -p "$(dirname "$bin")"
  lipo -create -output "$bin" \
    target/aarch64-apple-darwin/release/eleven target/x86_64-apple-darwin/release/eleven
else
  cargo build -p eleven --release --locked
  bin="target/release/eleven"
fi

version="$(cargo pkgid -p eleven | sed 's/.*[#@]//')"
app="dist/eleven.app"

rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$bin" "$app/Contents/MacOS/eleven"
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

# An ad-hoc signature: Apple Silicon refuses to run unsigned code, and lipo invalidates the
# per-architecture signatures the linker made. (Distribution without Gatekeeper warnings needs
# a Developer ID signature and notarisation instead.)
codesign --force --deep --sign - "$app"

echo "Built $app (version ${version})"
