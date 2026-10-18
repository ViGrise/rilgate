#!/usr/bin/env bash
set -euo pipefail
root="$(cd "$(dirname "$0")/../.." && pwd)"
workdir="$(mktemp -d "/tmp/rilgate-browser-suite.XXXXXX")"
trap 'rm -rf "$workdir"' EXIT
cp "$root/tests/e2e/package.json" "$root/tests/e2e/package-lock.json" "$workdir/"
mkdir -p "$workdir/tests/e2e"
cp "$root/tests/e2e/"*.mjs "$workdir/tests/e2e/"
cd "$workdir"
npm ci --ignore-scripts
npx playwright install "$@" chromium firefox webkit
binary="${RILGATE_BINARY:-${CARGO_TARGET_DIR:-$root/target}/debug/rilgate}"
if [[ "$binary" != /* ]]; then binary="$root/$binary"; fi
RILGATE_PROJECT_ROOT="$root" RILGATE_BINARY="$binary" npx playwright test --config tests/e2e/playwright.config.mjs
