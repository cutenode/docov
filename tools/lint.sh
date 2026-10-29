#!/usr/bin/env bash
# Formatting and clippy (for Windows too when its target is installed). CI's lint check.
set -euo pipefail
cd "$(dirname "$0")/.."

cargo fmt --check
cargo clippy --all-targets -- -D warnings
if rustup target list --installed | grep -qx x86_64-pc-windows-msvc; then
  cargo clippy --all-targets --target x86_64-pc-windows-msvc -- -D warnings
fi
