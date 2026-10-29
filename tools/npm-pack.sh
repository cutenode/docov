#!/usr/bin/env bash
# Packs docov's npm packages into OUT as tarballs: an @docov/<platform> package for each folder in
# BINS, holding the docov binary built for that platform, then docov itself (npm/docov), whose
# launcher runs whichever of them npm installed. BINS' folders are named for Node's
# `<process.platform>-<process.arch>` (darwin-arm64, win32-x64). Every package gets Cargo.toml's
# version, and docov depends on that exact version of each, the way Biome and esbuild pin theirs.
#
#   tools/npm-pack.sh BINS OUT
set -euo pipefail
bins=$(cd "$1" && pwd)
mkdir -p "$2"
out=$(cd "$2" && pwd)
cd "$(dirname "$0")/.."

version=$(cargo metadata --no-deps --format-version 1 | jq -r '.packages[0].version')
stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT

platforms=()
for dir in "$bins"/*/; do
  platform=$(basename "$dir")
  platforms+=("$platform")
  os=${platform%-*} cpu=${platform#*-}
  exe=docov
  [[ $os == win32 ]] && exe=docov.exe
  pkg=$stage/$platform
  mkdir "$pkg"
  cp "$dir/$exe" LICENSE "$pkg/"
  # Artifact downloads drop the executable bit, and npm packs the mode it finds.
  chmod +x "$pkg/$exe"
  # Scoped packages publish as private unless they say otherwise.
  jq --arg name "@docov/$platform" --arg version "$version" --arg os "$os" --arg cpu "$cpu" --arg exe "$exe" \
    '{name: $name, version: $version, description: "docov'\''s binary for \($os) \($cpu)", license, repository, os: [$os], cpu: [$cpu], files: [$exe], publishConfig: {access: "public"}}' \
    npm/docov/package.json >"$pkg/package.json"
  npm pack "$pkg" --pack-destination "$out"
done

cp -R npm/docov "$stage/docov"
cp README.md LICENSE "$stage/docov/"
jq --arg version "$version" '. + {version: $version, optionalDependencies: ($ARGS.positional | map({key: "@docov/\(.)", value: $version}) | from_entries)}' \
  npm/docov/package.json --args "${platforms[@]}" >"$stage/docov/package.json"
npm pack "$stage/docov" --pack-destination "$out"
