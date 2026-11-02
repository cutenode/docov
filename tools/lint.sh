#!/usr/bin/env bash
# Formatting, clippy (for Windows too when its target is installed), rustdoc, and shellcheck,
# cargo-deny, zizmor and actionlint where they're installed (CI's Linux lint job has them all).
# CI's lint check.
set -euo pipefail
cd "$(dirname "$0")/.."

# Cargo.toml's version matches npm/docov/package.json's: release.yml bumps both together and
# publishes the crate and the npm packages at that version.
cargo_version=$(cargo metadata --no-deps --format-version 1 | jq -r '.packages[0].version')
npm_version=$(jq -r .version npm/docov/package.json)
if [[ $cargo_version != "$npm_version" ]]; then
  echo "Cargo.toml's version is $cargo_version, but npm/docov/package.json's is $npm_version" >&2
  exit 1
fi

cargo fmt --check
cargo clippy --all-targets -- -D warnings
if rustup target list --installed | grep -qx x86_64-pc-windows-msvc; then
  cargo clippy --all-targets --target x86_64-pc-windows-msvc -- -D warnings
fi
# Broken intra-doc links and bare URLs, which clippy doesn't check.
RUSTDOCFLAGS='-D warnings' cargo doc --no-deps --locked --document-private-items
if command -v shellcheck >/dev/null; then
  shellcheck action/*.sh tools/*.sh
fi
# Advisories, licenses and sources, configured in deny.toml.
if command -v cargo-deny >/dev/null; then
  cargo deny --locked check
fi
# The workflows and action.yml. zizmor runs its online audits (impostor commits, known-vulnerable
# actions) when GH_TOKEN is set, as on CI.
if command -v zizmor >/dev/null; then
  zizmor .
fi
if command -v actionlint >/dev/null; then
  actionlint
fi
