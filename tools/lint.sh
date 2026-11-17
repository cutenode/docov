#!/usr/bin/env bash
# Formatting, clippy (for each release target that's installed too), rustdoc, the npm config's
# hardening, and the linters below where they're installed. CI's lint check. CI's Linux lint job
# installs them all and sets REQUIRE_LINTERS=true, so there a missing one fails instead of being
# skipped.
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
# The release targets in build.yml, each where it's installed besides the host, so code behind a
# `cfg` for another OS or a dependency that doesn't build there fails here and not on the release
# tag. CI's lint jobs install the ones their runner doesn't build natively.
host=$(rustc -vV | sed -n 's/^host: //p')
installed=$(rustup target list --installed)
while read -r target; do
  if [[ $target != "$host" ]] && grep -qx "$target" <<<"$installed"; then
    cargo clippy --all-targets --target "$target" -- -D warnings
  fi
done < <(sed -n 's/.*target: \([a-z0-9_-]*\),.*/\1/p' .github/workflows/build.yml)
# Broken intra-doc links and bare URLs, which clippy doesn't check.
RUSTDOCFLAGS='-D warnings' cargo doc --no-deps --locked --document-private-items
if have shellcheck; then
  shellcheck action/*.sh tools/*.sh
fi
# Spelling, configured in _typos.toml.
if have typos; then
  typos
fi
# Links to files and anchors in the Markdown, configured in lychee.toml. scheduled.yml checks the
# rest weekly, so a site that's down doesn't fail a pull request.
if have lychee; then
  lychee --offline --no-progress .
fi
# Unused dependencies, and source files no module declares, configured in Cargo.toml.
if have cargo-shear; then
  cargo shear --deny-warnings --locked
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
# The JavaScript's lints (.oxlintrc.json) and its and the config files' formatting (.oxfmtrc.json,
# following .editorconfig). `npm ci` installs oxlint and oxfmt, and knip below.
if have oxlint; then
  npm run --silent oxlint
fi
if have oxfmt; then
  npm run --silent format -- --check
fi
# Unused files, exports and dependencies in the JavaScript, configured in knip.jsonc.
if have knip; then
  npm run --silent knip
fi
