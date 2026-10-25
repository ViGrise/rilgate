#!/usr/bin/env bash
# Installs the pinned nfpm release for the Linux host architecture after a
# checksum check.
#
# Usage: scripts/release/install-nfpm.sh <install-dir>
set -euo pipefail
if [[ $# -ne 1 ]]; then
  echo "usage: $0 <install-dir>" >&2
  exit 2
fi
version=2.47.0
case "$(uname -m)" in
  x86_64) archive="nfpm_${version}_Linux_x86_64.tar.gz"
    sha256=0660ca602b2d2d2ae4781a06c692b3eeb9d437ffea05b831d76e41f4a3188783 ;;
  aarch64) archive="nfpm_${version}_Linux_arm64.tar.gz"
    sha256=1c0f5f2999b9a974bfb04fdb0cc3306096de530ac5dbb25d739cc5f5219c919c ;;
  *)
    echo "unsupported host architecture: $(uname -m)" >&2
    exit 2
    ;;
esac
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
curl -fsSL -o "$work/$archive" \
  "https://github.com/goreleaser/nfpm/releases/download/v$version/$archive"
echo "$sha256  $work/$archive" | sha256sum -c -
tar -xzf "$work/$archive" -C "$work" nfpm
mkdir -p "$1"
install -m 0755 "$work/nfpm" "$1/nfpm"
