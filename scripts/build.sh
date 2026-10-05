#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
cargo xtask build-wasm
cargo build --locked "$@"
