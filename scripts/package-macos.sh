#!/usr/bin/env bash
# Build macOS release packages into dist/: a universal (Apple Silicon + Intel) eleven.app as
#   eleven-<version>-macos-universal.dmg   (drag-to-Applications disk image)
#   eleven-<version>-macos-universal.zip
set -euo pipefail

cd "$(dirname "$0")/.."

./scripts/bundle-macos.sh --universal

version="$(cargo pkgid -p eleven | sed 's/.*[#@]//')"
base="dist/eleven-${version}-macos-universal"
rm -f "${base}.zip" "${base}.dmg"

ditto -c -k --keepParent dist/eleven.app "${base}.zip"

staging="$(mktemp -d)/eleven"
mkdir -p "$staging"
cp -R dist/eleven.app "$staging/"
ln -s /Applications "$staging/Applications"
hdiutil create -volname "eleven ${version}" -srcfolder "$staging" -ov -format UDZO "${base}.dmg" >/dev/null

lipo -info dist/eleven.app/Contents/MacOS/eleven
echo "Built ${base}.dmg and ${base}.zip"
