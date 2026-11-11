#!/usr/bin/env bash
# Formatting, clippy (for each release target that's installed too), rustdoc, the npm config's
# hardening, and shellcheck, cargo-deny, zizmor, actionlint and knip where they're installed. CI's
# lint check. CI's Linux lint job installs them all and sets REQUIRE_LINTERS=true, so there a missing
# one fails instead of being skipped.
set -euo pipefail
cd "$(dirname "$0")/.."
PATH=$PWD/node_modules/.bin:$PATH

# Whether linter $1 is installed, failing when it isn't and REQUIRE_LINTERS is true.
have() {
  command -v "$1" >/dev/null && return
  if [[ ${REQUIRE_LINTERS:-} == true ]]; then
    echo "$1 isn't installed, and REQUIRE_LINTERS is true" >&2
    exit 1
  fi
  return 1
}

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
# The targets release.yml builds, each where it's installed besides the host, so code behind a
# `cfg` for another OS or a dependency that doesn't build there fails here and not on the release
# tag. CI's lint jobs install the ones their runner doesn't build natively.
host=$(rustc -vV | sed -n 's/^host: //p')
installed=$(rustup target list --installed)
while read -r target; do
  if [[ $target != "$host" ]] && grep -qx "$target" <<<"$installed"; then
    cargo clippy --all-targets --target "$target" -- -D warnings
  fi
done < <(sed -n 's/.*target: \([a-z0-9_-]*\),.*/\1/p' .github/workflows/release.yml)
# Broken intra-doc links and bare URLs, which clippy doesn't check.
RUSTDOCFLAGS='-D warnings' cargo doc --no-deps --locked --document-private-items
if have shellcheck; then
  shellcheck action/*.sh tools/*.sh
fi
# Advisories, licenses and sources, configured in deny.toml.
if have cargo-deny; then
  cargo deny --locked check
fi
# The workflows and action.yml. zizmor runs its online audits (impostor commits, known-vulnerable
# actions) when GH_TOKEN is set, as scheduled.yml's weekly run does.
if have zizmor; then
  zizmor .
fi
if have actionlint; then
  actionlint
fi
# Unused files, exports and dependencies in the JavaScript, configured in knip.jsonc. `npm ci`
# installs it.
if have knip; then
  npm run --silent knip
fi
