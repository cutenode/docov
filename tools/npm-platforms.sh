#!/usr/bin/env bash
# Fills in npm/ for uppt/pack: an @docov/<platform> package in npm/platforms/<platform> for each
# folder in BINS, holding the docov binary built for that platform, and @docov/cli (npm/docov),
# whose launcher runs whichever of them npm installed. BINS' folders are named for Node's
# `<process.platform>-<process.arch>` (darwin-arm64, win32-x64). Every package gets
# npm/docov/package.json's version, and @docov/cli depends on that exact version of each, the way
# Biome and esbuild pin theirs. It edits the checkout, so release.yml runs it on a fresh one.
#
#   tools/npm-platforms.sh BINS
set -euo pipefail
bins=$(cd "$1" && pwd)
cd "$(dirname "$0")/.."

platforms=()
for dir in "$bins"/*/; do
  platform=$(basename "$dir")
  platforms+=("$platform")
  os=${platform%-*} cpu=${platform#*-}
  exe=docov
  [[ $os == win32 ]] && exe=docov.exe
  pkg=npm/platforms/$platform
  mkdir -p "$pkg"
  cp "$dir/$exe" LICENSE "$pkg/"
  # Artifact downloads drop the executable bit, and npm packs the mode it finds.
  chmod +x "$pkg/$exe"
  # Scoped packages publish as private unless they say otherwise.
  jq --arg name "@docov/$platform" --arg os "$os" --arg cpu "$cpu" --arg exe "$exe" \
    '{name: $name, version, description: "docov'\''s binary for \($os) \($cpu)", license, repository, os: [$os], cpu: [$cpu], files: [$exe], publishConfig: {access: "public"}}' \
    npm/docov/package.json >"$pkg/package.json"
done

cp README.md LICENSE npm/docov/
manifest=$(jq '. as $pkg | . + {optionalDependencies: ($ARGS.positional | map({key: "@docov/\(.)", value: $pkg.version}) | from_entries)}' \
  npm/docov/package.json --args "${platforms[@]}")
echo "$manifest" >npm/docov/package.json
