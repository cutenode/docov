#!/usr/bin/env bash
# What CI checks: formatting, clippy (for Windows too when its target is installed), the tests
# under coverage, the Hegel properties' own coverage, and the npm launcher's tests under coverage.
# Hegel runs derandomized, so it draws the same cases every run and the coverage numbers don't move.
set -euo pipefail
cd "$(dirname "$0")/.."

cargo fmt --check
cargo clippy --all-targets -- -D warnings
if rustup target list --installed | grep -qx x86_64-pc-windows-msvc; then
  cargo clippy --all-targets --target x86_64-pc-windows-msvc -- -D warnings
fi
export HEGEL_DERANDOMIZE=true

# cargo coverage, writing lcov to $LCOV_DIR/$1.lcov in place of its table when LCOV_DIR is set (CI
# sets it, for the job summary and the coverage artifact). The thresholds hold either way.
coverage() {
  local name=$1
  shift
  if [[ -z "${LCOV_DIR:-}" ]]; then
    cargo coverage "$@"
    return
  fi
  mkdir -p "$LCOV_DIR"
  cargo coverage "$@" --lcov --output-path "$LCOV_DIR/$name.lcov"
}
coverage suite
coverage props --test props
# No branch threshold: the launcher's one branch V8 counts is the Windows `.exe` name, and each OS
# only takes one side of it.
node --test --experimental-test-coverage --test-coverage-include='npm/docov/**' \
  --test-coverage-lines=100 --test-coverage-functions=100 npm/launcher.test.mjs
