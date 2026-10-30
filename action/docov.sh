#!/usr/bin/env bash
# The GitHub Action (action.yml): runs docov once for annotations and a Markdown report, and puts
# the report in the job summary and a sticky pull request comment. Exits with docov's code.
set -euo pipefail

# Its own folder, so a second run in the job never reads the first one's report.
out=$(mktemp -d "$RUNNER_TEMP/docov.XXXXXX")

# One comment per pull request, found by its hidden marker and updated in place.
comment() {
  local marker='<!-- docov -->' comments=repos/$GITHUB_REPOSITORY/issues/$PR_NUMBER/comments id
  { echo "$marker"; cat "$out/report.md"; } >"$out/comment.md"
  id=$(gh api "$comments" --paginate --jq ".[] | select(.body | startswith(\"$marker\")) | .id" | head -n1) || return
  if [[ -n $id ]]; then
    gh api -X PATCH "repos/$GITHUB_REPOSITORY/issues/comments/$id" -F "body=@$out/comment.md" >/dev/null
  else
    gh api "$comments" -F "body=@$out/comment.md" >/dev/null
  fi
}

# The action's own version when it's published, so a pinned action runs the docov it shipped with.
version=$INPUT_VERSION
if [[ -z $version ]]; then
  version=$(sed -n 's/^version = "\(.*\)"$/\1/p' "$GITHUB_ACTION_PATH/Cargo.toml" | head -n1)
  if ! npm view "@docov/cli@$version" version >/dev/null 2>&1; then
    version=latest
  fi
fi

# The extra arguments, split the way a shell would, quotes and all, without running anything.
args=()
while IFS= read -r arg; do
  args+=("$arg")
done < <(printf '%s' "$INPUT_ARGS" | xargs -r -n1 printf '%s\n')
args+=(--format github --format "markdown=$out/report.md")
if [[ -n $INPUT_COMPARE ]]; then
  args+=(--compare "$INPUT_COMPARE")
fi

code=0
npx --yes "@docov/cli@$version" "${args[@]}" || code=$?
echo "markdown=$out/report.md" >>"$GITHUB_OUTPUT"
# docov writes no report when it can't scan (exit 2), and its error said why.
if [[ ! -f $out/report.md ]]; then
  exit "$code"
fi
if [[ $INPUT_SUMMARY == true ]]; then
  cat "$out/report.md" >>"$GITHUB_STEP_SUMMARY"
fi
# A token that can't write (on a pull request from a fork) only warns: coverage passed or failed
# either way.
if [[ $INPUT_COMMENT == true && -n $PR_NUMBER ]] && ! comment; then
  echo "::warning title=docov::couldn't comment on the pull request, which needs pull-requests: write"
fi
exit "$code"
