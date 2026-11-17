#!/usr/bin/env bash
# Builds the npm packages the way release.yml does, from a docov binary for this platform, checks
# @docov/cli with publint, packs both, installs the tarballs in an empty project and runs docov
# through the launcher, which has to print Cargo.toml's version. It works on a copy, since
# tools/npm-platforms.sh edits the checkout. CI's action jobs run it on their build; it needs
# `npm ci` for publint.
#
#   tools/npm-pack-check.sh BINARY
set -euo pipefail
binary=$(cd "$(dirname "$1")" && pwd)/$(basename "$1")
cd "$(dirname "$0")/.."
version=$(cargo metadata --no-deps --format-version 1 | jq -r '.packages[0].version')

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
# Git Bash's /tmp means nothing to Windows' npm and node.
if command -v cygpath >/dev/null; then
  work=$(cygpath -m "$work")
fi

platform=$(node -p 'process.platform + "-" + process.arch')
exe=docov
[[ $platform == win32-* ]] && exe=docov.exe
mkdir -p "$work/bins/$platform" "$work/repo/npm" "$work/repo/tools" "$work/packs" "$work/project"
cp "$binary" "$work/bins/$platform/$exe"
cp -R npm/docov "$work/repo/npm/"
cp README.md LICENSE "$work/repo/"
cp tools/npm-platforms.sh "$work/repo/tools/"
"$work/repo/tools/npm-platforms.sh" "$work/bins"

npm run --silent publint -- --strict "$work/repo/npm/docov"
for pkg in npm/docov "npm/platforms/$platform"; do
  (cd "$work/repo/$pkg" && npm pack --silent --pack-destination "$work/packs" >/dev/null)
done

# Offline: @docov/cli's one dependency is the platform package beside it.
cd "$work/project"
echo '{"private": true}' >package.json
npm install --silent --offline --no-audit --no-fund "$work"/packs/*.tgz
said=$(node_modules/.bin/docov --version)
if [[ $said != "docov $version" ]]; then
  echo "the installed docov --version printed \"$said\", not \"docov $version\"" >&2
  exit 1
fi
echo "@docov/cli and @docov/$platform $version pack, install and run"
