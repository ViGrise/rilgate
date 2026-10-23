#!/usr/bin/env bash
# Dependency advisory gate.
#
#   scripts/advisories.sh [--open-issues]
#
# Runs `cargo deny check advisories` against the committed lockfile. Each
# advisory must have an open GitHub issue with the label `dependency-advisory`
# and the advisory ID in its title. A tracked advisory prints a warning. An
# untracked advisory fails the check. With --open-issues, the script also opens
# a tracking issue for each untracked advisory. Requires cargo-deny, jq and an
# authenticated gh.

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

report="$(mktemp)"
trap 'rm -f "$report"' EXIT
# cargo-deny exits non-zero for any finding. The JSON summary decides the result.
cargo deny --locked --format json --config .config/deny.toml check advisories 2>"$report" || true

if ! jq -e 'select(.type == "summary")' "$report" >/dev/null 2>&1; then
  echo "::error::cargo deny did not produce a summary" >&2
  cat "$report" >&2
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

open_issue() {
  local id="$1" package="$2" title="$3" url="$4" references
  references="$(printf '%b' "$5")"
  local body
  body="$(cat <<EOF
\`cargo deny check advisories\` reports [${id}](https://rustsec.org/advisories/${id}) for \`${package}\`.

- Advisory: ${title}
- Source: ${url}
${references}

## Dependency path

\`\`\`text
$(cargo tree --locked -e normal -i "$package" 2>&1 | head -n 40)
\`\`\`

## Upstream tracking

- [ ] Link the upstream issue or pull request that removes or replaces \`${package}\`.

Close this issue when the lockfile no longer contains an affected version.
EOF
)"
  gh label create "$LABEL" --color B60205 \
    --description "A RustSec advisory that affects the dependency tree" >/dev/null 2>&1 || true
  gh issue create --label "$LABEL" --title "${id}: ${package} - ${title}" --body "$body"
}

while IFS=$'\t' read -r id package title url references; do
  issue="$(tracking_issue "$id")"
  if [[ -n "$issue" ]]; then
    echo "::warning::${id} (${package}) is tracked in ${issue}"
    continue
  fi
  failed=1
  if [[ "$OPEN_ISSUES" == 1 ]]; then
    issue="$(open_issue "$id" "$package" "$title" "$url" "$references")"
    echo "::error::${id} (${package}) is new. Opened ${issue}"
  else
    echo "::error::${id} (${package}) has no open issue with the label ${LABEL}"
  fi
done < <(jq -r 'select(.type == "diagnostic" and .fields.advisory != null)
  | .fields.advisory
  | [.id, .package, .title, (.url // ""),
     ((.references // []) | map("- Reference: " + .) | join("\\n"))]
  | @tsv' "$report" | sort -u)

exit "$failed"
