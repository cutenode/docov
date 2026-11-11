#!/usr/bin/env bash
# Everything CI checks: tools/lint.sh, tools/coverage.sh, then the npm packages from a debug build
# (tools/npm-pack-check.sh).
set -euo pipefail
cd "$(dirname "$0")/.."

tools/lint.sh
tools/coverage.sh
cargo build --locked
binary=target/debug/docov
[[ -f $binary.exe ]] && binary+=.exe
tools/npm-pack-check.sh "$binary"
