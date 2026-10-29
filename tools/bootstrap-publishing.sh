#!/usr/bin/env bash
# Sets up trusted publishing for release.yml. crates.io and npm only trust a workflow with a
# package they already have, so this publishes the first version from here: the crate from this
# checkout, and the npm packages from release.yml's latest successful run on main. Then it trusts
# release.yml for the crate and for each npm package. Finished steps are skipped, so run it again
# after a failure or after adding a platform to release.yml.
#
# npm drops a trust config that no publish has used within 48 hours, so push the next release tag
# within 48 hours of running this, or run it again first to renew them.
#
# Needs `npm login` as a member of the docov npm org (https://www.npmjs.com/org/create), and asks for a crates.io token with the publish-new, publish-update and
# trusted-publishing scopes (https://crates.io/settings/tokens/new).
set -euo pipefail
cd "$(dirname "$0")/.."
owner=cutenode repo=docov workflow=release.yml

npm whoami >/dev/null
if [[ -z ${CARGO_REGISTRY_TOKEN:-} ]]; then
  read -rsp "crates.io token: " CARGO_REGISTRY_TOKEN
  echo
  export CARGO_REGISTRY_TOKEN
fi
version=$(cargo metadata --no-deps --format-version 1 | jq -r '.packages[0].version')

tools/cargo-publish.sh
crates() {
  curl -sS --fail-with-body -A "docov bootstrap (https://github.com/$owner/$repo)" \
    -H @<(printf 'Authorization: %s\n' "$CARGO_REGISTRY_TOKEN") "$@"
}
config=$(jq -n --arg owner $owner --arg repo $repo --arg workflow $workflow \
  '{crate: "docov", repository_owner: $owner, repository_name: $repo, workflow_filename: $workflow, environment: null}')
configs=https://crates.io/api/v1/trusted_publishing/github_configs
trusted=$(crates "$configs?crate=docov" | jq --argjson config "$config" \
  '[.github_configs[] | select(.repository_owner == $config.repository_owner and .repository_name == $config.repository_name and .workflow_filename == $config.workflow_filename)] | length')
if [[ $trusted == 0 ]]; then
  jq -n --argjson config "$config" '{github_config: $config}' |
    crates -X POST -H 'Content-Type: application/json' --data @- "$configs" >/dev/null
  echo "crates.io trusts $workflow for docov"
else
  echo "crates.io already trusts $workflow for docov"
fi

run=$(gh run list -R $owner/$repo --workflow $workflow --branch main --status success -L 1 --json databaseId -q '.[0].databaseId')
if [[ -z $run ]]; then
  echo "$workflow hasn't run on main. Start it with \`gh workflow run $workflow -R $owner/$repo --ref main\` and run this again when it's done." >&2
  exit 1
fi
packages=$(mktemp -d)
trap 'rm -rf "$packages"' EXIT
gh run download "$run" -R $owner/$repo -n npm-packages -D "$packages"
if [[ ! -f $packages/docov-cli-$version.tgz ]]; then
  echo "$workflow's latest run on main didn't pack docov $version. Start one with \`gh workflow run $workflow -R $owner/$repo --ref main\` and run this again when it's done." >&2
  exit 1
fi
tools/npm-publish.sh "$packages"

for tgz in "$packages"/*.tgz; do
  name=$(tar -xOzf "$tgz" package/package.json | jq -r .name)
  # A version from CI has a provenance attestation. That publish validated the trust config, so
  # it no longer expires.
  if [[ -n $(npm view "$name" dist.attestations) ]]; then
    echo "npm trusts $workflow for $name, and CI has published it"
    continue
  fi
  # npm keeps one config per package, and an expired one still lists, so replace whatever's there.
  for id in $(npm trust list "$name" --json | jq -r .id); do
    npm trust revoke "$name" --id "$id"
  done
  npm trust github "$name" --repo $owner/$repo --file $workflow --allow-publish --yes
done
