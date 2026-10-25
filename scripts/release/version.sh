#!/usr/bin/env bash
# Prints the workspace package version from Cargo.toml.
set -euo pipefail
awk -F'"' '/^version = /{print $2; exit}' "$(dirname "$0")/../../Cargo.toml"
