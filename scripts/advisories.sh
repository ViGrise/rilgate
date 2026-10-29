#!/usr/bin/env bash
# Dependency advisory gate.
#
#   scripts/advisories.sh [--open-issues]
#
# Runs `cargo deny check advisories` against the committed lockfile. Each
# advisory must have an open GitHub issue with the label `dependency-advisory`
# and a title that starts with the advisory ID. A tracked advisory prints a
# warning. An untracked advisory fails the check. With --open-issues, the script
# also opens a tracking issue for each untracked advisory, and adds new upstream
# issues to each tracked one. Requires cargo-deny, jq and an authenticated gh.

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

# Prints the inverted dependency tree of each affected version of the advisory's
# package. A plain package name is ambiguous when the lockfile holds two versions.
dependency_tree() {
  local finding="$1" specs=() spec
  shift
  while IFS= read -r spec; do
    specs+=(-i "$spec")
  done < <(jq -r '.fields.advisory.package as $package
    | [.fields.graphs[]?.Krate.version] | unique[] | "\($package)@\(.)"' <<<"$finding")
  if [[ ${#specs[@]} == 0 ]]; then
    specs=(-i "$(jq -r '.fields.advisory.package' <<<"$finding")")
  fi
  cargo tree --locked -e normal "${specs[@]}" "$@"
}

github_repos() {
  sed -nE 's#^https://github\.com/([^/]+/[^/.]+).*#\1#p' | sort -u
}

# Lists issues in the GitHub repositories of the third-party crates on the
# dependency path of the advisory: issues that mention the advisory ID, and open
# issues with the package name in the title. The package's own repository is
# searched only for the ID, because most of its issues name the package.
upstream_issues() {
  local id="$1" package="$2" finding="$3" names metadata own_repo
  names="$(dependency_tree "$finding" --prefix none 2>/dev/null \
    | awk '{print $1}' | sort -u)"
  metadata="$(cargo metadata --locked --format-version 1 2>/dev/null)"
  own_repo="$(jq -r --arg package "$package" \
    '.packages[] | select(.name == $package) | .repository // empty' <<<"$metadata" | github_repos)"
  jq -r --arg names "$names" '
      ($names | split("\n")) as $path
      | .packages[]
      | select(.source != null and (.name as $name | $path | index($name)))
      | .repository // empty' <<<"$metadata" \
    | github_repos \
    | while read -r repo; do
        gh search issues "$id" --repo "$repo" --limit 5 --json url,title,state \
          --jq '.[] | "- [ ] \(.url) (\(.state)): \(.title)"' 2>/dev/null || true
        if ! grep -qxF "$repo" <<<"$own_repo"; then
          gh search issues "$package" --repo "$repo" --match title --state open --limit 5 \
            --json url,title,state --jq '.[] | "- [ ] \(.url) (\(.state)): \(.title)"' 2>/dev/null || true
        fi
        # The search API allows 30 requests per minute.
        sleep 4
      done \
    | awk '{ match($0, /https:[^ ]+/) } !seen[substr($0, RSTART, RLENGTH)]++'
}

# Adds new upstream issues to the "Upstream tracking" section of an open
# tracking issue, and updates the state of the issues that it already lists.
refresh_issue() {
  local issue="$1" id="$2" package="$3" finding="$4" upstream body updated
  body="$(gh issue view "$issue" --json body --jq .body)"
  grep -qx '## Upstream tracking' <<<"$body" || return 0
  upstream="$(
    upstream_issues "$id" "$package" "$finding"
    # The current state of each listed link. A merged pull request counts as closed.
    sed -n '/^## Upstream tracking$/,/^Close this issue/p' <<<"$body" \
      | sed -nE 's#^- \[.\] (https://github\.com/[^ ]+/(issues|pull)/[0-9]+) .*#\2 \1#p' \
      | while read -r kind url; do
          if [[ "$kind" == pull ]]; then kind=pr; else kind=issue; fi
          gh "$kind" view "$url" --json url,title,state --jq \
            '"- [ ] \(.url) (\(if .state == "OPEN" then "open" else "closed" end)): \(.title)"' 2>/dev/null || true
        done
  )"
  [[ -n "$upstream" ]] || return 0
  updated="$(UPSTREAM="$upstream" awk '
    function link(line) {
      match(line, /https:[^ ]+/)
      return substr(line, RSTART, RLENGTH)
    }
    BEGIN {
      # A later line for the same link carries a newer state.
      total = split(ENVIRON["UPSTREAM"], lines, "\n")
      for (i = 1; i <= total; i++) {
        address = link(lines[i])
        if (!(address in found)) url[++count] = address
        found[address] = lines[i]
        match(lines[i], /\((open|closed)\):/)
        state[address] = substr(lines[i], RSTART, RLENGTH)
      }
    }
    function finish(   i) {
      print ""
      for (i = 1; i <= kept; i++) print items[i]
      for (i = 1; i <= count; i++) if (!(url[i] in listed)) print found[url[i]]
      print ""
      section = 0
    }
    section && (/^## / || /^Close this issue/) { finish() }
    section {
      if ($0 == "") next
      if ($0 ~ /^- \[ \] Link the upstream issue/) next
      if ($0 ~ /^- \[.\] https:/) {
        listed[link($0)] = 1
        if (link($0) in state) sub(/\((open|closed)\):/, state[link($0)])
      }
      items[++kept] = $0
      next
    }
    /^## Upstream tracking$/ { section = 1 }
    { print }
    END { if (section) finish() }' <<<"$body")"
  if [[ "$updated" != "$body" ]]; then
    gh issue edit "$issue" --body "$updated" >/dev/null
    echo "::notice::${id}: updated the upstream tracking in ${issue}"
  fi
  comment_when_upstream_closed "$issue" "$id" "$updated"
}

# Comments once on a tracking issue when every upstream issue that it lists is
# closed, because a dependency update can then remove the advisory.
comment_when_upstream_closed() {
  local issue="$1" id="$2" body="$3" marker="<!-- upstream-closed -->" links
  links="$(sed -n '/^## Upstream tracking$/,/^Close this issue/p' <<<"$body" | grep -E '^- \[.\] https:' || true)"
  [[ -n "$links" ]] || return 0
  grep -qv '(closed):' <<<"$links" && return 0
  gh issue view "$issue" --json comments --jq '.comments[].body' | grep -qF "$marker" && return 0
  gh issue comment "$issue" --body "${marker}
All upstream issues in the tracking list are closed. Update the dependencies, then close this issue if the lockfile no longer contains an affected version." >/dev/null
  echo "::notice::${id}: all upstream issues are closed. Commented on ${issue}"
}

open_issue() {
  local finding="$1" id package version title body upstream path
  id="$(jq -r '.fields.advisory.id' <<<"$finding")"
  package="$(jq -r '.fields.advisory.package' <<<"$finding")"
  version="$(jq -r '[.fields.graphs[]?.Krate.version] | unique | join(", ")' <<<"$finding")"
  title="$(jq -r '.fields.advisory.title' <<<"$finding")"
  upstream="$(upstream_issues "$id" "$package" "$finding")"
  if [[ -z "$upstream" ]]; then
    upstream="- [ ] Link the upstream issue or pull request that removes or replaces \`${package}\`."
  fi
  path="$(dependency_tree "$finding" 2>&1 | sed "s# (${ROOT}/[^)]*)##" | head -n 40)"
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
    if [[ "$OPEN_ISSUES" == 1 ]]; then
      refresh_issue "$issue" "$id" "$package" "$finding"
    fi
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
