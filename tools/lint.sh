#!/usr/bin/env bash
# Formatting, clippy (for Windows too when its target is installed), and shellcheck where it's
# installed (GitHub's Linux runners have it). CI's lint check.
set -euo pipefail
cd "$(dirname "$0")/.."

cargo fmt --check
cargo clippy --all-targets -- -D warnings
if rustup target list --installed | grep -qx x86_64-pc-windows-msvc; then
  cargo clippy --all-targets --target x86_64-pc-windows-msvc -- -D warnings
fi
if command -v shellcheck >/dev/null; then
  shellcheck action/*.sh tools/*.sh
fi
