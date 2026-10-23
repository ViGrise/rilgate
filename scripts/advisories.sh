#!/usr/bin/env bash
# Dependency advisory gate.
#
#   scripts/advisories.sh [--open-issues]
#
# Runs `cargo deny check advisories` against the committed lockfile. Each
# advisory must have an open GitHub issue with the label `dependency-advisory`
# and a title that starts with the advisory ID. A tracked advisory prints a
# warning. An untracked advisory fails the check. With --open-issues, the script
# also opens a tracking issue for each untracked advisory. Requires cargo-deny,
# jq and an authenticated gh.

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

LABEL="dependency-advisory"
OPEN_ISSUES=0
for arg in "$@"; do
  case "$arg" in
    --open-issues) OPEN_ISSUES=1 ;;
    *) echo "usage: scripts/advisories.sh [--open-issues]" >&2; exit 2 ;;
  esac
done

output="$(mktemp)"
report="$(mktemp)"
trap 'rm -f "$output" "$report"' EXIT
# cargo-deny exits non-zero for any finding. The JSON summary decides the result.
cargo deny --locked --format json --config .config/deny.toml check advisories 2>"$output" || true
# The same stream carries plain-text toolchain messages, for example from rustup.
grep '^{' "$output" >"$report" || true

if ! jq -e 'select(.type == "summary")' "$report" >/dev/null 2>&1; then
  echo "::error::cargo deny did not produce a summary" >&2
  cat "$output" >&2
  exit 1
fi

failed=0

# Findings without an advisory record (for example a yanked crate) need a code change.
while IFS=$'\t' read -r code message; do
  echo "::error::cargo deny ${code}: ${message}"
  failed=1
done < <(jq -r 'select(.type == "diagnostic" and .fields.severity == "error" and .fields.advisory == null)
  | [.fields.code, .fields.message] | @tsv' "$report")

# Listing reads issues directly. The search index can miss a new issue.
tracking_issue() {
  gh issue list --state open --label "$LABEL" --limit 500 --json title,url \
    --jq "map(select(.title | startswith(\"$1:\"))) | first | .url // empty"
}

# Lists issues that mention the advisory in the GitHub repositories of the
# third-party crates on its dependency path.
upstream_issues() {
  local id="$1" package="$2" names
  names="$(cargo tree --locked -e normal -i "$package" --prefix none 2>/dev/null \
    | awk '{print $1}' | sort -u)"
  cargo metadata --locked --format-version 1 2>/dev/null \
    | jq -r --arg names "$names" '
        ($names | split("\n")) as $path
        | .packages[]
        | select(.source != null and (.name as $name | $path | index($name)))
        | .repository // empty' \
    | sed -nE 's#^https://github\.com/([^/]+/[^/.]+).*#\1#p' | sort -u \
    | while read -r repo; do
        gh search issues "$id" --repo "$repo" --limit 5 --json url,title,state \
          --jq '.[] | "- [ ] \(.url) (\(.state)): \(.title)"' 2>/dev/null || true
      done
}

open_issue() {
  local finding="$1" id package version title body upstream path
  id="$(jq -r '.fields.advisory.id' <<<"$finding")"
  package="$(jq -r '.fields.advisory.package' <<<"$finding")"
  version="$(jq -r '[.fields.graphs[]?.Krate.version] | unique | join(", ")' <<<"$finding")"
  title="$(jq -r '.fields.advisory.title' <<<"$finding")"
  upstream="$(upstream_issues "$id" "$package")"
  if [[ -z "$upstream" ]]; then
    upstream="- [ ] Link the upstream issue or pull request that removes or replaces \`${package}\`."
  fi
  path="$(cargo tree --locked -e normal -i "$package" 2>&1 | sed "s# (${ROOT}/[^)]*)##" | head -n 40)"
  body="$(jq -r --arg version "$version" --arg path "$path" --arg upstream "$upstream" '
    .fields as $f | $f.advisory as $a
    | "> \($a.title)\n\n"
    + "| Details | |\n| --- | --- |\n"
    + "| Status | \($f.code) |\n"
    + "| Package | `\($a.package)` |\n"
    + "| Version | `\($version)` |\n"
    + (if $a.url then "| URL | [\($a.url)](\($a.url)) |\n" else "" end)
    + "| Date | \($a.date) |\n\n"
    + "\($a.description)\n\n"
    + "See the [advisory page](https://rustsec.org/advisories/\($a.id).html) for more details.\n\n"
    + "## Dependency path\n\n```text\n\($path)\n```\n\n"
    + "## Upstream tracking\n\n\($upstream)\n\n"
    + "Close this issue when the lockfile no longer contains an affected version."' <<<"$finding")"
  gh label create "$LABEL" --color B60205 \
    --description "A RustSec advisory that affects the dependency tree" >/dev/null 2>&1 || true
  gh issue create --label "$LABEL" --title "${id}: ${title}" --body "$body"
}

while IFS= read -r finding; do
  id="$(jq -r '.fields.advisory.id' <<<"$finding")"
  package="$(jq -r '.fields.advisory.package' <<<"$finding")"
  issue="$(tracking_issue "$id")"
  if [[ -n "$issue" ]]; then
    echo "::warning::${id} (${package}) is tracked in ${issue}"
    continue
  fi
  failed=1
  if [[ "$OPEN_ISSUES" == 1 ]]; then
    issue="$(open_issue "$finding")"
    echo "::error::${id} (${package}) is new. Opened ${issue}"
  else
    echo "::error::${id} (${package}) has no open issue with the label ${LABEL}"
  fi
done < <(jq -c 'select(.type == "diagnostic" and .fields.advisory != null)' "$report" \
  | jq -sc 'unique_by(.fields.advisory.id) | .[]')

exit "$failed"
