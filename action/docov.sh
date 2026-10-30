#!/usr/bin/env bash
# The GitHub Action (action.yml): runs docov once for annotations and a Markdown report, puts the
# report in the job summary and a sticky pull request comment, and with PUBLISH (a push to the
# default branch) commits the json report and a shields.io badge to the data branch. Exits with
# docov's code.
set -euo pipefail

# Its own folder, so a second run in the job never reads the first one's report.
out=$(mktemp -d "$RUNNER_TEMP/docov.XXXXXX")
remote=$GITHUB_SERVER_URL/$GITHUB_REPOSITORY.git

# git with the token, sent the way actions/checkout sends it, so it stays out of the remote URL.
git_auth() {
  local basic
  basic=$(printf 'x-access-token:%s' "$INPUT_TOKEN" | base64 | tr -d '\n')
  git -c "http.$GITHUB_SERVER_URL/.extraheader=AUTHORIZATION: basic $basic" "$@"
}

# Fetches the data branch into a new repository at $1, failing when there's no data branch yet.
fetch_data() {
  git_auth ls-remote --exit-code --heads "$remote" "$INPUT_DATA_BRANCH" >/dev/null || return
  git init -q "$1"
  git_auth -C "$1" fetch -q --depth 1 "$remote" "$INPUT_DATA_BRANCH"
}

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

# Commits the reports to the data branch, so its history is the trend and its latest files are what
# badges and pull requests read.
publish() {
  local data=$out/data
  if fetch_data "$data"; then
    git -C "$data" checkout -q -B "$INPUT_DATA_BRANCH" FETCH_HEAD
  else
    git init -q "$data"
    git -C "$data" checkout -q --orphan "$INPUT_DATA_BRANCH"
  fi
  cp "$out/docov.json" "$out/shields.json" "$data/"
  git -C "$data" add docov.json shields.json
  if git -C "$data" diff --cached --quiet; then
    return
  fi
  git -C "$data" -c user.name='github-actions[bot]' \
    -c user.email='41898282+github-actions[bot]@users.noreply.github.com' commit -q -m "docov for $GITHUB_SHA"
  git_auth -C "$data" push -q "$remote" "HEAD:refs/heads/$INPUT_DATA_BRANCH"
}

# The action's own version when it's published, so a pinned action runs the docov it shipped with.
version=$INPUT_VERSION
if [[ -z $version ]]; then
  version=$(sed -n 's/^version = "\(.*\)"$/\1/p' "$GITHUB_ACTION_PATH/Cargo.toml" | head -n1)
  if ! npm view "@docov/cli@$version" version >/dev/null 2>&1; then
    version=latest
  fi
fi

# A pull request compares with the default branch's latest report, from the data branch.
compare=$INPUT_COMPARE
if [[ -z $compare && -n $INPUT_DATA_BRANCH && -n $PR_NUMBER ]] && fetch_data "$out/base" &&
  git -C "$out/base" show FETCH_HEAD:docov.json >"$out/base.json" 2>/dev/null; then
  compare=$out/base.json
fi

# The extra arguments, split the way a shell would, quotes and all, without running anything.
args=()
while IFS= read -r arg; do
  args+=("$arg")
done < <(printf '%s' "$INPUT_ARGS" | xargs -r -n1 printf '%s\n')
args+=(--format github --format "markdown=$out/report.md")
if [[ -n $compare ]]; then
  args+=(--compare "$compare")
fi
if [[ $PUBLISH == true ]]; then
  args+=(--format "json=$out/docov.json" --format "shields=$out/shields.json")
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
if [[ $PUBLISH == true ]]; then
  publish
fi
exit "$code"
