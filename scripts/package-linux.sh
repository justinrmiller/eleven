#!/usr/bin/env bash
# Build Linux release packages into dist/:
#   eleven-<version>-linux-<arch>.tar.gz   (binary, desktop entry, icon, README)
#   eleven_<version>_<debarch>.deb         (installs to /usr; needs dpkg-deb)
# Build on the oldest distro you want to support: the binary links against its glibc.
set -euo pipefail

cd "$(dirname "$0")/.."

if [[ "$(uname -s)" != "Linux" ]]; then
  echo "package-linux.sh must be run on Linux" >&2
  exit 1
fi

cargo build -p eleven --release --locked

version="$(cargo pkgid -p eleven | sed 's/.*[#@]//')"
arch="$(uname -m)"
bin="target/release/eleven"
mkdir -p dist

# --- tarball ---------------------------------------------------------------
name="eleven-${version}-linux-${arch}"
stage="$(mktemp -d)/${name}"
mkdir -p "$stage"
install -m755 "$bin" "$stage/eleven"
install -m644 packaging/linux/eleven.desktop "$stage/eleven.desktop"
install -m644 packaging/icon-256.png "$stage/eleven.png"
install -m644 README.md "$stage/README.md"
tar -C "$(dirname "$stage")" -czf "dist/${name}.tar.gz" "$name"
echo "Built dist/${name}.tar.gz"

# --- .deb ------------------------------------------------------------------
if ! command -v dpkg-deb >/dev/null; then
  echo "dpkg-deb not found; skipping the .deb" >&2
  exit 0
fi
debarch="$(dpkg --print-architecture)"
root="$(mktemp -d)/deb"
install -Dm755 "$bin" "$root/usr/bin/eleven"
install -Dm644 packaging/linux/eleven.desktop "$root/usr/share/applications/eleven.desktop"
install -Dm644 packaging/icon-256.png "$root/usr/share/icons/hicolor/256x256/apps/eleven.png"
install -Dm644 README.md "$root/usr/share/doc/eleven/README.md"
installed_kb="$(du -sk "$root/usr" | cut -f1)"
mkdir -p "$root/DEBIAN"
cat > "$root/DEBIAN/control" <<EOF
Package: eleven
Version: ${version}
Architecture: ${debarch}
Maintainer: eleven maintainers <noreply@github.com>
Installed-Size: ${installed_kb}
Depends: libc6 (>= 2.35), libasound2 | libasound2t64, libgl1, libegl1, libxkbcommon0, libxkbcommon-x11-0
Recommends: xdg-desktop-portal
Section: sound
Priority: optional
Homepage: https://github.com/justinrmiller/eleven
Description: XMMS-style audio player written in Rust and egui
 eleven plays MP3, FLAC, Ogg Vorbis, WAV/AIFF and AAC/ALAC files and
 Icecast/Shoutcast streams, with a 10-band equalizer, playlist editor,
 spectrum analyzer and classic Winamp/XMMS .wsz skins. It goes to eleven.
EOF
deb="dist/eleven_${version}_${debarch}.deb"
dpkg-deb --root-owner-group --build "$root" "$deb" >/dev/null
echo "Built ${deb}"
