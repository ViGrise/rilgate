#!/usr/bin/env bash
# Builds rilgate-windows-<arch>.msi from a Windows release binary with WiX. Runs
# in Git Bash on Windows with the wix .NET tool on PATH.
#
# Usage: scripts/release/windows-msi.sh <binary> <arch> <version> <output-dir>
#   arch: amd64 or arm64
set -euo pipefail
if [[ $# -ne 4 ]]; then
  echo "usage: $0 <binary> <arch> <version> <output-dir>" >&2
  exit 2
fi
root="$(cd "$(dirname "$0")/../.." && pwd)"
case "$2" in
  amd64) wix_arch=x64 ;;
  arm64) wix_arch=arm64 ;;
  *)
    echo "unsupported architecture: $2" >&2
    exit 2
    ;;
esac
# Windows Installer versions are numeric, so a prerelease suffix is dropped.
version="${3#v}"
version="${version%%-*}"
mkdir -p "$4"
windows_path() { cygpath -w "$(cd "$(dirname "$1")" && pwd)/$(basename "$1")"; }
msi="$(cd "$4" && pwd)/rilgate-windows-$2.msi"

wix build -arch "$wix_arch" \
  -d "Version=$version" \
  -d "BinaryPath=$(windows_path "$1")" \
  -d "LicensePath=$(windows_path "$root/LICENSE")" \
  -d "ConfigPath=$(windows_path "$root/config/rilgate.yml")" \
  -o "$(cygpath -w "$msi")" \
  "$(windows_path "$root/deploy/packages/rilgate.wxs")"
# WiX also writes debug symbols beside the installer, which are not a release file.
rm -f "${msi%.msi}.wixpdb"
echo "Packaged:"
ls -1 "$msi"
