#!/usr/bin/env bash
# Packages the committed source tree with all Cargo dependencies vendored as
# rilgate-src-vendor-<version>.tar.gz, so the release builds without network
# access: scripts/build.sh --release --offline
#
# Usage: scripts/release/source.sh <version> <output-dir>
set -euo pipefail
if [[ $# -ne 2 ]]; then
  echo "usage: $0 <version> <output-dir>" >&2
  exit 2
fi
root="$(cd "$(dirname "$0")/../.." && pwd)"
version="${1#v}"
mkdir -p "$2"
output="$(cd "$2" && pwd)"
name="rilgate-src-vendor-$version"
stage="$(mktemp -d)"
trap 'rm -rf "$stage"' EXIT

git -C "$root" archive --format=tar --prefix="$name/" HEAD | tar -x -C "$stage"
(
  cd "$stage/$name"
  cargo vendor --locked --versioned-dirs vendor >> .cargo/config.toml
  echo "$version" > VERSION
  cargo metadata --offline --locked --format-version 1 > /dev/null
)
tar -czf "$output/$name.tar.gz" -C "$stage" "$name"
echo "Packaged:"
ls -1 "$output/$name.tar.gz"
