#!/usr/bin/env bash
# Publishes the tarballs tools/npm-pack.sh made in DIR. The platform packages go first, so docov
# never lists a package that isn't on the registry yet. Versions already there are skipped, so a
# publish that failed partway can run again.
#
#   tools/npm-publish.sh DIR
set -euo pipefail
dir=$1

publish() {
  local spec
  spec=$(tar -xOzf "$1" package/package.json | jq -r '"\(.name)@\(.version)"')
  if [[ -n $(npm view "$spec" version 2>/dev/null) ]]; then
    echo "$spec is already published"
    return
  fi
  npm publish "$1"
}

# npm pack names @docov/<platform> docov-<platform>-<version>.tgz, and docov docov-<version>.tgz.
for tgz in "$dir"/docov-[a-z]*.tgz; do
  publish "$tgz"
done
publish "$dir"/docov-[0-9]*.tgz
