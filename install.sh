#!/usr/bin/env bash
# Installs a Rilgate release binary for Linux, macOS or FreeBSD from GitHub
# Releases.
#
#   curl -fsSL https://github.com/ViGrise/rilgate/releases/latest/download/install.sh | bash
#
# VERSION selects a release tag (default: latest) and INSTALL_DIR the target
# directory (default: /usr/local/bin). On Linux amd64 and arm64, LIBC selects the
# dynamic glibc build (gnu) or the static build (musl); by default the glibc build
# is used when the system has glibc 2.36 or newer. Linux ppc64le and riscv64 have
# only the static build. Archives are verified against SHA256SUMS.
set -euo pipefail

REPO="ViGrise/rilgate"
BIN_NAME="rilgate"
INSTALL_DIR="${INSTALL_DIR:-/usr/local/bin}"
VERSION="${VERSION:-latest}"

fail() {
  echo "$*" >&2
  exit 1
}

os="$(uname -s | tr '[:upper:]' '[:lower:]')"
case "$os" in
  linux | darwin | freebsd) ;;
  *) fail "Unsupported OS: $os (on Windows, use install.ps1 from GitHub Releases)" ;;
esac

case "$(uname -m)" in
  x86_64 | amd64) arch="amd64" ;;
  aarch64 | arm64) arch="arm64" ;;
  ppc64le) arch="ppc64le" ;;
  riscv64) arch="riscv64" ;;
  *) fail "Unsupported architecture: $(uname -m)" ;;
esac

if [ "$VERSION" = "latest" ]; then
  base="https://github.com/${REPO}/releases/latest/download"
else
  base="https://github.com/${REPO}/releases/download/${VERSION}"
fi
case "$os-$arch" in
  linux-* | darwin-amd64 | darwin-arm64 | freebsd-amd64) ;;
  *) fail "No release build for $os on $arch" ;;
esac
variant=""
if [ "$os" = "linux" ] && { [ "$arch" = "amd64" ] || [ "$arch" = "arm64" ]; }; then
  libc="${LIBC:-}"
  if [ -z "$libc" ]; then
    glibc="$(getconf GNU_LIBC_VERSION 2>/dev/null | awk '{print $2}' || true)"
    if [ -n "$glibc" ] && awk -v version="$glibc" 'BEGIN { split(version, part, "."); exit !(part[1] > 2 || (part[1] == 2 && part[2] >= 36)) }'; then
      libc="gnu"
    else
      libc="musl"
    fi
  fi
  case "$libc" in
    gnu) ;;
    musl) variant="-static" ;;
    *) fail "Unsupported LIBC: $libc (use gnu or musl)" ;;
  esac
elif [ "$os" = "linux" ]; then
  [ "${LIBC:-musl}" = "musl" ] || fail "Linux ${arch} has only the static build; unset LIBC"
  variant="-static"
fi
archive="${BIN_NAME}-${os}-${arch}${variant}.tar.gz"

download() {
  if command -v curl >/dev/null 2>&1; then
    curl -fsSL "$1" -o "$2"
  elif command -v wget >/dev/null 2>&1; then
    wget -qO "$2" "$1"
  else
    fail "curl or wget is required"
  fi
}

workdir="$(mktemp -d)"
trap 'rm -rf "$workdir"' EXIT

echo "Downloading ${archive} (${VERSION})..."
download "${base}/${archive}" "${workdir}/${archive}"
download "${base}/SHA256SUMS" "${workdir}/SHA256SUMS"

expected="$(awk -v file="$archive" '$2 == file {print $1}' "${workdir}/SHA256SUMS")"
[ -n "$expected" ] || fail "SHA256SUMS has no entry for ${archive}"
if command -v sha256sum >/dev/null 2>&1; then
  actual="$(sha256sum "${workdir}/${archive}" | awk '{print $1}')"
elif command -v shasum >/dev/null 2>&1; then
  actual="$(shasum -a 256 "${workdir}/${archive}" | awk '{print $1}')"
elif command -v sha256 >/dev/null 2>&1; then
  actual="$(sha256 -q "${workdir}/${archive}")"
else
  actual="$(openssl dgst -sha256 -r "${workdir}/${archive}" | awk '{print $1}')"
fi
[ "$expected" = "$actual" ] || fail "Checksum mismatch for ${archive}"

tar -xzf "${workdir}/${archive}" -C "$workdir"
source="${workdir}/${BIN_NAME}-${os}-${arch}${variant}/${BIN_NAME}"

mkdir -p "$INSTALL_DIR" 2>/dev/null || true
if [ -w "$INSTALL_DIR" ]; then
  install -m 0755 "$source" "${INSTALL_DIR}/${BIN_NAME}"
elif command -v sudo >/dev/null 2>&1; then
  sudo install -m 0755 "$source" "${INSTALL_DIR}/${BIN_NAME}"
else
  fail "${INSTALL_DIR} is not writable; set INSTALL_DIR to a writable directory"
fi

echo "Installed ${INSTALL_DIR}/${BIN_NAME}"
if ! "${INSTALL_DIR}/${BIN_NAME}" --version; then
  if [ "$os" = "linux" ] && [ -z "$variant" ] && { [ "$arch" = "amd64" ] || [ "$arch" = "arm64" ]; }; then
    fail "${BIN_NAME} is installed but cannot start; the glibc build needs glibc 2.36 or newer. Set LIBC=musl to install the static build."
  fi
  fail "${BIN_NAME} is installed but cannot start."
fi
echo "Upgrade later with: ${BIN_NAME} upgrade"
