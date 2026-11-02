#!/usr/bin/env bash
# Runs one fuzz target for a number of seconds. Every generated file goes below fuzz/work/:
# the build in target/, the inputs that libFuzzer finds in corpus/<target>/ and failing inputs
# in artifacts/<target>/. The seed inputs in seeds/<target>/ are only read.
set -euo pipefail
target="${1:?usage: fuzz/run.sh <target> [seconds]}"
seconds="${2:-60}"
cd "$(dirname "$0")"
# cargo-fuzz always creates its default artifacts/<target>/ directory. It stays empty because
# libFuzzer writes to work/artifacts/, so it is removed after the run when it is empty.
trap 'rmdir "artifacts/$target" artifacts 2>/dev/null || true' EXIT
mkdir -p "work/corpus/$target" "work/artifacts/$target"
corpus=("work/corpus/$target")
if [[ -d "seeds/$target" ]]; then corpus+=("seeds/$target"); fi
cargo +"${FUZZ_TOOLCHAIN:-nightly}" fuzz run --target-dir work/target "$target" "${corpus[@]}" -- \
  -max_total_time="$seconds" -rss_limit_mb=4096 -artifact_prefix="work/artifacts/$target/"
