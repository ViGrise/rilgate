#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
if [[ -z "${CARGO_TARGET_DIR:-}" ]]; then
  export CARGO_TARGET_DIR
  CARGO_TARGET_DIR="$(mktemp -d /tmp/rilgate-tests.XXXXXX)"
  trap 'rm -rf "$CARGO_TARGET_DIR"' EXIT
fi
cargo xtask build-wasm
cargo fmt --all -- --check
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
node --test tests/*.test.mjs
