#!/usr/bin/env bash
# Print eleven's version. Fails if Cargo.lock is out of date with Cargo.toml (after a version
# bump, run `cargo update --workspace`), so packaging never builds with a stale lockfile.
set -euo pipefail

cd "$(dirname "$0")/.."
cargo pkgid --locked -p eleven | sed 's/.*[#@]//'
