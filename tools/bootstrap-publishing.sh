#!/usr/bin/env bash
# Sets up what release.yml needs to publish: the `npm` and `crates-io` GitHub environments its
# publishing jobs run in, permission for Actions to open the release PR, and each registry trusting
# release.yml from its environment, staged publishing only on npm. Both registries only trust a
# workflow with a package they already have, so a crate or npm package that isn't there yet gets
# published first: the crate from this checkout, an npm package as a 0.0.0 placeholder. Finished
# steps are skipped, so run it again after a failure or after adding a platform to release.yml.
#
# npm drops a trust config that no publish has used within 48 hours, so merge the next release PR
# within 48 hours of running this, or run it again first with RETRUST=1 to renew them.
#
# Needs `gh` logged in as a repository admin, `npm login` as a member of the docov npm org
# (https://www.npmjs.com/org/create), and asks for a crates.io token with the publish-new,
# publish-update and trusted-publishing scopes (https://crates.io/settings/tokens/new).
set -euo pipefail
cd "$(dirname "$0")/.."
owner=cutenode repo=docov workflow=release.yml

npm whoami >/dev/null
if [[ -z ${CARGO_REGISTRY_TOKEN:-} ]]; then
  read -rsp "crates.io token: " CARGO_REGISTRY_TOKEN
  echo
  export CARGO_REGISTRY_TOKEN
fi

# An environment that's already there keeps whatever protection it has.
for environment in npm crates-io; do
  if gh api "repos/$owner/$repo/environments/$environment" >/dev/null 2>&1; then
    echo "GitHub environment $environment already exists"
    continue
  fi
  echo '{"deployment_branch_policy": {"protected_branches": false, "custom_branch_policies": true}}' |
    gh api -X PUT "repos/$owner/$repo/environments/$environment" --input - >/dev/null
  # Only release tags can deploy to it, so only they can publish.
  gh api "repos/$owner/$repo/environments/$environment/deployment-branch-policies" -f name='v*' -f type=tag >/dev/null
  echo "Created GitHub environment $environment for v* tags"
done
if [[ $(gh api "repos/$owner/$repo/actions/permissions/workflow" -q .can_approve_pull_request_reviews) != true ]]; then
  gh api -X PUT "repos/$owner/$repo/actions/permissions/workflow" -F can_approve_pull_request_reviews=true
  echo "Actions can open pull requests"
fi

tools/cargo-publish.sh
crates() {
  curl -sS --fail-with-body -A "docov bootstrap (https://github.com/$owner/$repo)" \
    -H @<(printf 'Authorization: %s\n' "$CARGO_REGISTRY_TOKEN") "$@"
}
config=$(jq -n --arg owner $owner --arg repo $repo --arg workflow $workflow \
  '{crate: "docov", repository_owner: $owner, repository_name: $repo, workflow_filename: $workflow, environment: "crates-io"}')
configs=https://crates.io/api/v1/trusted_publishing/github_configs
existing=$(crates "$configs?crate=docov" | jq --argjson config "$config" \
  '[.github_configs[] | select(.repository_owner == $config.repository_owner and .repository_name == $config.repository_name and .workflow_filename == $config.workflow_filename)]')
if jq -e --argjson config "$config" 'any(.environment == $config.environment)' <<<"$existing" >/dev/null; then
  echo "crates.io already trusts $workflow for docov"
else
  # One from before release.yml published from an environment.
  for id in $(jq -r '.[].id' <<<"$existing"); do
    crates -X DELETE "$configs/$id" >/dev/null
  done
  jq -n --argjson config "$config" '{github_config: $config}' |
    crates -X POST -H 'Content-Type: application/json' --data @- "$configs" >/dev/null
  echo "crates.io trusts $workflow for docov"
fi

for name in @docov/cli $(grep -oE 'platform: [a-z0-9]+-[a-z0-9]+' .github/workflows/$workflow | sed 's|platform: |@docov/|'); do
  if [[ -z $(npm view "$name" version 2>/dev/null) ]]; then
    stub=$(mktemp -d)
    jq --arg name "$name" '{name: $name, description, license, repository}' npm/docov/package.json >"$stub/package.json"
    npx -y setup-trusted-publishing@1.0.2 --cwd "$stub" --access public
    rm -rf "$stub"
  fi

  # npm keeps one config per package, and an expired one still lists, so replace whatever's there
  # unless it's the one uppt needs.
  trusts=$(npm trust list "$name" --json | jq -s .)
  if [[ -z ${RETRUST:-} ]] && jq -e --arg repo $owner/$repo --arg file $workflow \
    'length == 1 and (.[0] | .repository == $repo and .file == $file and .environment == "npm" and .permissions == ["createStagedPackage"])' \
    <<<"$trusts" >/dev/null; then
    echo "npm already trusts $workflow to stage $name"
    continue
  fi
  for id in $(jq -r '.[].id' <<<"$trusts"); do
    npm trust revoke "$name" --id "$id"
  done
  npm trust github "$name" --repo $owner/$repo --file $workflow --env npm --allow-stage-publish --yes
done
