#!/usr/bin/env bash
# Writes an ASCII-armored detached GPG signature <file>.asc for each file in a
# directory. RELEASE_GPG_PRIVATE_KEY holds the armored private key and
# RELEASE_GPG_PASSPHRASE its optional passphrase. Without a key, the script
# signs nothing and prints a warning.
#
# Usage: scripts/release/sign.sh <dir>
set -euo pipefail
if [[ $# -ne 1 ]]; then
  echo "usage: $0 <dir>" >&2
  exit 2
fi
if [[ -z "${RELEASE_GPG_PRIVATE_KEY:-}" ]]; then
  echo "::warning::RELEASE_GPG_PRIVATE_KEY is not set, so the release has no .asc signatures"
  exit 0
fi
GNUPGHOME="$(mktemp -d)"
export GNUPGHOME
trap 'gpgconf --kill gpg-agent >/dev/null 2>&1 || true; rm -rf "$GNUPGHOME"' EXIT
printf '%s\n' "$RELEASE_GPG_PRIVATE_KEY" | gpg --batch --quiet --import
gpg --batch --armor --export > "$1/rilgate-release-key.asc"
for file in "$1"/*; do
  case "$file" in
    *.asc) continue ;;
  esac
  printf '%s' "${RELEASE_GPG_PASSPHRASE:-}" | gpg --batch --yes --pinentry-mode loopback \
    --passphrase-fd 0 --armor --detach-sign --output "$file.asc" "$file"
done
echo "Signed:"
ls -1 "$1"/*.asc
