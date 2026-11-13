#!/usr/bin/env bash
# Build Linux release packages into dist/:
#   eleven-<version>-linux-<arch>.tar.gz   (binary, desktop entry, icon, README)
#   eleven_<version>_<debarch>.deb         (installs to /usr; needs dpkg-deb)
# The binary links against the build machine's glibc, so build on the oldest distro you want
# to support. The .deb's dependencies are derived from the binary itself.
set -euo pipefail

cd "$(dirname "$0")/.."

if [[ "$(uname -s)" != "Linux" ]]; then
  echo "package-linux.sh must be run on Linux" >&2
  exit 1
fi

version="$(./scripts/version.sh)"
cargo build -p eleven --profile dist --locked

arch="$(uname -m)"
bin="target/dist/eleven"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
mkdir -p dist

# --- tarball ---------------------------------------------------------------
name="eleven-${version}-linux-${arch}"
stage="$tmp/$name"
mkdir -p "$stage"
install -m755 "$bin" "$stage/eleven"
install -m644 packaging/linux/eleven.desktop "$stage/eleven.desktop"
install -m644 packaging/icon-256.png "$stage/eleven.png"
install -m644 README.md "$stage/README.md"
# Root-owned entries: extracting as root must not hand the files to whichever local account
# shares the build machine's uid.
tar --owner=0 --group=0 --numeric-owner -C "$tmp" -czf "dist/${name}.tar.gz" "$name"
echo "Built dist/${name}.tar.gz"

# --- .deb ------------------------------------------------------------------
if ! command -v dpkg-deb >/dev/null; then
  echo "dpkg-deb not found; skipping the .deb" >&2
  exit 0
fi
debarch="$(dpkg --print-architecture)"
root="$tmp/deb"
install -Dm755 "$bin" "$root/usr/bin/eleven"
install -Dm644 packaging/linux/eleven.desktop "$root/usr/share/applications/eleven.desktop"
install -Dm644 packaging/icon-256.png "$root/usr/share/icons/hicolor/256x256/apps/eleven.png"
install -Dm644 README.md "$root/usr/share/doc/eleven/README.md"

# Libraries the binary links against (libc, libasound, libgcc), with the versions this build
# actually needs; dpkg-shlibdeps reads them from the binary.
linked="$(
  cd "$tmp" && mkdir -p debian &&
    printf 'Source: eleven\n\nPackage: eleven\nArchitecture: any\n' > debian/control &&
    dpkg-shlibdeps -O "$OLDPWD/$bin" 2>/dev/null | sed -n 's/^shlibs:Depends=//p'
)"
if [[ -z "$linked" ]]; then
  echo "dpkg-shlibdeps produced no dependencies (is dpkg-dev installed?)" >&2
  exit 1
fi
# Libraries loaded at runtime with dlopen, which dpkg-shlibdeps cannot see: OpenGL/EGL, X11 and
# Wayland (winit), xkbcommon, and D-Bus (file dialogs via the desktop portal).
loaded="libegl1, libgl1, libx11-6, libx11-xcb1, libxcursor1, libxi6, libxkbcommon0, libxkbcommon-x11-0, libwayland-client0, libwayland-egl1, libdbus-1-3"

installed_kb="$(du -sk "$root/usr" | cut -f1)"
mkdir -p "$root/DEBIAN"
cat > "$root/DEBIAN/control" <<EOF
Package: eleven
Version: ${version}
Architecture: ${debarch}
Maintainer: eleven maintainers <noreply@github.com>
Installed-Size: ${installed_kb}
Depends: ${linked}, ${loaded}
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
echo "Built ${deb} (Depends: ${linked}, ...)"
