#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
cargo xtask build-wasm
cargo xtask build-web --check
cargo fmt --all -- --check
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
node --test tests/*.test.mjs
