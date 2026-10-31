#!/usr/bin/env bash
# Approves the npm packages release.yml staged for VERSION: the platform packages first, so
# @docov/cli never goes live listing one that isn't on the registry yet. npm asks for a second
# factor for each.
#
#   tools/npm-approve.sh 1.2.3
set -euo pipefail
version=$1

ids=$(npm stage list --json | jq -r --arg version "$version" \
  '[.[] | select(.version == $version and .status == "staged" and (.packageName | startswith("@docov/")))]
   | sort_by(.packageName == "@docov/cli") | .[].id')
if [[ -z $ids ]]; then
  echo "Nothing from docov $version is waiting for approval" >&2
  exit 1
fi
for id in $ids; do
  npm stage approve "$id"
done
