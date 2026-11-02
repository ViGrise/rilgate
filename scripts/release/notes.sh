#!/usr/bin/env bash
# Prints the Markdown body of the GitHub Release for a version. The body links
# to the release notes and upgrade guide on the platform docs, shows the install
# and verify commands, lists the changes since the previous release grouped by
# Conventional Commit type and sorted by scope, with the author and pull request
# of each change, and names the first-time contributors. A stable release compares with the previous stable
# release; a prerelease compares with the previous tag.
#
# The repository needs the full history and tags. GH_TOKEN (or a gh login) reads
# pull request numbers and new contributors from the GitHub API, and
# GITHUB_REPOSITORY selects the repository (default: ViGrise/rilgate).
#
# Usage: scripts/release/notes.sh <version> <release-dir>
set -euo pipefail
if [[ $# -ne 2 ]]; then
  echo "usage: $0 <version> <release-dir>" >&2
  exit 2
fi

version="$1"
release_dir="$2"
repo="${GITHUB_REPOSITORY:-ViGrise/rilgate}"
image="ghcr.io/${repo,,}"
docs="https://platform.vigrise.com/docs/open-source-software/rilgate"
tag="v$version"

git rev-parse --verify --quiet "refs/tags/$tag" >/dev/null || {
  echo "Tag $tag does not exist" >&2
  exit 1
}
if [[ "$version" == *-* ]]; then
  previous="$(git describe --tags --abbrev=0 --match 'v[0-9]*' "$tag^")"
else
  previous="$(git describe --tags --abbrev=0 --match 'v[0-9]*' --exclude 'v*-*' "$tag^")"
fi
previous_stable="$(git describe --tags --abbrev=0 --match 'v[0-9]*' --exclude 'v*-*' "$tag^")"

# Upgrade guide anchors drop the dots and any prerelease suffix: from-040-to-050.
anchor_version() {
  local core="${1#v}"
  core="${core%%-*}"
  printf '%s' "${core//./}"
}
upgrade_url="$docs/releases/upgrading#from-$(anchor_version "$previous_stable")-to-$(anchor_version "$tag")"

# The compare API maps each commit to the GitHub account that authored it. A
# commit whose email has no GitHub account shows its author name.
declare -A authors=()
while IFS=$'\t' read -r sha login; do
  authors[$sha]="$login"
done < <(gh api --paginate "repos/$repo/compare/$previous...$tag?per_page=100" \
  -q '.commits[] | "\(.sha)\t\(.author.login // "")"')

author() {
  local sha="$1"
  if [[ -n "${authors[$sha]:-}" ]]; then
    printf '@%s' "${authors[$sha]}"
  else
    git log -1 --format=%an "$sha"
  fi
}

# Squash merges end the subject with (#NN). Other commits get the number of
# their pull request from the API, or the short commit hash when none exists.
reference() {
  local sha="$1" subject="$2" number
  if [[ "$subject" =~ \(#([0-9]+)\)$ ]]; then
    printf '#%s' "${BASH_REMATCH[1]}"
    return
  fi
  number="$(gh api "repos/$repo/commits/$sha/pulls" -q '.[0].number // empty' 2>/dev/null || true)"
  if [[ -n "$number" ]]; then
    printf '#%s' "$number"
  else
    printf '%s' "${sha:0:7}"
  fi
}

declare -A groups=()
position=0
breaking=0
while IFS=$'\t' read -r sha subject; do
  ref="$(reference "$sha" "$subject")"
  [[ "$subject" =~ ^(.*)\ \(#[0-9]+\)$ ]] && subject="${BASH_REMATCH[1]}"
  subject="${subject% \[skip ci\]}"
  type="other" scope="" bang="" description="$subject"
  if [[ "$subject" =~ ^([a-z]+)(\(([^\)]+)\))?(!)?:\ (.+)$ ]]; then
    type="${BASH_REMATCH[1]}" scope="${BASH_REMATCH[3]}" bang="${BASH_REMATCH[4]}"
    description="${BASH_REMATCH[5]}"
  fi
  if [[ -n "$bang" ]] || git log -1 --format=%B "$sha" | grep -Eq '^BREAKING[ -]CHANGE:'; then
    group="breaking"
    breaking=1
  else
    case "$type" in
      feat) group="feat" ;;
      fix) group="fix" ;;
      docs) group="docs" ;;
      *) group="other" ;;
    esac
  fi
  # Each entry starts with its scope and commit position as sort keys. "~"
  # sorts after every scope, so items without a scope come last.
  position=$((position + 1))
  groups[$group]+="${scope:-~}"$'\t'"$position"$'\t'"- ${scope:+**$scope:** }${description^} ($(author "$sha") $ref)"$'\n'
done < <(git log --reverse --format='%H%x09%s' "$previous..$tag")

# GitHub's generated notes name the first-time contributors of the release.
new_contributors="$(gh api -X POST "repos/$repo/releases/generate-notes" \
  -f tag_name="$tag" -f previous_tag_name="$previous" -q .body |
  grep -E '^\* @[^ ]+ made their first contribution in ' || true)"

if ((breaking)); then
  cat <<EOF
> [!IMPORTANT]
> This release has breaking changes. Read [Upgrading from ${previous_stable#v} to $version]($upgrade_url) before you deploy.

EOF
fi
if [[ "$version" != *-* ]]; then
  echo "Read the [Rilgate $version release notes]($docs/releases/$version) for the main changes."
  echo
fi

cat <<EOF
## Install

\`\`\`bash
curl -fsSL https://github.com/$repo/releases/download/$tag/install.sh | VERSION=$tag bash
\`\`\`

\`\`\`powershell
\$env:VERSION = '$tag'; irm https://github.com/$repo/releases/download/$tag/install.ps1 | iex
\`\`\`

Container images for Linux amd64 and arm64 are \`$image:$version\` (Alpine) and \`$image:$version-debian\` (Debian). See [Container image tags]($docs/releases/overview#container-image-tags) for the other tags, and [Choose the artifact]($docs/releases/overview#choose-the-artifact) for the packages and archives below.

## Verify

EOF
if [[ -f "$release_dir/SHA256SUMS.asc" ]]; then
  cat <<'EOF'
```bash
gpg --import rilgate-release-key.asc
gpg --verify SHA256SUMS.asc SHA256SUMS
sha256sum --check --ignore-missing SHA256SUMS
```
EOF
else
  cat <<'EOF'
```bash
sha256sum --check --ignore-missing SHA256SUMS
```
EOF
fi

echo
echo "## Changes"
for group in breaking feat fix docs other; do
  [[ -n "${groups[$group]:-}" ]] || continue
  case "$group" in
    breaking) heading="Breaking changes" ;;
    feat) heading="Features" ;;
    fix) heading="Fixes" ;;
    docs) heading="Documentation" ;;
    other) heading="Other changes" ;;
  esac
  printf '\n### %s\n\n' "$heading"
  printf '%s' "${groups[$group]}" | LC_ALL=C sort -t $'\t' -k1,1 -k2,2n | cut -f3-
done

if [[ -n "$new_contributors" ]]; then
  printf '\n## New Contributors\n\n%s\n' "$new_contributors"
fi
printf '\n**Full Changelog**: https://github.com/%s/compare/%s...%s\n' "$repo" "$previous" "$tag"
