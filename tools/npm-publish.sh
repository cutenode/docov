#!/usr/bin/env bash
# Publishes the tarballs tools/npm-pack.sh made in DIR. The platform packages (the ones with an
# `os`) go first, so @docov/cli never lists a package that isn't on the registry yet. Versions
# already there are skipped, so a publish that failed partway can run again.
#
#   tools/npm-publish.sh DIR
set -euo pipefail
# Absolute, since npm reads a relative `npm-packages/x.tgz` as the GitHub repository npm-packages/x.tgz.
dir=$(cd "$1" && pwd)

manifest() {
  tar -xOzf "$1" package/package.json
}

publish() {
  local spec
  spec=$(manifest "$1" | jq -r '"\(.name)@\(.version)"')
  if [[ -n $(npm view "$spec" version 2>/dev/null) ]]; then
    echo "$spec is already published"
    return
  fi
  npm publish "$1"
}

for tgz in "$dir"/*.tgz; do
  if manifest "$tgz" | jq -e .os >/dev/null; then
    publish "$tgz"
  fi
done
for tgz in "$dir"/*.tgz; do
  if ! manifest "$tgz" | jq -e .os >/dev/null; then
    publish "$tgz"
  fi
done
