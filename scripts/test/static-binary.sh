#!/usr/bin/env bash
# Runs a static Linux release binary in an empty image, which has no C library,
# dynamic loader or other files, so any runtime library dependency fails the check.
#
# Usage: scripts/test/static-binary.sh <binary>
set -euo pipefail
cd "$(dirname "$0")/../.."
if [[ $# -ne 1 ]]; then
  echo "usage: $0 <binary>" >&2
  exit 2
fi
image="rilgate-static-smoke-$$"
context="$(mktemp -d /tmp/rilgate-static.XXXXXX)"
cleanup() {
  docker image rm -f "$image" >/dev/null 2>&1 || true
  rm -rf "$context"
}
trap cleanup EXIT

cp "$1" "$context/rilgate"
cp config/rilgate.production.yml "$context/rilgate.yml"
printf 'FROM scratch\nCOPY rilgate rilgate.yml /\n' | docker build -q -t "$image" -f - "$context" >/dev/null
docker run --rm "$image" /rilgate --version
docker run --rm -e RILGATE_SECURITY_SECRET=static-smoke-secret-with-at-least-32-bytes \
  "$image" /rilgate --config /rilgate.yml config check
