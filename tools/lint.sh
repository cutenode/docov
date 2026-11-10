#!/usr/bin/env bash
# Formatting, clippy (for Windows too when its target is installed), rustdoc, the npm config's
# hardening, and shellcheck, cargo-deny, zizmor, actionlint and knip where they're installed (CI's
# Linux lint job has them all). CI's lint check.
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

# The npm config in the root and npm/docov (.npmrc and package.json) holds @lavamoat/harden's
# moderate level, plus the opinions harden-decisions.json turns on and minus the ones it turns off.
# `harden check` takes a level but no decisions, so this checks at strict and lets through what
# the decisions turn off. check exits 1 for those too, so jq decides.
declined=$(jq -c 'to_entries | map(select(.value == false).key)' harden-decisions.json)
for dir in . npm/docov; do
  report=$(cd "$dir" && npx -y @lavamoat/harden@0.8.1 check -p npm -l strict --json) || true
  jq -e --arg dir "$dir" --argjson declined "$declined" \
    '.checks | map(select(.satisfied != 1).id) - $declined |
      if . == [] then true else "harden: \($dir) is missing \(join(", "))\n" | halt_error end' \
    <<<"$report" >/dev/null
  # harden reads a key's first line in .npmrc and npm its last, so a second one would get past it.
  # Keys ending in [] are lists, set once per item.
  repeated=$(awk -F= '/^[[:space:]]*[^#;[:space:]]/ {
    key = $1
    gsub(/[[:space:]]/, "", key)
    if (key !~ /\[\]$/ && seen[key]++ == 1) print key
  }' "$dir/.npmrc")
  if [[ -n $repeated ]]; then
    echo "harden: $dir/.npmrc sets $repeated more than once, and npm takes the last" >&2
    exit 1
  fi
done

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
# Unused files, exports and dependencies in the JavaScript, configured in knip.jsonc. `npm ci`
# installs it.
if [[ -x node_modules/.bin/knip ]]; then
  npm run --silent knip
fi
