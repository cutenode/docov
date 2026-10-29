#!/usr/bin/env bash
# What CI checks: formatting, clippy (for Windows too when its target is installed), the tests
# under coverage, and the Hegel properties' own coverage. Hegel runs derandomized, so it draws the
# same cases every run and the coverage numbers don't move.
set -euo pipefail
cd "$(dirname "$0")/.."

cargo fmt --check
cargo clippy --all-targets -- -D warnings
if rustup target list --installed | grep -qx x86_64-pc-windows-msvc; then
  cargo clippy --all-targets --target x86_64-pc-windows-msvc -- -D warnings
fi
export HEGEL_DERANDOMIZE=true
cargo coverage
cargo coverage --test props
