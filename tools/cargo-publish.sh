#!/usr/bin/env bash
# Publishes docov to crates.io unless Cargo.toml's version is already there, so a release that
# failed partway can run again.
set -euo pipefail
cd "$(dirname "$0")/.."

version=$(cargo metadata --no-deps --format-version 1 | jq -r '.packages[0].version')
# crates.io asks API clients for a User-Agent that says who they are.
if curl -sf -A "docov release (https://github.com/cutenode/docov)" "https://crates.io/api/v1/crates/docov/$version" >/dev/null; then
  echo "docov@$version is already on crates.io"
  exit
fi
cargo publish --locked
