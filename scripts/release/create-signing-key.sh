#!/usr/bin/env bash
# Creates the release signing key, stores it and its passphrase as the GitHub
# Actions secrets RELEASE_GPG_PRIVATE_KEY and RELEASE_GPG_PASSPHRASE, and writes
# an offline backup. An empty passphrase creates an unprotected key and removes
# RELEASE_GPG_PASSPHRASE. Run it on a trusted machine with gpg and an
# authenticated gh.
#
# Usage: scripts/release/create-signing-key.sh "<name> <email>" <backup-dir>
#   EXPIRY sets the key lifetime (default: 2y). REPO sets the repository
#   (default: ViGrise/rilgate).
set -euo pipefail
if [[ $# -ne 2 ]]; then
  echo "usage: $0 \"<name> <email>\" <backup-dir>" >&2
  exit 2
fi
user_id="$1"
backup="$2"
expiry="${EXPIRY:-2y}"
repo="${REPO:-ViGrise/rilgate}"
for tool in gpg gh; do
  command -v "$tool" >/dev/null 2>&1 || { echo "$tool is not installed" >&2; exit 1; }
done
gh auth status >/dev/null 2>&1 || { echo "gh is not authenticated; run gh auth login" >&2; exit 1; }
if [[ -e "$backup" && -n "$(ls -A "$backup")" ]]; then
  echo "$backup is not empty" >&2
  exit 1
fi

read -r -s -p "Passphrase for the new key: " passphrase
echo
read -r -s -p "Repeat the passphrase: " repeated
echo
[[ "$passphrase" == "$repeated" ]] || { echo "The passphrases are different" >&2; exit 1; }
if [[ -z "$passphrase" ]]; then
  echo "The passphrase is empty, so the secret key in the backup is not encrypted." >&2
fi

fingerprint="$(
  printf '%s' "$passphrase" | gpg --batch --pinentry-mode loopback --passphrase-fd 0 \
    --status-fd 1 --quick-generate-key "$user_id" ed25519 sign "$expiry" \
    | awk '$2 == "KEY_CREATED" {print $4}'
)"
[[ -n "$fingerprint" ]] || { echo "gpg did not create a key" >&2; exit 1; }

umask 077
mkdir -p "$backup"
printf '%s' "$passphrase" | gpg --batch --pinentry-mode loopback --passphrase-fd 0 \
  --armor --export-secret-keys "$fingerprint" > "$backup/rilgate-release-signing.secret.asc"
gpg --armor --export "$fingerprint" > "$backup/rilgate-release-signing.public.asc"
revocation="${GNUPGHOME:-$HOME/.gnupg}/openpgp-revocs.d/$fingerprint.rev"
[[ -f "$revocation" ]] && cp "$revocation" "$backup/rilgate-release-signing.revocation.asc"

gh secret set RELEASE_GPG_PRIVATE_KEY --repo "$repo" < "$backup/rilgate-release-signing.secret.asc"
if [[ -n "$passphrase" ]]; then
  printf '%s' "$passphrase" | gh secret set RELEASE_GPG_PASSPHRASE --repo "$repo"
else
  gh secret delete RELEASE_GPG_PASSPHRASE --repo "$repo" >/dev/null 2>&1 || true
fi

echo "Created key $fingerprint ($user_id, expires in $expiry)."
if [[ -n "$passphrase" ]]; then
  echo "Set RELEASE_GPG_PRIVATE_KEY and RELEASE_GPG_PASSPHRASE in $repo."
else
  echo "Set RELEASE_GPG_PRIVATE_KEY in $repo. RELEASE_GPG_PASSPHRASE is not set."
fi
echo "Move $backup to offline storage. It holds the secret key and the revocation certificate."
