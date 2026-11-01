#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
cargo xtask build-wasm
cargo xtask build-web
cargo fmt --all -- --check
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
npm ci --prefix tests --no-audit --no-fund
tests/node_modules/.bin/biome ci .
node --test tests/*.test.mjs
