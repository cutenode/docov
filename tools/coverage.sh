#!/usr/bin/env bash
# The tests under coverage, the Hegel properties' own coverage, and the npm launcher's tests under
# coverage, each failing under its thresholds. CI's coverage check. Hegel runs derandomized, so it
# draws the same cases every run and the coverage numbers don't move.
set -euo pipefail
cd "$(dirname "$0")/.."

export HEGEL_DERANDOMIZE=true

# cargo coverage, whose table lists any uncovered lines. With LCOV_DIR set (CI sets it, for the job
# summary and the coverage artifact), the same run is also written to $LCOV_DIR/$1.lcov, under the
# thresholds too, so the summary shows where it fell short.
coverage() {
  local name=$1 status=0
  shift
  cargo coverage "$@" || status=$?
  if [[ -n "${LCOV_DIR:-}" ]]; then
    mkdir -p "$LCOV_DIR"
    cargo llvm-cov report --lcov --output-path "$LCOV_DIR/$name.lcov"
  fi
  return "$status"
}
coverage suite
coverage props --test props
# No branch threshold: the launcher's one branch V8 counts is the Windows `.exe` name, and each OS
# only takes one side of it.
node --test --experimental-test-coverage --test-coverage-include='npm/docov/**' \
  --test-coverage-lines=100 --test-coverage-functions=100 npm/launcher.test.mjs
