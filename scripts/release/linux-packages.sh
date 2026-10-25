#!/usr/bin/env bash
# Builds the Debian package, the RPM package and the systemd-sysext image for one
# Linux release binary. Needs nfpm and mksquashfs on PATH.
#
# Usage: scripts/release/linux-packages.sh <binary> <arch> <version> <output-dir>
#   arch: amd64, arm64, ppc64le or riscv64
set -euo pipefail
if [[ $# -ne 4 ]]; then
  echo "usage: $0 <binary> <arch> <version> <output-dir>" >&2
  exit 2
fi
root="$(cd "$(dirname "$0")/../.." && pwd)"
binary="$(cd "$(dirname "$1")" && pwd)/$(basename "$1")"
arch="$2"
version="${3#v}"
mkdir -p "$4"
output="$(cd "$4" && pwd)"

case "$arch" in
  amd64) deb_arch=amd64 rpm_arch=x86_64 sysext_arch=x86-64 ;;
  arm64) deb_arch=arm64 rpm_arch=aarch64 sysext_arch=arm64 ;;
  ppc64le) deb_arch=ppc64el rpm_arch=ppc64le sysext_arch=ppc64-le ;;
  riscv64) deb_arch=riscv64 rpm_arch=riscv64 sysext_arch=riscv64 ;;
  *)
    echo "unsupported architecture: $arch" >&2
    exit 2
    ;;
esac

stage="$(mktemp -d)"
trap 'rm -rf "$stage"' EXIT
cd "$root"

mkdir "$stage/package"
cp "$binary" "$stage/package/rilgate"
cp deploy/packages/nfpm.yaml deploy/packages/rilgate.service deploy/packages/postinstall.sh \
  deploy/packages/preremove.sh deploy/packages/postremove.sh config/rilgate.yml README.md LICENSE "$stage/package/"
cp .env.example "$stage/package/env.example"
export PACKAGE_ARCH="$arch" PACKAGE_VERSION="$version"
(
  cd "$stage/package"
  nfpm package --config nfpm.yaml --packager deb --target "$output/rilgate_${version}_${deb_arch}.deb"
  nfpm package --config nfpm.yaml --packager rpm --target "$output/rilgate-${version}-1.${rpm_arch}.rpm"
)

# A system extension holds only /usr. The release name in the image must match
# the installed image name, so the image is installed as rilgate.raw.
sysext="$stage/sysext"
install -D -m 0755 "$binary" "$sysext/usr/bin/rilgate"
install -D -m 0644 deploy/packages/rilgate.service "$sysext/usr/lib/systemd/system/rilgate.service"
install -D -m 0644 config/rilgate.yml "$sysext/usr/share/rilgate/rilgate.yml"
install -D -m 0644 .env.example "$sysext/usr/share/doc/rilgate/env.example"
install -D -m 0644 LICENSE "$sysext/usr/share/doc/rilgate/LICENSE"
mkdir -p "$sysext/usr/lib/extension-release.d"
printf 'ID=_any\nARCHITECTURE=%s\n' "$sysext_arch" \
  > "$sysext/usr/lib/extension-release.d/extension-release.rilgate"
mksquashfs "$sysext" "$output/rilgate-sysext-linux-${arch}.raw" \
  -all-root -noappend -quiet -comp zstd

echo "Packaged:"
ls -1 "$output"/rilgate_"${version}"_"${deb_arch}".deb \
  "$output"/rilgate-"${version}"-1."${rpm_arch}".rpm \
  "$output"/rilgate-sysext-linux-"${arch}".raw
