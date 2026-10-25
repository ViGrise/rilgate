#!/usr/bin/env bash
# Packages a release binary with its license and example configuration as
# rilgate-<platform>.tar.gz, or rilgate-<platform>.zip for Windows binaries, and
# copies the bare binary as rilgate-<platform> (.exe on Windows) beside it.
#
# Usage: scripts/release/package.sh <binary> <platform> <output-dir>
set -euo pipefail
if [[ $# -ne 3 ]]; then
  echo "usage: $0 <binary> <platform> <output-dir>" >&2
  exit 2
fi
root="$(cd "$(dirname "$0")/../.." && pwd)"
binary="$1"
platform="$2"
mkdir -p "$3"
output="$(cd "$3" && pwd)"
name="rilgate-$platform"
stage="$(mktemp -d)"
trap 'rm -rf "$stage"' EXIT

mkdir "$stage/$name"
cp "$binary" "$stage/$name/"
cp "$root/LICENSE" "$root/config/rilgate.yml" "$stage/$name/"
chmod +x "$stage/$name/$(basename "$binary")"

case "$binary" in
  *.exe)
    (cd "$stage" && 7z a -tzip -bso0 -bsp0 "$output/$name.zip" "$name")
    cp "$binary" "$output/$name.exe"
    ;;
  *)
    tar -czf "$output/$name.tar.gz" -C "$stage" "$name"
    cp "$binary" "$output/$name"
    chmod +x "$output/$name"
    ;;
esac
echo "Packaged:"
ls -1 "$output/$name"*
